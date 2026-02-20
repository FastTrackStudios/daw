//! Multi-DAW integration tests.
//!
//! These tests validate the PID-based socket discovery and ExtState-based
//! role classification that fts-control uses to connect to multiple REAPER
//! instances simultaneously.
//!
//! Uses `run_multi_reaper_test` which spawns N REAPER processes, connects
//! to each, and passes a `MultiDawTestContext` to the test body. All
//! processes are killed on exit.
//!
//! Run with:
//!
//!   cargo test -p daw-reaper --test reaper_multi_daw -- --ignored --nocapture

use eyre::Result;
use reaper_test::{DawInstanceConfig, run_multi_reaper_test};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Two-instance: session + signal classification via ExtState
// ---------------------------------------------------------------------------

/// Spawn two REAPER instances — one session, one signal — connect to both,
/// and verify they are independently reachable with correct ExtState roles.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn two_daw_session_and_signal() -> Result<()> {
    run_multi_reaper_test(
        "two_daw_session_and_signal",
        vec![
            DawInstanceConfig::new("session"),
            DawInstanceConfig::new("signal").with_env("FTS_DAW_ROLE", "signal"),
        ],
        |ctx| {
            Box::pin(async move {
                let session = ctx.by_label("session");
                let signal = ctx.by_label("signal");

                // ── Verify distinct PIDs and sockets ────────────────────
                assert_ne!(
                    session.pid, signal.pid,
                    "Two instances should have different PIDs"
                );
                assert_ne!(
                    session.socket_path, signal.socket_path,
                    "Two instances should have different socket paths"
                );
                println!(
                    "  Session: PID {}, socket {}",
                    session.pid,
                    session.socket_path.display()
                );
                println!(
                    "  Signal:  PID {}, socket {}",
                    signal.pid,
                    signal.socket_path.display()
                );

                // ── Verify ExtState-based role classification ───────────
                let session_role = session.daw.ext_state().get("FTS", "role").await?;
                let signal_role = signal.daw.ext_state().get("FTS", "role").await?;

                println!("  Session FTS/role = {:?}", session_role);
                println!("  Signal  FTS/role = {:?}", signal_role);

                assert!(
                    session_role.is_none(),
                    "Session DAW should not have FTS/role set"
                );
                assert_eq!(
                    signal_role.as_deref(),
                    Some("signal"),
                    "Signal DAW should have FTS/role = 'signal'"
                );

                // ── Verify both respond to project queries independently ─
                let session_projects = session.daw.projects().await?;
                let signal_projects = signal.daw.projects().await?;

                println!(
                    "  Session: {} project(s), Signal: {} project(s)",
                    session_projects.len(),
                    signal_projects.len()
                );

                assert!(
                    !session_projects.is_empty(),
                    "Session DAW should have at least one project"
                );
                assert!(
                    !signal_projects.is_empty(),
                    "Signal DAW should have at least one project"
                );

                // ── Verify transport state is queryable on both ─────────
                // Transport lives on the Project, not the Daw
                let session_transport = session_projects[0].transport().get_state().await?;
                let signal_transport = signal_projects[0].transport().get_state().await?;

                println!(
                    "  Session transport: playing={}, bpm={:.1}",
                    session_transport.is_playing(),
                    session_transport.tempo.bpm
                );
                println!(
                    "  Signal transport:  playing={}, bpm={:.1}",
                    signal_transport.is_playing(),
                    signal_transport.tempo.bpm
                );

                // Both should be stopped (freshly spawned, no project playing)
                assert!(
                    !session_transport.is_playing(),
                    "Session should not be playing on fresh spawn"
                );
                assert!(
                    !signal_transport.is_playing(),
                    "Signal should not be playing on fresh spawn"
                );

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: independent track operations
// ---------------------------------------------------------------------------

/// Spawn two REAPER instances and verify that adding tracks to one
/// does not affect the other.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn two_daw_independent_tracks() -> Result<()> {
    run_multi_reaper_test(
        "two_daw_independent_tracks",
        vec![
            DawInstanceConfig::new("daw_a"),
            DawInstanceConfig::new("daw_b"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_a = &ctx.by_label("daw_a").daw;
                let daw_b = &ctx.by_label("daw_b").daw;

                // Get the first project on each
                let projects_a = daw_a.projects().await?;
                let projects_b = daw_b.projects().await?;
                let project_a = &projects_a[0];
                let project_b = &projects_b[0];

                // Record initial track counts
                let initial_a = project_a.tracks().count().await?;
                let initial_b = project_b.tracks().count().await?;
                println!("  Initial tracks: A={}, B={}", initial_a, initial_b);

                // Add tracks only to DAW A
                project_a.tracks().add("Test Track A1", None).await?;
                project_a.tracks().add("Test Track A2", None).await?;
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;

                // Verify DAW A has 2 more tracks
                let after_a = project_a.tracks().count().await?;
                assert_eq!(after_a, initial_a + 2, "DAW A should have 2 new tracks");

                // Verify DAW B is unchanged
                let after_b = project_b.tracks().count().await?;
                assert_eq!(
                    after_b, initial_b,
                    "DAW B should be unaffected by tracks added to DAW A"
                );

                println!("  After adding to A: A={}, B={}", after_a, after_b);

                // Cleanup: remove all tracks (simpler than selective removal)
                project_a.tracks().remove_all().await?;

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: ExtState isolation
// ---------------------------------------------------------------------------

/// Verify that ExtState written in one REAPER instance is not visible
/// in another (they are separate processes with separate state).
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn two_daw_ext_state_isolation() -> Result<()> {
    run_multi_reaper_test(
        "two_daw_ext_state_isolation",
        vec![
            DawInstanceConfig::new("writer"),
            DawInstanceConfig::new("reader"),
        ],
        |ctx| {
            Box::pin(async move {
                let writer = &ctx.by_label("writer").daw;
                let reader = &ctx.by_label("reader").daw;

                // Write a value in the "writer" instance
                writer
                    .ext_state()
                    .set("TestSection", "test_key", "hello_from_writer", false)
                    .await?;

                // Verify it exists in the writer
                let writer_value = writer.ext_state().get("TestSection", "test_key").await?;
                assert_eq!(
                    writer_value.as_deref(),
                    Some("hello_from_writer"),
                    "Writer should see its own ExtState"
                );

                // Verify it does NOT exist in the reader (separate process)
                let reader_value = reader.ext_state().get("TestSection", "test_key").await?;
                assert!(
                    reader_value.is_none(),
                    "Reader should NOT see ExtState from writer (separate process)"
                );

                println!("  Writer sees: {:?}", writer_value);
                println!("  Reader sees: {:?}", reader_value);

                // Cleanup
                writer
                    .ext_state()
                    .delete("TestSection", "test_key", false)
                    .await?;

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: independent transport control
// ---------------------------------------------------------------------------

/// Spawn two REAPER instances and verify that play/stop/tempo/position
/// on one instance does not affect the other.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn two_daw_independent_transport() -> Result<()> {
    run_multi_reaper_test(
        "two_daw_independent_transport",
        vec![
            DawInstanceConfig::new("daw_a"),
            DawInstanceConfig::new("daw_b"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_a = &ctx.by_label("daw_a").daw;
                let daw_b = &ctx.by_label("daw_b").daw;

                let projects_a = daw_a.projects().await?;
                let projects_b = daw_b.projects().await?;
                let transport_a = projects_a[0].transport();
                let transport_b = projects_b[0].transport();

                // ── 1. Both start stopped ───────────────────────────────
                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                assert!(!state_a.is_playing(), "A should start stopped");
                assert!(!state_b.is_playing(), "B should start stopped");
                println!("  [1] Both stopped ✓");

                // ── 2. Play A only — B stays stopped ────────────────────
                transport_a.play().await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                let pos_a = transport_a.get_position().await?;
                assert!(state_a.is_playing(), "A should be playing");
                assert!(!state_b.is_playing(), "B should still be stopped");
                println!("  [2] A playing (pos {:.2}s), B stopped ✓", pos_a);

                // ── 3. Set different tempos ─────────────────────────────
                transport_a.set_tempo(140.0).await?;
                transport_b.set_tempo(90.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                assert!(
                    (state_a.tempo.bpm - 140.0).abs() < 0.5,
                    "A tempo should be ~140, got {:.1}",
                    state_a.tempo.bpm
                );
                assert!(
                    (state_b.tempo.bpm - 90.0).abs() < 0.5,
                    "B tempo should be ~90, got {:.1}",
                    state_b.tempo.bpm
                );
                println!(
                    "  [3] A tempo={:.1}, B tempo={:.1} ✓",
                    state_a.tempo.bpm, state_b.tempo.bpm
                );

                // ── 4. Seek B to 30s — A position unaffected ────────────
                transport_b.set_position(30.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let pos_a = transport_a.get_position().await?;
                let pos_b = transport_b.get_position().await?;
                assert!(
                    pos_a < 10.0,
                    "A position should be < 10s (still near start), got {:.2}",
                    pos_a
                );
                assert!(
                    (pos_b - 30.0).abs() < 1.0,
                    "B position should be ~30s, got {:.2}",
                    pos_b
                );
                println!("  [4] A pos={:.2}s, B pos={:.2}s ✓", pos_a, pos_b);

                // ── 5. Play B, stop A — roles reversed ──────────────────
                transport_b.play().await?;
                transport_a.stop().await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                let pos_a = transport_a.get_position().await?;
                let pos_b = transport_b.get_position().await?;
                assert!(!state_a.is_playing(), "A should be stopped now");
                assert!(state_b.is_playing(), "B should be playing now");
                println!(
                    "  [5] A stopped (pos {:.2}s), B playing (pos {:.2}s) ✓",
                    pos_a, pos_b
                );

                // ── 6. Verify B is advancing while playing ──────────────
                let pos_b_1 = transport_b.get_position().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;
                let pos_b_2 = transport_b.get_position().await?;
                assert!(
                    pos_b_2 > pos_b_1,
                    "B should be advancing: {:.2} -> {:.2}",
                    pos_b_1,
                    pos_b_2
                );
                println!("  [6] B advancing: {:.2}s -> {:.2}s ✓", pos_b_1, pos_b_2);

                // ── 7. Stop both ────────────────────────────────────────
                transport_b.stop().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                assert!(!state_a.is_playing(), "A should be stopped");
                assert!(!state_b.is_playing(), "B should be stopped");
                println!("  [7] Both stopped ✓");

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: concurrent track + transport operations
// ---------------------------------------------------------------------------

/// Spawn two REAPER instances. Perform track manipulation on one while
/// controlling transport on the other, verifying no cross-instance interference.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn two_daw_tracks_and_transport() -> Result<()> {
    run_multi_reaper_test(
        "two_daw_tracks_and_transport",
        vec![
            DawInstanceConfig::new("tracks"),
            DawInstanceConfig::new("transport"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_tracks = &ctx.by_label("tracks").daw;
                let daw_transport = &ctx.by_label("transport").daw;

                let projects_t = daw_tracks.projects().await?;
                let projects_x = daw_transport.projects().await?;
                let project_t = &projects_t[0];
                let project_x = &projects_x[0];
                let transport = project_x.transport();
                let tracks = project_t.tracks();

                // ── 1. Baseline: both idle, no extra tracks ─────────────
                let initial_count = tracks.count().await?;
                let state = transport.get_state().await?;
                assert!(!state.is_playing(), "transport DAW should start stopped");
                println!(
                    "  [1] Baseline: tracks={}, transport stopped ✓",
                    initial_count
                );

                // ── 2. Start playing on transport DAW, add tracks on tracks DAW
                transport.play().await?;
                let t1 = tracks.add("Guitar", None).await?;
                let t2 = tracks.add("Bass", None).await?;
                let t3 = tracks.add("Drums", None).await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let count = tracks.count().await?;
                let state = transport.get_state().await?;
                assert_eq!(count, initial_count + 3, "Should have 3 new tracks");
                assert!(state.is_playing(), "Transport should still be playing");
                println!("  [2] Added 3 tracks while transport plays ✓");

                // ── 3. Rename + set volume on tracks while transport seeks
                t1.rename("Lead Guitar").await?;
                t2.set_volume(0.5).await?;
                t3.set_pan(-0.75).await?;
                transport.set_position(15.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let pos = transport.get_position().await?;
                assert!(pos >= 14.0, "Transport should be near 15s, got {:.2}", pos);
                // Verify rename stuck
                let guitar = tracks.by_name("Lead Guitar").await?;
                assert!(guitar.is_some(), "Track should be renamed to 'Lead Guitar'");
                println!(
                    "  [3] Renamed/vol/pan tracks + seeked transport to {:.2}s ✓",
                    pos
                );

                // ── 4. Mute/solo on tracks DAW doesn't affect transport DAW
                t1.mute().await?;
                t3.solo().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                // Transport DAW should have no tracks matching our names
                let transport_tracks = project_x.tracks();
                let cross_check = transport_tracks.by_name("Lead Guitar").await?;
                assert!(
                    cross_check.is_none(),
                    "Transport DAW should not have tracks from tracks DAW"
                );
                let state = transport.get_state().await?;
                assert!(state.is_playing(), "Transport should still be playing");
                println!("  [4] Mute/solo on tracks DAW, transport unaffected ✓");

                // ── 5. Change tempo on transport while removing tracks
                transport.set_tempo(160.0).await?;
                tracks.remove_all().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let count = tracks.count().await?;
                let state = transport.get_state().await?;
                assert_eq!(count, 0, "All tracks should be removed");
                assert!(
                    (state.tempo.bpm - 160.0).abs() < 0.5,
                    "Tempo should be ~160, got {:.1}",
                    state.tempo.bpm
                );
                assert!(state.is_playing(), "Transport should still be playing");
                println!("  [5] Removed all tracks + tempo=160 ✓");

                // ── 6. Re-add tracks while stopping transport ───────────
                tracks.add("Synth Pad", None).await?;
                tracks.add("Vocals", None).await?;
                transport.stop().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let count = tracks.count().await?;
                let state = transport.get_state().await?;
                assert_eq!(count, 2, "Should have 2 new tracks");
                assert!(!state.is_playing(), "Transport should be stopped");
                println!("  [6] Added 2 tracks + stopped transport ✓");

                // Cleanup
                tracks.remove_all().await?;

                Ok(())
            })
        },
    )
}
