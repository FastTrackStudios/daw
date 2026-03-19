//! SHM extension runtime for daw-bridge.
//!
//! Provides a one-call [`connect`] function that handles the full SHM bootstrap
//! handshake and returns a ready-to-use [`daw::Daw`] handle. This is the only
//! dependency a hot-reloadable extension process needs to talk to REAPER.
//!
//! # Example
//!
//! ```rust,ignore
//! use daw_extension_runtime::GuestOptions;
//!
//! #[tokio::main]
//! async fn main() -> eyre::Result<()> {
//!     let daw = daw_extension_runtime::connect(GuestOptions {
//!         role: "my-extension",
//!         ..Default::default()
//!     }).await?;
//!
//!     let project = daw.current_project().await.unwrap();
//!     let state = project.transport().get_state().await.unwrap();
//!     println!("Tempo: {:.1} BPM", state.tempo);
//!     Ok(())
//! }
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

/// Options for connecting to daw-bridge as an SHM guest.
pub struct GuestOptions<'a> {
    /// Role name sent as connection metadata (e.g. "signal", "session", "fts-macros").
    pub role: &'a str,

    /// Explicit bootstrap socket path. If `None`, auto-discovers from
    /// `FTS_SHM_BOOTSTRAP_SOCK` env var or scans `/tmp/fts-daw-*.bootstrap.sock`.
    pub bootstrap_socket: Option<PathBuf>,

    /// Maximum number of concurrent in-flight RPC requests.
    pub max_concurrent_requests: u32,
}

impl Default for GuestOptions<'_> {
    fn default() -> Self {
        Self {
            role: "guest",
            bootstrap_socket: None,
            max_concurrent_requests: 64,
        }
    }
}

/// Connect to daw-bridge via SHM and return a ready-to-use [`daw::Daw`] handle.
///
/// This performs the full bootstrap sequence:
/// 1. Discover the SHM bootstrap socket
/// 2. Perform SHM handshake (send session ID, receive segment path + fds)
/// 3. Establish a roam session over the ShmLink
/// 4. Open a virtual connection with role metadata
/// 5. Return a [`daw::Daw`] handle wired to the connection
pub async fn connect(opts: GuestOptions<'_>) -> Result<daw::Daw> {
    let pid = std::process::id();

    // Step 1: Discover bootstrap socket
    let bootstrap_sock = match opts.bootstrap_socket {
        Some(path) => path,
        None => discover_bootstrap_socket()?,
    };
    info!(
        "[guest:{pid}] Connecting via bootstrap socket: {}",
        bootstrap_sock.display()
    );

    // Step 2: SHM handshake
    let link = connect_shm(&bootstrap_sock)?;
    info!("[guest:{pid}] SHM link established");

    // Step 3: Establish roam session
    let (_root_caller, session) = roam::initiator_conduit(link)
        .establish::<roam::DriverCaller>(())
        .await
        .map_err(|e| eyre!("roam handshake failed: {e:?}"))?;
    info!("[guest:{pid}] Roam session established");

    // Step 4: Open virtual connection
    // Leak the role string — guest connections live for the process lifetime.
    let role: &'static str = Box::leak(opts.role.to_string().into_boxed_str());
    let conn = session
        .open_connection(
            ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: opts.max_concurrent_requests,
            },
            vec![MetadataEntry {
                key: "role",
                value: MetadataValue::String(role),
                flags: MetadataFlags::NONE,
            }],
        )
        .await
        .map_err(|e| eyre!("open_connection failed: {e:?}"))?;

    let mut driver = Driver::new(conn, ());
    let caller = ErasedCaller::new(driver.caller());
    moire::task::spawn(async move { driver.run().await });

    // Step 5: Return Daw handle
    let daw = daw::Daw::new(caller);
    info!("[guest:{pid}] Connected as role={role}");
    Ok(daw)
}

/// Discover the SHM bootstrap socket.
///
/// Priority:
/// 1. `FTS_SHM_BOOTSTRAP_SOCK` env var
/// 2. Scan `/tmp/fts-daw-*.bootstrap.sock` and pick the newest
pub fn discover_bootstrap_socket() -> Result<PathBuf> {
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
            "No SHM bootstrap socket found. Is REAPER running with the daw-bridge?\n\
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
