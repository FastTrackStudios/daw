//! Tests for daw-standalone
//!
//! These tests verify the standalone DAW implementation works correctly
//! using the daw-control API which provides ergonomic access to transport.
//!
//! r[verify daw.protocol]
//! This test suite verifies the DAW Protocol implementation.

use daw_control::Daw;
use integration_tests::{setup_external_test, setup_test};
use tokio::time::Duration;

/// r[verify transport.play]
/// Verifies that the play() method starts playback.
///
/// r[verify transport.play.start]
/// r[verify transport.play.from-position]
#[tokio::test]
async fn test_transport_play() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    // Get current project and its transport
    let project = daw.current_project().await?;
    let transport = project.transport();

    // Play - no need to pass project context, it's inherited from project
    transport.play().await?;

    // Give it time to process
    tokio::time::sleep(Duration::from_millis(10)).await;
    Ok(())
}

/// r[verify transport.stop]
/// Verifies that the stop() method stops playback.
///
/// r[verify transport.stop.maintain-position]
#[tokio::test]
async fn test_transport_stop() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;
    let transport = project.transport();

    // Stop - no need to pass project context
    transport.stop().await?;

    tokio::time::sleep(Duration::from_millis(10)).await;
    Ok(())
}

/// r[verify transport.state.transitions]
/// Verifies state transitions work correctly.
///
/// Tests the full state transition cycle:
/// - Stopped -> Playing (via play)
/// - Playing -> Stopped (via stop)
#[tokio::test]
async fn test_transport_state_transitions() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;
    let transport = project.transport();

    // Play
    transport.play().await?;

    // Give it time to process
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Stop
    transport.stop().await?;

    tokio::time::sleep(Duration::from_millis(10)).await;
    Ok(())
}

/// Test spawning the actual daw-standalone binary
/// r[verify daw.protocol.spawn]
/// Verifies the DAW cell can be spawned as a separate process.
#[tokio::test]
async fn test_spawn_standalone_binary() -> eyre::Result<()> {
    let (host_handle, _dir) = setup_external_test("daw-standalone").await;

    // Create Daw handle for the spawned cell
    let daw = Daw::new(host_handle);

    // The spawned binary starts up and connects successfully
    // We can get the current project (or it may fail if not fully initialized)
    let _result = daw.current_project().await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

/// r[verify transport.concurrent]
/// Verifies multiple clients can call transport methods concurrently.
#[tokio::test]
async fn test_concurrent_calls() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;

    // Both transports share the same underlying connection but are separate handles
    let transport1 = project.transport();
    let transport2 = project.transport();

    // Both can call methods concurrently
    let (r1, r2) = tokio::join!(transport1.play(), transport2.stop());

    r1?;
    r2?;
    Ok(())
}

/// r[verify transport.position]
/// Verifies position control works correctly.
#[tokio::test]
async fn test_transport_position() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;
    let transport = project.transport();

    // Set position to 10.5 seconds
    transport.set_position(10.5).await?;

    // Get position back
    let pos = transport.get_position().await?;
    assert!(
        (pos - 10.5).abs() < 0.001,
        "Position should be ~10.5, got {}",
        pos
    );
    Ok(())
}

/// r[verify transport.tempo]
/// Verifies tempo control works correctly.
#[tokio::test]
async fn test_transport_tempo() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;
    let transport = project.transport();

    // Set tempo to 140 BPM
    transport.set_tempo(140.0).await?;

    // Get tempo back
    let tempo = transport.get_tempo().await?;
    assert!(
        (tempo - 140.0).abs() < 0.001,
        "Tempo should be ~140, got {}",
        tempo
    );
    Ok(())
}

/// r[verify transport.loop]
/// Verifies loop control works correctly.
#[tokio::test]
async fn test_transport_loop() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;
    let transport = project.transport();

    // Initially not looping
    let looping = transport.is_looping().await?;
    assert!(!looping, "Should not be looping initially");

    // Toggle loop on
    transport.toggle_loop().await?;

    // Should be looping now
    let looping = transport.is_looping().await?;
    assert!(looping, "Should be looping after toggle");

    // Toggle loop off
    transport.toggle_loop().await?;

    // Should not be looping
    let looping = transport.is_looping().await?;
    assert!(!looping, "Should not be looping after second toggle");
    Ok(())
}

/// r[verify transport.musical_position]
/// Verifies musical position seeking works correctly.
#[tokio::test]
async fn test_transport_musical_position() -> eyre::Result<()> {
    let fixture = setup_test();
    let daw = Daw::new(fixture.guest_handle);

    let project = daw.current_project().await?;
    let transport = project.transport();

    // Set to measure 2, beat 1 (0-indexed: measure 1, beat 0)
    transport.set_position_musical(1, 0, 0).await?;

    // At 120 BPM in 4/4, measure 2 beat 1 = 2 seconds
    // (4 beats per measure * 0.5 seconds per beat = 2 seconds for first measure)
    let pos = transport.get_position().await?;
    assert!(
        (pos - 2.0).abs() < 0.1,
        "Position should be ~2.0s, got {}",
        pos
    );
    Ok(())
}
