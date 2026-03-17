//! In-process roam caller using memory channels.
//!
//! Wraps `roam::memory_link_pair` + acceptor/initiator into a reusable struct
//! that any in-process consumer (plugins, extensions, desktop apps) can use
//! to get an `ErasedCaller` without duplicating the boilerplate.

use roam::{BareConduit, DriverCaller, DriverReplySink, ErasedCaller, Handler};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

/// Keeps the server-side acceptor task alive.
struct KeepAlive {
    _handle: JoinHandle<()>,
}

/// In-process roam caller backed by memory channels.
///
/// Creates a `memory_link_pair`, spawns an acceptor task for the server side,
/// and establishes an initiator on the client side. The resulting `ErasedCaller`
/// routes RPC calls directly to the handler without any network overhead.
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
    /// Spawns a background task that accepts and dispatches requests via
    /// an in-memory link pair. The task lives as long as any `LocalCaller`
    /// clone exists.
    pub async fn new(handler: impl Handler<DriverReplySink>) -> eyre::Result<Self> {
        let (client_link, server_link) = roam::memory_link_pair(256);

        // Server side: accept and dispatch
        let handle = tokio::spawn(async move {
            match roam::acceptor(BareConduit::new(server_link))
                .establish::<DriverCaller>(handler)
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

        // Client side: get the ErasedCaller
        let (caller, _session) = roam::initiator(BareConduit::new(client_link))
            .establish::<DriverCaller>(())
            .await
            .map_err(|e| eyre::eyre!("LocalCaller initiation failed: {:?}", e))?;

        debug!("LocalCaller established (in-process memory channels)");

        Ok(Self {
            caller: ErasedCaller::new(caller),
            _keep_alive: Arc::new(KeepAlive { _handle: handle }),
        })
    }

    /// Get the `ErasedCaller` for use with `Daw::new()` or `Daw::init()`.
    pub fn erased_caller(&self) -> ErasedCaller {
        self.caller.clone()
    }
}
