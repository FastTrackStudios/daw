//! Integration test demonstrating SHM guest hot-reload.
//!
//! Simulates the hot-reload workflow: a guest process connects to REAPER via
//! the SHM bootstrap socket, exercises DAW services, disconnects (simulating
//! a rebuild/restart), and reconnects — multiple times. REAPER stays running
//! throughout, and each "guest generation" gets a fresh virtual connection.
//!
//! Run with:
//!
//!   cargo xtask reaper-test shm_guest_hot_reload

use std::io::Write;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use daw_control::Daw;
use eyre::{Result, eyre};
use roam::{
    ConnectionSettings, Driver, ErasedCaller, MetadataEntry, MetadataFlags, MetadataValue, Parity,
};
use roam_shm::bootstrap::{BootstrapStatus, encode_request};
use roam_shm::{Segment, ShmLink};
use shm_primitives::PeerId;

use reaper_test::reaper_test;

/// Discover the SHM bootstrap socket (same logic as the guest example).
fn discover_bootstrap_socket() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("FTS_SHM_BOOTSTRAP_SOCK") {
        return Ok(path.into());
    }

    let mut matches = Vec::new();
    for entry in std::fs::read_dir("/tmp")? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("fts-daw-") && name_str.ends_with(".bootstrap.sock") {
            matches.push(entry.path());
        }
    }

    if matches.is_empty() {
        eyre::bail!("No SHM bootstrap socket found");
    }

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

/// Connect to the SHM bootstrap socket and get an ShmLink.
fn connect_shm(bootstrap_path: &Path) -> Result<ShmLink> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let pid = u128::from(std::process::id());
    let sid = format!("{:032x}", nanos ^ (pid << 64));

    let request =
        encode_request(sid.as_bytes()).map_err(|e| eyre!("encode bootstrap request: {e}"))?;

    let mut stream = UnixStream::connect(bootstrap_path)
        .map_err(|e| eyre!("connect to bootstrap socket: {e}"))?;
    stream
        .write_all(&request)
        .map_err(|e| eyre!("send bootstrap request: {e}"))?;

    let received = shm_primitives::bootstrap::recv_response_unix(stream.as_raw_fd())
        .map_err(|e| eyre!("receive bootstrap response: {e}"))?;

    if received.response.status != BootstrapStatus::Success {
        eyre::bail!("bootstrap rejected: {:?}", received.response.status,);
    }

    let fds = received
        .fds
        .ok_or_else(|| eyre!("no fds in bootstrap response"))?;

    let shm_path = std::str::from_utf8(&received.response.payload)
        .map_err(|e| eyre!("bootstrap payload not utf-8: {e}"))?;

    let segment = Arc::new(
        Segment::attach(Path::new(shm_path)).map_err(|e| eyre!("attach SHM segment: {e}"))?,
    );

    let peer_id =
        PeerId::new(received.response.peer_id as u8).ok_or_else(|| eyre!("invalid peer id"))?;

    let doorbell_fd = fds.doorbell_fd.into_raw_fd();
    let mmap_rx_fd = fds.mmap_control_fd.into_raw_fd();
    let mmap_tx_fd = unsafe { libc::dup(mmap_rx_fd) };
    if mmap_tx_fd < 0 {
        eyre::bail!("failed to dup mmap fd: {}", std::io::Error::last_os_error());
    }

    unsafe {
        roam_shm::guest_link_from_raw(segment, peer_id, doorbell_fd, mmap_rx_fd, mmap_tx_fd, true)
    }
    .map_err(|e| eyre!("build guest link: {e}"))
}

/// Connect via SHM, open a virtual connection, and return a Daw handle.
///
/// Each call simulates a fresh guest process connecting to the running host.
async fn connect_guest(bootstrap_path: &Path, cycle: u32) -> Result<Daw> {
    println!("[cycle {cycle}] Connecting to SHM bootstrap...");
    let link = connect_shm(bootstrap_path)?;

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
    let (_root_caller, session) = roam::initiator_conduit(link, handshake)
        .establish::<roam::DriverCaller>(())
        .await
        .map_err(|e| eyre!("roam handshake failed: {e:?}"))?;

    let conn = session
        .open_connection(
            ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: 64,
            },
            vec![MetadataEntry {
                key: "role",
                value: MetadataValue::String("hot-reload-test"),
                flags: MetadataFlags::NONE,
            }],
        )
        .await
        .map_err(|e| eyre!("open_connection failed: {e:?}"))?;

    let mut driver = Driver::new(conn, ());
    let caller = ErasedCaller::new(driver.caller());
    moire::task::spawn(async move { driver.run().await });

    println!("[cycle {cycle}] SHM virtual connection established");
    Ok(Daw::new(caller))
}

// ---------------------------------------------------------------------------
// Hot reload test: connect, use services, disconnect, repeat 3 times
// ---------------------------------------------------------------------------

#[reaper_test]
async fn shm_guest_hot_reload(_ctx: &ReaperTestContext) -> eyre::Result<()> {
    let bootstrap_path = discover_bootstrap_socket()?;
    println!("Bootstrap socket: {}", bootstrap_path.display());

    for cycle in 1..=3u32 {
        println!("\n=== Guest cycle {cycle} ===");

        // Connect as a fresh guest via SHM
        let daw = connect_guest(&bootstrap_path, cycle).await?;

        // Create a project tab
        let project = daw.create_project().await.map_err(|e| eyre!("{e}"))?;
        println!("[cycle {cycle}] Created project: {}", project.guid());

        // Add a track with a cycle-specific name
        let track_name = format!("HotReload-Cycle{cycle}");
        let track = project
            .tracks()
            .add(&track_name, None)
            .await
            .map_err(|e| eyre!("{e}"))?;
        let track_info = track.info().await.map_err(|e| eyre!("{e}"))?;
        assert_eq!(track_info.name, track_name);
        println!("[cycle {cycle}] Added track: {}", track_info.name);

        // Add FX and read parameters
        let fx = track
            .fx_chain()
            .add("ReaEQ")
            .await
            .map_err(|e| eyre!("{e}"))?;
        let params = fx.parameters().await.map_err(|e| eyre!("{e}"))?;
        println!("[cycle {cycle}] ReaEQ has {} parameters", params.len());
        assert!(!params.is_empty(), "ReaEQ should have parameters");

        // Query transport (proves cross-service access works)
        let current_proj = daw.current_project().await.map_err(|e| eyre!("{e}"))?;
        let state = current_proj
            .transport()
            .get_state()
            .await
            .map_err(|e| eyre!("{e}"))?;
        println!("[cycle {cycle}] Transport: tempo={:.1} BPM", state.tempo);

        // Clean up: remove tracks, close project
        let _ = project.tracks().remove_all().await;
        let _ = daw.close_project(project.guid().to_string()).await;
        println!("[cycle {cycle}] Cleaned up project");

        // Drop the Daw handle — simulates guest process exit.
        // The host's session and SHM peer slot are released.
        drop(daw);
        println!("[cycle {cycle}] Disconnected (simulating process restart)");

        // Brief pause to let the host clean up the peer
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    println!("\n=== All 3 guest cycles connected and worked over SHM ===");
    println!("Hot reload validated: guests can connect, use services, disconnect, and reconnect");
    Ok(())
}
