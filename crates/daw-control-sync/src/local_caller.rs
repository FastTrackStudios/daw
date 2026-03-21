//! In-process roam caller using memory channels.
//!
//! Wraps `roam::memory_link_pair` + acceptor/initiator into a reusable struct
//! that any in-process consumer (plugins, extensions, desktop apps) can use
//! to get an `ErasedCaller` without duplicating the boilerplate.
//!
//! Uses roam's virtual connection pattern: the root session is established
//! with an `on_connection` acceptor, then the client opens a virtual connection
//! to get a service-specific `Driver` and `ErasedCaller`.

use moire::task::JoinHandle;
use roam::{
    AcceptedConnection, BareConduit, ConnectionAcceptor, ConnectionHandle, ConnectionId,
    ConnectionSettings, Driver, DriverCaller, DriverReplySink, ErasedCaller, Handler,
    MetadataEntry, MetadataFlags, MetadataValue, Parity,
};
use std::sync::Arc;
use tracing::{debug, warn};

/// Keeps the server-side acceptor task alive.
struct KeepAlive {
    _handle: JoinHandle<()>,
}

/// Wraps a `Handler` into a `ConnectionAcceptor` that spawns a `Driver`
/// per virtual connection.
struct LocalAcceptor<H> {
    handler: Arc<H>,
}

impl<H: Handler<DriverReplySink> + Clone + 'static> ConnectionAcceptor for LocalAcceptor<H> {
    fn accept(
        &self,
        _conn_id: ConnectionId,
        peer_settings: &ConnectionSettings,
        _metadata: &[MetadataEntry],
    ) -> Result<AcceptedConnection, roam::Metadata<'static>> {
        let handler = Arc::clone(&self.handler);
        let settings = ConnectionSettings {
            parity: peer_settings.parity.other(),
            max_concurrent_requests: 64,
        };
        Ok(AcceptedConnection {
            settings,
            metadata: vec![],
            setup: Box::new(move |handle: ConnectionHandle| {
                let mut driver = Driver::new(handle, handler.as_ref().clone());
                moire::task::spawn(async move { driver.run().await });
            }),
        })
    }
}

/// In-process roam caller backed by memory channels.
///
/// Creates a `memory_link_pair`, spawns an acceptor task for the server side
/// with a `ConnectionAcceptor`, and establishes an initiator on the client side.
/// The client then opens a virtual connection to get an `ErasedCaller` for RPC.
///
/// # Example
///
/// ```ignore
/// let handler = RoutedHandler::new()
///     .with(fx_service_descriptor(), FxServiceDispatcher::new(fx_impl));
/// let local = LocalCaller::new(handler).await?;
/// let daw = Daw::new(local.erased_caller());
/// ```
#[derive(Clone)]
pub struct LocalCaller {
    caller: ErasedCaller,
    _keep_alive: Arc<KeepAlive>,
}

impl LocalCaller {
    /// Create a new in-process caller from any handler.
    ///
    /// Spawns a background task that accepts virtual connections and dispatches
    /// requests via an in-memory link pair. The task lives as long as any
    /// `LocalCaller` clone exists.
    pub async fn new(
        handler: impl Handler<DriverReplySink> + Clone + 'static,
    ) -> eyre::Result<Self> {
        let (client_link, server_link) = roam::memory_link_pair(256);

        let acceptor = LocalAcceptor {
            handler: Arc::new(handler),
        };

        // Server side: accept with virtual connection support
        let handle = moire::task::spawn(async move {
            let handshake = roam::HandshakeResult {
                role: roam::SessionRole::Acceptor,
                our_settings: ConnectionSettings {
                    parity: Parity::Even,
                    max_concurrent_requests: 64,
                },
                peer_settings: ConnectionSettings {
                    parity: Parity::Odd,
                    max_concurrent_requests: 64,
                },
                peer_supports_retry: true,
                session_resume_key: None,
                peer_resume_key: None,
                our_schema: vec![],
                peer_schema: vec![],
            };
            match roam::acceptor(BareConduit::new(server_link), handshake)
                .on_connection(acceptor)
                .establish::<DriverCaller>(())
                .await
            {
                Ok((_caller, _session)) => {
                    debug!("LocalCaller server session established");
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    warn!("LocalCaller server accept failed: {:?}", e);
                }
            }
        });

        // Client side: establish root session
        let handshake = roam::HandshakeResult {
            role: roam::SessionRole::Initiator,
            our_settings: ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: 64,
            },
            peer_settings: ConnectionSettings {
                parity: Parity::Even,
                max_concurrent_requests: 64,
            },
            peer_supports_retry: true,
            session_resume_key: None,
            peer_resume_key: None,
            our_schema: vec![],
            peer_schema: vec![],
        };
        let (_root_caller, session) =
            roam::initiator_conduit(BareConduit::new(client_link), handshake)
                .establish::<DriverCaller>(())
                .await
                .map_err(|e| eyre::eyre!("LocalCaller initiation failed: {:?}", e))?;

        // Open a virtual connection for DAW services
        let conn = session
            .open_connection(
                ConnectionSettings {
                    parity: Parity::Odd,
                    max_concurrent_requests: 64,
                },
                vec![MetadataEntry {
                    key: "role",
                    value: MetadataValue::String("local"),
                    flags: MetadataFlags::NONE,
                }],
            )
            .await
            .map_err(|e| eyre::eyre!("LocalCaller open_connection failed: {:?}", e))?;

        let mut driver = Driver::new(conn, ());
        let caller = ErasedCaller::new(driver.caller());
        moire::task::spawn(async move { driver.run().await });

        debug!("LocalCaller established (in-process memory channels, virtual connection)");

        Ok(Self {
            caller,
            _keep_alive: Arc::new(KeepAlive { _handle: handle }),
        })
    }

    /// Get the `ErasedCaller` for use with `Daw::new()` or `Daw::init()`.
    pub fn erased_caller(&self) -> ErasedCaller {
        self.caller.clone()
    }
}
