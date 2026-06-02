//! In-process [`Daw`](daw_control::Daw) construction backed by
//! [`Standalone`].
//!
//! Spawns a [`vox::memory_link_pair`] and wires the server side to a
//! `LayerRouter` built from `Standalone`'s [`architect::Services`]
//! bundle. The client side becomes a `Caller` you can wrap in
//! `daw_control::Daw` and pass to any consumer that speaks the proto
//! traits — `SynchronizationEngine`, UI shells, tests, etc.
//!
//! ```ignore
//! use daw_standalone::sync::Standalone;
//! use daw_standalone::bootstrap::build_in_process_daw;
//!
//! let standalone = Standalone::new();
//! standalone.seed_project(ProjectInfo { ... });
//! let daw: daw_control::Daw = build_in_process_daw(standalone).await?;
//! daw.current_project().await?.transport().play().await?;
//! ```
//!
//! This mirrors `daw-reaper`'s [`LocalCaller`] but keeps Standalone
//! self-contained (no backwards dep on daw-reaper).

use std::borrow::Cow;
use std::sync::Arc;

use architect::Services;
use moire::task::JoinHandle;
use vox::{
    BareConduit, Caller, ConnectionSettings, Driver, DriverReplySink, Handler, MetadataEntry,
    MetadataFlags, MetadataValue, Parity,
};

use crate::sync::Standalone;

/// Keeps the server-side acceptor task alive for the lifetime of the
/// returned [`InProcessDaw`]. Drop it to tear down the in-proc link.
struct KeepAlive {
    _server_task: JoinHandle<()>,
    _driver_task: JoinHandle<()>,
}

/// Bundle of an in-process [`daw_control::Daw`] + the resources
/// keeping its server side alive. Clone the inner `Daw` freely; drop
/// the bundle to shut everything down.
#[derive(Clone)]
pub struct InProcessDaw {
    pub daw: daw_control::Daw,
    pub standalone: Standalone,
    _keep_alive: Arc<KeepAlive>,
}

impl InProcessDaw {
    pub fn caller(&self) -> &Caller {
        self.daw.caller()
    }
}

/// Build an in-process `Daw` client wired to `standalone`'s service
/// bundle (`architect::Services::into_router()`).
pub async fn build_in_process_daw(standalone: Standalone) -> eyre::Result<InProcessDaw> {
    let handler = standalone.clone().into_router();
    let caller = build_caller(handler).await?;
    Ok(InProcessDaw {
        daw: daw_control::Daw::new(caller.caller),
        standalone,
        _keep_alive: caller.keep_alive,
    })
}

struct BuiltCaller {
    caller: Caller,
    keep_alive: Arc<KeepAlive>,
}

async fn build_caller<H>(handler: H) -> eyre::Result<BuiltCaller>
where
    H: Handler<DriverReplySink> + Clone + 'static,
{
    let (client_link, server_link) = vox::memory_link_pair(256);

    // ── Server side ───────────────────────────────────────────────
    let server_task = moire::task::spawn(async move {
        let handshake = vox::HandshakeResult {
            role: vox::SessionRole::Acceptor,
            our_settings: ConnectionSettings {
                parity: Parity::Even,
                max_concurrent_requests: 64,
                initial_channel_credit: 16,
            },
            peer_settings: ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: 64,
                initial_channel_credit: 16,
            },
            peer_supports_retry: true,
            session_resume_key: None,
            peer_resume_key: None,
            our_schema: vec![],
            peer_schema: vec![],
            peer_metadata: vec![],
        };
        match vox::acceptor_conduit(BareConduit::new(server_link), handshake)
            .on_connection(handler)
            .establish::<vox::NoopClient>()
            .await
        {
            Ok(_root) => {
                tracing::debug!("standalone in-proc server established");
                std::future::pending::<()>().await;
            }
            Err(e) => {
                tracing::warn!("standalone in-proc accept failed: {e:?}");
            }
        }
    });

    // ── Client side ───────────────────────────────────────────────
    let handshake = vox::HandshakeResult {
        role: vox::SessionRole::Initiator,
        our_settings: ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        },
        peer_settings: ConnectionSettings {
            parity: Parity::Even,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        },
        peer_supports_retry: true,
        session_resume_key: None,
        peer_resume_key: None,
        our_schema: vec![],
        peer_schema: vec![],
        peer_metadata: vec![],
    };
    let root = vox::initiator_conduit(BareConduit::new(client_link), handshake)
        .establish::<vox::NoopClient>()
        .await
        .map_err(|e| eyre::eyre!("standalone in-proc initiation failed: {e:?}"))?;
    let session = root
        .session
        .clone()
        .ok_or_else(|| eyre::eyre!("standalone in-proc root session missing"))?;

    let conn = session
        .open_connection(
            ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: 64,
                initial_channel_credit: 16,
            },
            vec![
                MetadataEntry {
                    key: Cow::Borrowed("vox-service"),
                    value: MetadataValue::String(Cow::Borrowed("daw-local")),
                    flags: MetadataFlags::NONE,
                },
                MetadataEntry {
                    key: Cow::Borrowed("role"),
                    value: MetadataValue::String(Cow::Borrowed("local")),
                    flags: MetadataFlags::NONE,
                },
            ],
        )
        .await
        .map_err(|e| eyre::eyre!("standalone in-proc open_connection failed: {e:?}"))?;

    let mut driver = Driver::new(conn, ());
    let caller = Caller::new(driver.caller());
    let driver_task = moire::task::spawn(async move { driver.run().await });

    Ok(BuiltCaller {
        caller,
        keep_alive: Arc::new(KeepAlive {
            _server_task: server_task,
            _driver_task: driver_task,
        }),
    })
}
