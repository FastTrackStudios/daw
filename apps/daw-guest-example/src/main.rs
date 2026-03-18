//! Example DAW guest process — connects to REAPER via SHM bootstrap.
//!
//! This demonstrates the hot-reloadable guest pattern:
//! 1. Discover the SHM bootstrap socket (written by the REAPER extension)
//! 2. Perform the SHM handshake (send session ID, receive segment path + fds)
//! 3. Establish a roam session over the ShmLink
//! 4. Open a virtual connection for DAW services
//! 5. Call DAW services (transport, project, tracks, FX) via zero-copy SHM RPC
//!
//! You can rebuild and restart this binary without restarting REAPER.
//!
//! # Usage
//!
//! ```sh
//! # Start REAPER with the daw-test-extension loaded, then:
//! cargo run -p daw-guest-example
//!
//! # Or specify a custom bootstrap socket:
//! FTS_SHM_BOOTSTRAP_SOCK=/tmp/fts-daw-12345.bootstrap.sock cargo run -p daw-guest-example
//! ```

use std::io::Write;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::{Result, WrapErr, eyre};
use roam::{
    ConnectionSettings, Driver, ErasedCaller, MetadataEntry, MetadataFlags, MetadataValue, Parity,
};
use roam_shm::bootstrap::{BootstrapStatus, encode_request};
use roam_shm::{Segment, ShmLink};
use shm_primitives::PeerId;
use tracing::info;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .wrap_err("building tokio runtime")?;

    rt.block_on(run())
}

async fn run() -> Result<()> {
    let pid = std::process::id();
    info!("[guest:{pid}] DAW guest example starting");

    // Step 1: Discover bootstrap socket
    let bootstrap_sock = discover_bootstrap_socket()?;
    info!(
        "[guest:{pid}] Found bootstrap socket: {}",
        bootstrap_sock.display()
    );

    // Step 2: Connect and perform SHM handshake
    let link = connect_shm(&bootstrap_sock)?;
    info!("[guest:{pid}] SHM link established");

    // Step 3: Establish root roam session over the ShmLink
    let (_root_caller, session) = roam::initiator_conduit(link)
        .establish::<roam::DriverCaller>(())
        .await
        .map_err(|e| eyre!("roam handshake failed: {e:?}"))?;
    info!("[guest:{pid}] Roam session established over SHM");

    // Step 4: Open a virtual connection for DAW services
    let conn = session
        .open_connection(
            ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: 64,
            },
            vec![MetadataEntry {
                key: "role",
                value: MetadataValue::String("guest-example"),
                flags: MetadataFlags::NONE,
            }],
        )
        .await
        .map_err(|e| eyre!("open_connection failed: {e:?}"))?;

    let mut driver = Driver::new(conn, ());
    let caller = ErasedCaller::new(driver.caller());
    tokio::spawn(async move { driver.run().await });

    // Step 5: Create a high-level Daw handle from the caller
    let daw = daw::Daw::new(caller);

    // ── Transport ────────────────────────────────────────────────────────
    let project = daw.current_project().await.map_err(|e| eyre!("{e}"))?;
    let transport = project.transport();

    info!("[guest:{pid}] Querying transport state...");
    let state = transport.get_state().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Transport: tempo={:.1} BPM, playing={}, looping={}",
        state.tempo,
        state.play_state == daw::service::PlayState::Playing,
        state.looping,
    );

    // ── Project ──────────────────────────────────────────────────────────
    info!("[guest:{pid}] Creating new project tab...");
    let test_project = daw.create_project().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Created project: guid={}",
        test_project.guid()
    );

    // ── Tracks ───────────────────────────────────────────────────────────
    info!("[guest:{pid}] Adding tracks...");
    let tracks = test_project.tracks();
    let track_a = tracks
        .add("SHM-Track-A", None)
        .await
        .map_err(|e| eyre!("{e}"))?;
    let track_b = tracks
        .add("SHM-Track-B", None)
        .await
        .map_err(|e| eyre!("{e}"))?;
    let track_a_info = track_a.info().await.map_err(|e| eyre!("{e}"))?;
    let track_b_info = track_b.info().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Added tracks: A={}, B={}",
        track_a_info.name, track_b_info.name
    );

    let all_tracks = tracks.all().await.map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] Project has {} tracks", all_tracks.len());

    // ── FX ───────────────────────────────────────────────────────────────
    info!("[guest:{pid}] Adding ReaEQ to Track A...");
    let fx = track_a
        .fx_chain()
        .add("ReaEQ")
        .await
        .map_err(|e| eyre!("{e}"))?;
    let fx_info = fx.info().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Added FX: {} (guid={})",
        fx_info.name,
        fx.guid()
    );

    let params = fx.parameters().await.map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] FX has {} parameters", params.len());
    if let Some(first) = params.first() {
        info!(
            "[guest:{pid}] First param: '{}' = {:.4}",
            first.name, first.value
        );
    }

    // ── Cleanup ──────────────────────────────────────────────────────────
    info!("[guest:{pid}] Cleaning up: closing test project tab...");
    let _ = tracks.remove_all().await;
    let _ = daw.close_project(test_project.guid().to_string()).await;

    info!(
        "[guest:{pid}] Done. All DAW services available over SHM virtual connection. \
         Guest can be rebuilt and restarted without touching REAPER."
    );
    Ok(())
}

/// Discover the SHM bootstrap socket.
///
/// Priority:
/// 1. `FTS_SHM_BOOTSTRAP_SOCK` env var
/// 2. Scan `/tmp/fts-daw-*.bootstrap.sock` and pick the newest
fn discover_bootstrap_socket() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("FTS_SHM_BOOTSTRAP_SOCK") {
        return Ok(path.into());
    }

    let mut matches = Vec::new();
    for entry in std::fs::read_dir("/tmp").wrap_err("reading /tmp")? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("fts-daw-") && name_str.ends_with(".bootstrap.sock") {
            matches.push(entry.path());
        }
    }

    if matches.is_empty() {
        eyre::bail!(
            "No SHM bootstrap socket found. Is REAPER running with the daw-test-extension?\n\
             Set FTS_SHM_BOOTSTRAP_SOCK to specify the path manually."
        );
    }

    // Sort by modification time (newest first)
    matches.sort_by(|a, b| {
        let time_a = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        let time_b = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        time_b.cmp(&time_a)
    });

    Ok(matches.remove(0))
}

/// Connect to the SHM bootstrap socket and establish an ShmLink.
fn connect_shm(bootstrap_path: &Path) -> Result<ShmLink> {
    let sid = generate_session_id();

    let request =
        encode_request(sid.as_bytes()).map_err(|e| eyre!("encode bootstrap request: {e}"))?;

    let mut stream = UnixStream::connect(bootstrap_path).map_err(|e| {
        eyre!(
            "connect to bootstrap socket {}: {e}",
            bootstrap_path.display()
        )
    })?;
    stream
        .write_all(&request)
        .wrap_err("send bootstrap request")?;

    let received = shm_primitives::bootstrap::recv_response_unix(stream.as_raw_fd())
        .map_err(|e| eyre!("receive bootstrap response: {e}"))?;

    if received.response.status != BootstrapStatus::Success {
        eyre::bail!(
            "bootstrap rejected: status={:?}, payload={}",
            received.response.status,
            String::from_utf8_lossy(&received.response.payload)
        );
    }

    let fds = received
        .fds
        .ok_or_else(|| eyre!("bootstrap success but no fds received"))?;

    let shm_path = std::str::from_utf8(&received.response.payload)
        .map_err(|e| eyre!("bootstrap payload not utf-8: {e}"))?;
    info!("Attaching to SHM segment at {}", shm_path);

    let segment = Arc::new(
        Segment::attach(Path::new(shm_path))
            .map_err(|e| eyre!("attach SHM segment at {shm_path}: {e}"))?,
    );

    let peer_id = PeerId::new(received.response.peer_id as u8)
        .ok_or_else(|| eyre!("invalid peer id {}", received.response.peer_id))?;

    let doorbell_fd = fds.doorbell_fd.into_raw_fd();
    let mmap_rx_fd = fds.mmap_control_fd.into_raw_fd();
    let mmap_tx_fd = unsafe { libc::dup(mmap_rx_fd) };
    if mmap_tx_fd < 0 {
        eyre::bail!(
            "failed to dup mmap fd for tx: {}",
            std::io::Error::last_os_error()
        );
    }

    unsafe {
        roam_shm::guest_link_from_raw(segment, peer_id, doorbell_fd, mmap_rx_fd, mmap_tx_fd, true)
    }
    .map_err(|e| eyre!("build guest link: {e}"))
}

/// Generate a unique session ID for this guest connection.
fn generate_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let pid = u128::from(std::process::id());
    format!("{:032x}", nanos ^ (pid << 64))
}
