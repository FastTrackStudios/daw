//! Transport — *where* the app connects. Chosen once at the app root.
//!
//! The app establishes **one** connection and hands its [`vox::Caller`]
//! around: every typed client is a cheap view over the same socket
//! (`ExampleRepoClient::new(caller.clone())`, …) and the server's
//! `LayerRouter` dispatches all services on that one connection. Features
//! pull the shared `Connection<Caller>` from context and build their
//! clients on demand — adding a feature adds **zero** connections.
//!
//! - [`Transport::Remote`] — talk to `app-server` over a vox WebSocket.
//! - [`Transport::Local`] — serve a backend **in-process** over a vox
//!   in-memory link (no server, no socket). Native only (the browser stays
//!   remote — vox's in-memory link isn't compiled for wasm).

use architect::{Duration, Schedule};
use example::ExampleRepoClient;
use vox_core::{Caller, initiator_on};
use vox_websocket::WsLink;

/// Default vox endpoint for the remote transport.
pub const DEFAULT_SERVER_URL: &str = "ws://127.0.0.1:4040/vox";

/// Where the services live. Chosen at the app root and provided via
/// context; `App` reads it to establish the connection.
#[derive(Clone)]
pub enum Transport {
    /// Remote `app-server` over a vox WebSocket.
    Remote(String),
    /// A backend served in-process — no server. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    Local(architect::LocalServer),
}

/// Establish the app's single connection and return its [`Caller`] — the
/// handle every typed client is built from.
pub async fn connect(transport: &Transport) -> Result<Caller, String> {
    // vox's `establish` yields a typed client; the caller inside it is the
    // session handle all other clients share. Which client type we anchor
    // with is arbitrary — the repo client is always present.
    let anchor: ExampleRepoClient = match transport {
        Transport::Remote(url) => establish_ws(url).await?,
        #[cfg(not(target_arch = "wasm32"))]
        Transport::Local(server) => server.establish().await.map_err(|e| format!("{e:?}"))?,
    };
    Ok(anchor.caller)
}

/// Establish one typed client over a fresh WebSocket, **resiliently**.
///
/// A just-booting or briefly-flaky server shouldn't fail the first frame,
/// so the connect+handshake is wrapped in [`architect::schedule::retry`]
/// under an exponential-backoff-with-jitter policy: ~200ms, 400ms, 800ms,
/// … (±20% jitter, capped at 5s), up to 5 retries. The policy's clock is
/// platform-portable — `tokio::time` on desktop, browser timers on web.
async fn establish_ws<C>(url: &str) -> Result<C, String>
where
    C: vox_core::FromVoxLane,
{
    architect::schedule::retry(
        || establish_ws_once::<C>(url),
        Schedule::exponential(Duration::from_millis(200))
            .max_delay(Duration::from_secs(5))
            .jittered()
            .take(5),
    )
    .await
}

/// One connect attempt.
async fn establish_ws_once<C>(url: &str) -> Result<C, String>
where
    C: vox_core::FromVoxLane,
{
    let link = WsLink::connect(url)
        .await
        .map_err(|e| format!("connect {url}: {e:?}"))?;
    initiator_on(link)
        .establish::<C>()
        .await
        .map_err(|e| format!("vox handshake: {e:?}"))
}
