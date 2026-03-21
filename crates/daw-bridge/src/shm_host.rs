//! SHM-based host for hot-reloadable guest processes.
//!
//! Creates an SHM segment and a bootstrap Unix socket. External processes
//! (FTS, test runners, CLI tools) connect to the bootstrap socket, perform
//! an SHM handshake, and then communicate via lock-free ring buffers in
//! shared memory — no socket overhead for RPC traffic.
//!
//! The bootstrap flow follows roam's `shm_host_two_guests` pattern:
//!
//! 1. Host creates SHM `Segment` + `HostHub`, binds bootstrap socket
//! 2. Guest connects to bootstrap socket, sends session ID
//! 3. Host prepares a peer slot, sends back SHM path + fds
//! 4. Guest mmaps the segment, establishes roam session over `ShmLink`
//! 5. Guest calls DAW services via roam RPC over SHM

use std::path::PathBuf;
use std::sync::Arc;

use roam_shm::bootstrap::decode_request;
use roam_shm::varslot::SizeClassConfig;
use roam_shm::{HostHub, Segment, SegmentConfig};
use shm_primitives::FileCleanup;
use tokio::net::UnixListener;
use tracing::{debug, info, warn};

use crate::routed_handler::DawConnectionAcceptor;

/// Maximum number of simultaneous SHM guest connections.
const MAX_GUESTS: u8 = 8;

/// BipBuffer capacity per direction per guest (64 KiB).
const BIPBUF_CAPACITY: u32 = 64 * 1024;

/// Maximum payload size before external mmap fallback (1 MiB).
const MAX_PAYLOAD_SIZE: u32 = 1024 * 1024;

/// Inline threshold — payloads smaller than this go directly in the ring.
const INLINE_THRESHOLD: u32 = 256;

/// VarSlotPool size classes for medium-sized payloads.
const SIZE_CLASSES: &[SizeClassConfig] = &[
    SizeClassConfig {
        slot_size: 4096,
        slot_count: 16,
    },
    SizeClassConfig {
        slot_size: 16384,
        slot_count: 8,
    },
];

/// Build the bootstrap socket path for this REAPER instance.
///
/// Default: `/tmp/fts-daw-{pid}.bootstrap.sock`
/// Override with `FTS_SHM_BOOTSTRAP_SOCK` env var.
fn bootstrap_socket_path() -> PathBuf {
    std::env::var("FTS_SHM_BOOTSTRAP_SOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let pid = std::process::id();
            PathBuf::from(format!("/tmp/fts-daw-{pid}.bootstrap.sock"))
        })
}

/// Managed SHM host state — segment, hub, and temp directory.
struct ShmHostState {
    segment: Arc<Segment>,
    hub: HostHub,
    shm_path: PathBuf,
    _tempdir: tempfile::TempDir,
}

/// Start the SHM bootstrap listener.
///
/// Creates an SHM segment and listens on a bootstrap Unix socket.
/// Each connecting guest gets its own peer slot in the SHM segment
/// and a roam session using the provided handler.
///
/// Returns the bootstrap socket path on success (for passing to guest processes).
pub fn start_shm_host(handler: DawConnectionAcceptor) -> Option<String> {
    let tempdir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to create tempdir for SHM segment: {}", e);
            return None;
        }
    };

    let shm_path = tempdir.path().join("fts-daw.shm");

    let segment = match Segment::create(
        &shm_path,
        SegmentConfig {
            max_guests: MAX_GUESTS,
            bipbuf_capacity: BIPBUF_CAPACITY,
            max_payload_size: MAX_PAYLOAD_SIZE,
            inline_threshold: INLINE_THRESHOLD,
            heartbeat_interval: 0,
            size_classes: SIZE_CLASSES,
        },
        FileCleanup::Manual,
    ) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            warn!(
                "Failed to create SHM segment at {}: {}",
                shm_path.display(),
                e
            );
            return None;
        }
    };

    let hub = HostHub::new(Arc::clone(&segment));

    let state = Arc::new(ShmHostState {
        segment,
        hub,
        shm_path,
        _tempdir: tempdir,
    });

    let bootstrap_path = bootstrap_socket_path();

    // Remove stale socket from a previous run
    let _ = std::fs::remove_file(&bootstrap_path);

    let listener = match UnixListener::bind(&bootstrap_path) {
        Ok(l) => l,
        Err(e) => {
            warn!(
                "Failed to bind SHM bootstrap socket at {}: {}",
                bootstrap_path.display(),
                e
            );
            return None;
        }
    };

    info!(
        "SHM host ready: segment={}, bootstrap={}",
        state.shm_path.display(),
        bootstrap_path.display()
    );

    moire::task::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    info!("SHM bootstrap connection received");
                    let state = Arc::clone(&state);
                    let acceptor = handler.clone();
                    moire::task::spawn(async move {
                        if let Err(e) = handle_bootstrap_connection(stream, &state, acceptor).await
                        {
                            warn!("SHM bootstrap failed: {}", e);
                        }
                    });
                }
                Err(e) => {
                    warn!("SHM bootstrap accept error: {}", e);
                }
            }
        }
    });

    Some(bootstrap_path.to_string_lossy().into_owned())
}

/// Handle a single bootstrap connection from a guest process.
async fn handle_bootstrap_connection(
    stream: tokio::net::UnixStream,
    state: &ShmHostState,
    acceptor: DawConnectionAcceptor,
) -> eyre::Result<()> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    // Convert to std stream for blocking bootstrap I/O (short messages)
    let mut std_stream = stream.into_std()?;
    std_stream.set_nonblocking(false)?;

    let mut request_buf = [0u8; 2048];
    let n = std_stream.read(&mut request_buf)?;
    if n == 0 {
        eyre::bail!("bootstrap request EOF");
    }

    let request = decode_request(&request_buf[..n])
        .map_err(|e| eyre::eyre!("decode bootstrap request: {}", e))?;

    let sid = String::from_utf8_lossy(request.sid).to_string();
    debug!("SHM bootstrap request from session {}", sid);

    // Prepare a peer slot and bootstrap response
    let shm_path_str = state
        .shm_path
        .to_str()
        .ok_or_else(|| eyre::eyre!("SHM path is not valid UTF-8"))?;

    let prepared = state
        .hub
        .prepare_bootstrap_success(shm_path_str.as_bytes())
        .map_err(|e| eyre::eyre!("prepare bootstrap: {}", e))?;

    // Send bootstrap success with fds (Unix fd-passing)
    prepared
        .send_success_unix(std_stream.as_raw_fd(), &state.segment)
        .map_err(|e| eyre::eyre!("send bootstrap success: {}", e))?;

    info!(
        "SHM peer {} bootstrapped for session {}",
        prepared.peer_id().get(),
        sid
    );

    // Build the host-side ShmLink and establish a roam session
    let link = prepared
        .host_peer
        .into_link()
        .map_err(|e| eyre::eyre!("build host link: {}", e))?;

    let handshake = roam::HandshakeResult {
        role: roam::SessionRole::Acceptor,
        our_settings: roam::ConnectionSettings {
            parity: roam::Parity::Even,
            max_concurrent_requests: 64,
        },
        peer_settings: roam::ConnectionSettings {
            parity: roam::Parity::Odd,
            max_concurrent_requests: 64,
        },
        peer_supports_retry: true,
        session_resume_key: None,
        peer_resume_key: None,
        our_schema: vec![],
        peer_schema: vec![],
    };
    let (_caller, _session_handle) = roam::acceptor(link, handshake)
        .on_connection(acceptor)
        .establish::<roam::DriverCaller>(())
        .await
        .map_err(|e| eyre::eyre!("SHM roam handshake failed: {:?}", e))?;

    info!("SHM session established for session {}", sid);

    // Keep the session alive — dropping _caller would close it.
    std::future::pending::<()>().await;
    Ok(())
}
