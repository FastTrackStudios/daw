//! Integration test: action registration via SHM guest connection.
//!
//! Verifies that a guest process connecting over SHM (not the Unix socket)
//! can register actions, look them up, subscribe to trigger events, and
//! execute them. This is the path that real extensions like fts-sync use.
//!
//! Run with:
//!
//!   cargo xtask reaper-test shm_action_registry

use std::io::Write;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use daw_control::Daw;
use eyre::{Result, eyre};
use shm_primitives::PeerId;
use vox::{
    ConnectionSettings, Driver, ErasedCaller, MetadataEntry, MetadataFlags, MetadataValue, Parity,
};
use vox_shm::bootstrap::{BootstrapStatus, encode_request};
use vox_shm::{Segment, ShmLink};

use reaper_test::reaper_test;

// ---------------------------------------------------------------------------
// SHM bootstrap helpers (same as reaper_shm_hot_reload.rs)
// ---------------------------------------------------------------------------

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
        eyre::bail!("bootstrap rejected: {:?}", received.response.status);
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
        vox_shm::guest_link_from_raw(segment, peer_id, doorbell_fd, mmap_rx_fd, mmap_tx_fd, true)
    }
    .map_err(|e| eyre!("build guest link: {e}"))
}

async fn connect_guest(bootstrap_path: &Path) -> Result<Daw> {
    let link = connect_shm(bootstrap_path)?;

    let handshake = vox::HandshakeResult {
        role: vox::SessionRole::Initiator,
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
    let (_root_caller, session) = vox::initiator_conduit(link, handshake)
        .establish::<vox::DriverCaller>(())
        .await
        .map_err(|e| eyre!("vox handshake failed: {e:?}"))?;

    let conn = session
        .open_connection(
            ConnectionSettings {
                parity: Parity::Odd,
                max_concurrent_requests: 64,
            },
            vec![MetadataEntry {
                key: "role",
                value: MetadataValue::String("shm-action-test"),
                flags: MetadataFlags::NONE,
            }],
        )
        .await
        .map_err(|e| eyre!("open_connection failed: {e:?}"))?;

    let mut driver = Driver::new(conn, ());
    let caller = ErasedCaller::new(driver.caller());
    moire::task::spawn(async move { driver.run().await });

    Ok(Daw::new(caller))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Guest connects via SHM, registers an action, and verifies it exists.
#[reaper_test]
async fn shm_guest_register_action(_ctx: &ReaperTestContext) -> eyre::Result<()> {
    let bootstrap_path = discover_bootstrap_socket()?;
    println!("Bootstrap socket: {}", bootstrap_path.display());

    let daw = connect_guest(&bootstrap_path).await?;
    println!("SHM guest connected");

    let actions = daw.action_registry();

    // Register an action via the SHM connection
    let cmd_id = actions
        .register("FTS_TEST_SHM_REGISTER", "FTS Test: SHM Action Registration")
        .await?;
    println!("Registered action: cmd_id={cmd_id}");
    assert!(
        cmd_id > 0,
        "register via SHM should return a valid command ID"
    );

    // Verify it has a command ID
    let exists = actions.is_registered("FTS_TEST_SHM_REGISTER").await?;
    assert!(exists, "action registered via SHM should have a command ID");

    // Verify it's actually in REAPER's action list (gaccel registered)
    let in_list = actions.is_in_action_list("FTS_TEST_SHM_REGISTER").await?;
    assert!(
        in_list,
        "action registered via SHM should appear in REAPER's action list"
    );

    // Look up command ID
    let looked_up = actions.lookup_command_id("FTS_TEST_SHM_REGISTER").await?;
    assert_eq!(
        looked_up,
        Some(cmd_id),
        "lookup should return the same command ID"
    );

    println!("Action registration via SHM verified successfully");
    Ok(())
}

/// Guest registers an action via SHM, subscribes, executes it, and receives
/// the trigger event back through the SHM connection.
#[reaper_test]
async fn shm_guest_action_subscribe_and_trigger(_ctx: &ReaperTestContext) -> eyre::Result<()> {
    let bootstrap_path = discover_bootstrap_socket()?;
    let daw = connect_guest(&bootstrap_path).await?;

    let actions = daw.action_registry();

    // Register
    let cmd_id = actions
        .register("FTS_TEST_SHM_TRIGGER", "FTS Test: SHM Action Trigger")
        .await?;
    assert!(cmd_id > 0, "register should succeed");
    println!("Registered FTS_TEST_SHM_TRIGGER → cmd_id={cmd_id}");

    // Subscribe to action events
    let mut rx = actions.subscribe_actions().await?;
    println!("Subscribed to action events");

    // Execute the action — this should trigger the callback on the host,
    // which broadcasts to our subscriber
    let executed = actions.execute_named_action("FTS_TEST_SHM_TRIGGER").await?;
    assert!(executed, "execute_named_action should return true");
    println!("Executed FTS_TEST_SHM_TRIGGER");

    // Wait for the trigger event (with timeout)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| eyre!("timed out waiting for action trigger event — subscriber may not be receiving via SHM"))?
        .map_err(|e| eyre!("rx error: {e:?}"))?;

    match event {
        Some(item) => {
            let daw_proto::ActionEvent::Triggered { ref command_name } = *item;
            println!("Received trigger event: {command_name}");
            assert_eq!(
                command_name, "FTS_TEST_SHM_TRIGGER",
                "trigger event should carry the correct command name"
            );
        }
        None => {
            panic!("Action event stream closed unexpectedly");
        }
    }

    println!("Full action round-trip via SHM verified: register → subscribe → execute → receive");
    Ok(())
}

/// Guest registers an action via SHM, then the socket-based ctx.daw can also
/// see and execute it — proving the action lands in REAPER's global registry.
#[reaper_test(isolated)]
async fn shm_guest_action_visible_from_socket(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let bootstrap_path = discover_bootstrap_socket()?;
    let shm_daw = connect_guest(&bootstrap_path).await?;

    let shm_actions = shm_daw.action_registry();

    // Register via SHM
    let cmd_id = shm_actions
        .register(
            "FTS_TEST_SHM_CROSS_VISIBLE",
            "FTS Test: SHM Cross-Visibility",
        )
        .await?;
    assert!(cmd_id > 0);
    println!("Registered via SHM: cmd_id={cmd_id}");

    // Now check via the socket-based connection (ctx.daw)
    let socket_actions = ctx.daw.action_registry();
    let exists = socket_actions
        .is_registered("FTS_TEST_SHM_CROSS_VISIBLE")
        .await?;
    assert!(
        exists,
        "action registered via SHM should be visible from socket connection"
    );

    let looked_up = socket_actions
        .lookup_command_id("FTS_TEST_SHM_CROSS_VISIBLE")
        .await?;
    assert_eq!(
        looked_up,
        Some(cmd_id),
        "socket lookup should return the same command ID as SHM registration"
    );

    println!("Cross-connection visibility verified: SHM registration visible from socket");
    Ok(())
}
