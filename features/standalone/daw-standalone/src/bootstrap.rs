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

use std::sync::Arc;

use architect::Services;
use moire::task::JoinHandle;
use vox::{Caller, ConnectionHandle, DriverReplySink, Handler};

use crate::sync::Standalone;

/// Keeps the server-side acceptor task + client connection alive for the
/// lifetime of the returned [`InProcessDaw`]. Drop it to tear down the
/// in-proc link.
struct KeepAlive {
    _server_task: JoinHandle<()>,
    _connection: ConnectionHandle,
}

/// Minimal `FromVoxLane` client capturing the lane's `Caller`
/// (vox 0.10 replacement for the removed `NoopClient`).
#[derive(Clone)]
struct LocalLaneClient {
    caller: Caller,
}

impl vox::FromVoxLane for LocalLaneClient {
    const SERVICE_NAME: &'static str = "daw-local";

    fn from_vox_lane(caller: Caller, _connection: Option<ConnectionHandle>) -> Self {
        Self { caller }
    }
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
    // vox 0.10 lane model: accept any lane and hand it to the supplied
    // handler (which dispatches by method id).
    let server_task = moire::task::spawn(async move {
        let acceptor = vox::lane_acceptor_fn(move |_req, connection| {
            connection.handle_with(handler.clone());
            Ok(())
        });
        match vox::acceptor_on(server_link)
            .on_lane(acceptor)
            .establish_connection()
            .await
        {
            Ok(_connection) => {
                tracing::debug!("standalone in-proc server established");
                std::future::pending::<()>().await;
            }
            Err(e) => {
                tracing::warn!("standalone in-proc accept failed: {e:?}");
            }
        }
    });

    // ── Client side ───────────────────────────────────────────────
    let connection = vox::initiator_on(client_link)
        .establish_connection()
        .await
        .map_err(|e| eyre::eyre!("standalone in-proc initiation failed: {e:?}"))?;
    let client = connection
        .open_lane::<LocalLaneClient>()
        .await
        .map_err(|e| eyre::eyre!("standalone in-proc open_lane failed: {e:?}"))?;
    let caller = client.caller;

    Ok(BuiltCaller {
        caller,
        keep_alive: Arc::new(KeepAlive {
            _server_task: server_task,
            _connection: connection,
        }),
    })
}
