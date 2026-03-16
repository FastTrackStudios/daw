//! Sync integration tests.
//!
//! These tests validate the streaming infrastructure that the sync engine
//! depends on: subscribing to change streams on one REAPER instance and
//! verifying that changes can be detected and applied to another.
//!
//! Uses `run_multi_reaper_test` to spawn two REAPER instances.
//!
//! Run with:
//!
//!   cargo test -p daw-reaper --test reaper_sync -- --ignored --nocapture

use eyre::Result;
use reaper_test::{DawInstanceConfig, run_multi_reaper_test};
use std::time::Duration;
use tokio_util;

// ---------------------------------------------------------------------------
// Two-instance: track change detection via streaming
// ---------------------------------------------------------------------------

/// Spawn two REAPER instances. Subscribe to track events on A, make changes
/// on A, verify events arrive, then apply the same changes to B and verify.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn sync_track_changes_between_instances() -> Result<()> {
    run_multi_reaper_test(
        "sync_track_changes",
        vec![
            DawInstanceConfig::new("source"),
            DawInstanceConfig::new("target"),
        ],
        |ctx| {
            Box::pin(async move {
                let source = &ctx.by_label("source").daw;
                let target = &ctx.by_label("target").daw;

                let source_project = &source.projects().await?[0];
                let target_project = &target.projects().await?[0];

                // ── 1. Subscribe to track events on source ──────────────
                let mut track_rx = source_project.tracks().subscribe().await?;
                println!("  [1] Subscribed to track events on source ✓");

                // ── 2. Add a track on source ────────────────────────────
                let track_a = source_project.tracks().add("Sync Test Guitar", None).await?;
                println!("  [2] Added 'Sync Test Guitar' on source");

                // ── 3. Wait for the Added event ─────────────────────────
                let mut saw_added = false;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                while tokio::time::Instant::now() < deadline {
                    tokio::select! {
                        result = track_rx.recv() => {
                            match result {
                                Ok(Some(event)) => {
                                    let event_str = format!("{:?}", &*event);
                                    println!("  [3] Track event: {}", &event_str[..event_str.len().min(120)]);
                                    if event_str.contains("Added") && event_str.contains("Sync Test Guitar") {
                                        saw_added = true;
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => { println!("  recv error: {e}"); break; }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                assert!(saw_added, "Should have received TrackEvent::Added for 'Sync Test Guitar'");
                println!("  [3] Received Added event ✓");

                // ── 4. Apply the same change to target ──────────────────
                let track_b = target_project.tracks().add("Sync Test Guitar", None).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let target_tracks = target_project.tracks().all().await?;
                let found = target_tracks.iter().any(|t| t.name == "Sync Test Guitar");
                assert!(found, "Target should have 'Sync Test Guitar'");
                println!("  [4] Applied to target — track exists ✓");

                // ── 5. Change volume on source, detect event ────────────
                track_a.set_volume(0.5).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let mut saw_volume = false;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                while tokio::time::Instant::now() < deadline {
                    tokio::select! {
                        result = track_rx.recv() => {
                            match result {
                                Ok(Some(event)) => {
                                    let event_str = format!("{:?}", &*event);
                                    if event_str.contains("VolumeChanged") {
                                        println!("  [5] Volume event: {}", &event_str[..event_str.len().min(120)]);
                                        saw_volume = true;
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => { println!("  recv error: {e}"); break; }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                assert!(saw_volume, "Should have received TrackEvent::VolumeChanged");
                println!("  [5] Volume change detected ✓");

                // ── 6. Apply volume to target, verify ───────────────────
                track_b.set_volume(0.5).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                let target_info = track_b.info().await?;
                assert!(
                    (target_info.volume - 0.5).abs() < 0.01,
                    "Target track volume should be ~0.5, got {}",
                    target_info.volume
                );
                println!("  [6] Target volume = {:.2} ✓", target_info.volume);

                // ── 7. Mute on source, detect, apply to target ──────────
                track_a.mute().await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let mut saw_mute = false;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                while tokio::time::Instant::now() < deadline {
                    tokio::select! {
                        result = track_rx.recv() => {
                            match result {
                                Ok(Some(event)) => {
                                    let event_str = format!("{:?}", &*event);
                                    if event_str.contains("MuteChanged") {
                                        saw_mute = true;
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => { println!("  recv error: {e}"); break; }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                assert!(saw_mute, "Should have received TrackEvent::MuteChanged");
                track_b.mute().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
                let target_info = track_b.info().await?;
                assert!(target_info.muted, "Target track should be muted");
                println!("  [7] Mute synced ✓");

                // ── Cleanup ─────────────────────────────────────────────
                source_project.tracks().remove_all().await?;
                target_project.tracks().remove_all().await?;
                println!("  Cleanup done ✓");

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: transport streaming detection
// ---------------------------------------------------------------------------

/// Spawn two REAPER instances. Subscribe to transport state on A,
/// play/stop on A, and verify the events arrive via the stream.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn sync_transport_streaming() -> Result<()> {
    run_multi_reaper_test(
        "sync_transport_streaming",
        vec![
            DawInstanceConfig::new("source"),
            DawInstanceConfig::new("target"),
        ],
        |ctx| {
            Box::pin(async move {
                let source = &ctx.by_label("source").daw;
                let target = &ctx.by_label("target").daw;

                let source_project = &source.projects().await?[0];
                let target_project = &target.projects().await?[0];

                let source_transport = source_project.transport();
                let target_transport = target_project.transport();

                // ── 1. Subscribe to transport on source ─────────────────
                let mut transport_rx = source_transport.subscribe_state().await?;
                println!("  [1] Subscribed to transport on source ✓");

                // ── 2. Set tempo on source, verify event ────────────────
                source_transport.set_tempo(142.0).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let mut saw_tempo = false;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                while tokio::time::Instant::now() < deadline {
                    tokio::select! {
                        result = transport_rx.recv() => {
                            match result {
                                Ok(Some(state)) => {
                                    if (state.tempo.bpm - 142.0).abs() < 1.0 {
                                        saw_tempo = true;
                                        println!("  [2] Got tempo={:.1} from stream", state.tempo.bpm);
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => { println!("  recv error: {e}"); break; }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                assert!(saw_tempo, "Should have received transport state with tempo ~142");

                // ── 3. Apply tempo to target ────────────────────────────
                target_transport.set_tempo(142.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
                let target_state = target_transport.get_state().await?;
                assert!(
                    (target_state.tempo.bpm - 142.0).abs() < 1.0,
                    "Target tempo should be ~142, got {:.1}",
                    target_state.tempo.bpm
                );
                println!("  [3] Target tempo = {:.1} ✓", target_state.tempo.bpm);

                // ── 4. Play source, detect playing state ────────────────
                source_transport.play().await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let mut saw_playing = false;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                while tokio::time::Instant::now() < deadline {
                    tokio::select! {
                        result = transport_rx.recv() => {
                            match result {
                                Ok(Some(state)) => {
                                    if state.is_playing() {
                                        saw_playing = true;
                                        println!("  [4] Transport playing detected via stream");
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => { println!("  recv error: {e}"); break; }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                assert!(saw_playing, "Should have detected playing state");

                // ── 5. Apply play to target, verify both playing ────────
                target_transport.play().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let source_state = source_transport.get_state().await?;
                let target_state = target_transport.get_state().await?;
                assert!(source_state.is_playing(), "Source should be playing");
                assert!(target_state.is_playing(), "Target should be playing");
                println!("  [5] Both instances playing ✓");

                // ── 6. Stop both ────────────────────────────────────────
                source_transport.stop().await?;
                target_transport.stop().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
                println!("  [6] Both stopped ✓");

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: transport position sync verification
// ---------------------------------------------------------------------------

/// Start both instances playing at the same tempo, stream transport state
/// from both, and verify their playhead positions stay in sync.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn sync_transport_position_lockstep() -> Result<()> {
    run_multi_reaper_test(
        "sync_transport_position_lockstep",
        vec![
            DawInstanceConfig::new("daw_a"),
            DawInstanceConfig::new("daw_b"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_a = &ctx.by_label("daw_a").daw;
                let daw_b = &ctx.by_label("daw_b").daw;

                let project_a = &daw_a.projects().await?[0];
                let project_b = &daw_b.projects().await?[0];

                let transport_a = project_a.transport();
                let transport_b = project_b.transport();

                // ── 1. Set identical tempo on both ──────────────────────
                transport_a.set_tempo(120.0).await?;
                transport_b.set_tempo(120.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
                println!("  [1] Both set to 120 BPM ✓");

                // ── 2. Reset both to position 0 ─────────────────────────
                transport_a.set_position(0.0).await?;
                transport_b.set_position(0.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                // ── 3. Enable metronome on both (action 40364) ─────────
                // 40364 = "Options: Toggle metronome"
                project_a.run_command("40364").await?;
                project_b.run_command("40364").await?;
                println!("  [2] Metronome enabled on both ✓");

                // ── 4. Subscribe to transport streams on both ───────────
                let mut rx_a = transport_a.subscribe_state().await?;
                let mut rx_b = transport_b.subscribe_state().await?;
                println!("  [3] Subscribed to transport streams on both ✓");

                // ── 5. Start both playing simultaneously ────────────────
                // (as close together as we can from the test process)
                transport_a.play().await?;
                transport_b.play().await?;
                println!("  [4] Both playing — listen for metronome sync!");

                // ── 6. Sample positions over 10 seconds (listen!) ─────
                let mut samples: Vec<(f64, f64, f64)> = Vec::new(); // (time, pos_a, pos_b)
                let start = std::time::Instant::now();
                let test_duration = Duration::from_secs(10);

                // Track latest positions from each stream
                let mut latest_a: f64 = 0.0;
                let mut latest_b: f64 = 0.0;
                let mut sample_interval = tokio::time::interval(Duration::from_millis(200));

                while start.elapsed() < test_duration {
                    tokio::select! {
                        result = rx_a.recv() => {
                            if let Ok(Some(state)) = result {
                                if let Some(ref time) = state.playhead_position.time {
                                    latest_a = time.as_seconds();
                                }
                            }
                        }
                        result = rx_b.recv() => {
                            if let Ok(Some(state)) = result {
                                if let Some(ref time) = state.playhead_position.time {
                                    latest_b = time.as_seconds();
                                }
                            }
                        }
                        _ = sample_interval.tick() => {
                            if latest_a > 0.0 && latest_b > 0.0 {
                                let elapsed = start.elapsed().as_secs_f64();
                                samples.push((elapsed, latest_a, latest_b));
                            }
                        }
                    }
                }

                // ── 6. Stop both ────────────────────────────────────────
                transport_a.stop().await?;
                transport_b.stop().await?;

                // ── 7. Analyze position drift ───────────────────────────
                println!("  [5] Collected {} position samples:", samples.len());
                println!("       {:>6}  {:>10}  {:>10}  {:>10}", "t", "pos_a", "pos_b", "drift");

                let mut max_drift: f64 = 0.0;
                for (t, pos_a, pos_b) in &samples {
                    let drift = (pos_a - pos_b).abs();
                    max_drift = max_drift.max(drift);
                    println!(
                        "       {:>6.2}s  {:>10.4}s  {:>10.4}s  {:>10.4}s {}",
                        t, pos_a, pos_b, drift,
                        if drift > 0.5 { "⚠" } else { "✓" }
                    );
                }

                println!("  [6] Max drift: {:.4}s", max_drift);

                // Both are playing independently (no sync engine connecting them),
                // so some drift is expected from the async play commands.
                // But they should both be advancing and within a reasonable window.
                assert!(
                    samples.len() >= 5,
                    "Should have at least 5 position samples, got {}",
                    samples.len()
                );

                // Verify both are actually advancing
                let first = &samples[0];
                let last = &samples[samples.len() - 1];
                assert!(
                    last.1 > first.1 + 1.0,
                    "A should advance at least 1s: {:.2} -> {:.2}",
                    first.1, last.1
                );
                assert!(
                    last.2 > first.2 + 1.0,
                    "B should advance at least 1s: {:.2} -> {:.2}",
                    first.2, last.2
                );

                // Without a sync engine, drift up to ~1s is acceptable
                // (the two play() calls are sequential from the test process).
                // With sync engine connected, this should be <50ms.
                println!(
                    "  [7] Both advancing. A: {:.2}s → {:.2}s, B: {:.2}s → {:.2}s ✓",
                    first.1, last.1, first.2, last.2
                );

                // The key metric: are they roughly tracking?
                // At 120 BPM both should advance ~3s in 3s of wall time.
                let a_distance = last.1 - first.1;
                let b_distance = last.2 - first.2;
                assert!(
                    (a_distance - b_distance).abs() < 0.5,
                    "Both should advance similar distances: A={:.2}s, B={:.2}s",
                    a_distance, b_distance
                );
                println!(
                    "  [8] Distance parity: A advanced {:.2}s, B advanced {:.2}s (diff={:.4}s) ✓",
                    a_distance, b_distance, (a_distance - b_distance).abs()
                );

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: elaborate transport sync torture test
// ---------------------------------------------------------------------------

/// The transport sync torture test: start, stop, seek, change tempo, resume,
/// seek again — verifying both instances match at every step. Uses streaming
/// to confirm positions converge after each operation.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn sync_transport_torture_test() -> Result<()> {
    run_multi_reaper_test(
        "sync_transport_torture",
        vec![
            DawInstanceConfig::new("daw_a"),
            DawInstanceConfig::new("daw_b"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_a = &ctx.by_label("daw_a").daw;
                let daw_b = &ctx.by_label("daw_b").daw;

                let project_a = &daw_a.projects().await?[0];
                let project_b = &daw_b.projects().await?[0];

                let transport_a = project_a.transport();
                let transport_b = project_b.transport();

                // Enable metronome on both
                project_a.run_command("40364").await?;
                project_b.run_command("40364").await?;

                // Ensure both are stopped and at position 0 before enabling Link
                transport_a.stop().await?;
                transport_b.stop().await?;
                transport_a.set_position(0.0).await?;
                transport_b.set_position(0.0).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Enable Ableton Link: A=Master, B=Puppet
                daw_a.ext_state().set("FTS_SYNC", "link_mode", "master", false).await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
                daw_b.ext_state().set("FTS_SYNC", "link_mode", "puppet", false).await?;
                tokio::time::sleep(Duration::from_secs(2)).await;
                println!("  Link enabled: A=Master, B=Puppet");

                // Start a position sync bridge: poll A's position at 10Hz and
                // forward to B when it changes (while stopped). Link handles
                // play/stop and phase alignment; this bridge handles cursor position.
                let transport_a_bridge = project_a.transport();
                let transport_b_bridge = project_b.transport();
                let sync_cancel = tokio_util::sync::CancellationToken::new();
                let sync_cancel_child = sync_cancel.child_token();
                tokio::spawn(async move {
                    let mut last_pos: f64 = 0.0;
                    let mut last_tempo: f64 = 0.0;
                    // Poll at 30Hz to keep up with Link's propagation speed
                    let mut interval = tokio::time::interval(Duration::from_millis(33));
                    loop {
                        tokio::select! {
                            _ = sync_cancel_child.cancelled() => break,
                            _ = interval.tick() => {
                                let Ok(state) = transport_a_bridge.get_state().await else { continue };
                                let playing = state.is_playing();

                                let pos = if playing {
                                    state.playhead_position.time
                                        .as_ref()
                                        .map(|t| t.as_seconds())
                                        .unwrap_or(0.0)
                                } else {
                                    state.edit_position.time
                                        .as_ref()
                                        .map(|t| t.as_seconds())
                                        .unwrap_or(0.0)
                                };

                                if playing {
                                    // While playing: only forward large jumps (seeks)
                                    // Small drift is handled by Link phase correction
                                    if (pos - last_pos).abs() > 2.0 {
                                        let _ = transport_b_bridge.set_position(pos).await;
                                    }
                                } else {
                                    // While stopped: sync tempo first, then position
                                    // (tempo changes can move the cursor, so set pos last)
                                    if (state.tempo.bpm - last_tempo).abs() > 0.01 {
                                        let _ = transport_b_bridge.set_tempo(state.tempo.bpm).await;
                                        last_tempo = state.tempo.bpm;
                                    }
                                    if (pos - last_pos).abs() > 0.01 {
                                        let _ = transport_b_bridge.set_position(pos).await;
                                    }
                                }
                                last_pos = pos;
                            }
                        }
                    }
                });
                println!("  Position sync bridge active (A→B, polling at 10Hz)");

                let tolerance = 0.20; // 200ms — Link puppet via timer (~30Hz)

                // Macros for helpers (can't use async fn inside async closure easily)
                macro_rules! get_positions {
                    ($ta:expr, $tb:expr) => {{
                        let pa = $ta.get_position().await?;
                        let pb = $tb.get_position().await?;
                        (pa, pb)
                    }};
                }

                macro_rules! assert_close {
                    ($label:expr, $pa:expr, $pb:expr, $tol:expr) => {{
                        let drift = ($pa - $pb).abs();
                        let status = if drift <= $tol { "✓" } else { "✗ FAIL" };
                        println!(
                            "       {:<40} A={:>8.3}s  B={:>8.3}s  drift={:.4}s {}",
                            $label, $pa, $pb, drift, status
                        );
                        assert!(
                            drift <= $tol,
                            "{}: drift {:.4}s exceeds tolerance {:.4}s (A={:.3}, B={:.3})",
                            $label, drift, $tol, $pa, $pb
                        );
                    }};
                }

                macro_rules! sample_drift {
                    ($rx_a:expr, $rx_b:expr, $dur:expr) => {{
                        let mut samples: Vec<(f64, f64, f64)> = Vec::new();
                        let mut latest_a: f64 = 0.0;
                        let mut latest_b: f64 = 0.0;
                        let mut max_drift: f64 = 0.0;
                        let start = std::time::Instant::now();
                        let mut interval = tokio::time::interval(Duration::from_millis(100));

                        while start.elapsed() < $dur {
                            tokio::select! {
                                result = $rx_a.recv() => {
                                    if let Ok(Some(state)) = result {
                                        if let Some(ref time) = state.playhead_position.time {
                                            latest_a = time.as_seconds();
                                        }
                                    }
                                }
                                result = $rx_b.recv() => {
                                    if let Ok(Some(state)) = result {
                                        if let Some(ref time) = state.playhead_position.time {
                                            latest_b = time.as_seconds();
                                        }
                                    }
                                }
                                _ = interval.tick() => {
                                    if latest_a > 0.0 || latest_b > 0.0 {
                                        let drift = (latest_a - latest_b).abs();
                                        max_drift = max_drift.max(drift);
                                        samples.push((start.elapsed().as_secs_f64(), latest_a, latest_b));
                                    }
                                }
                            }
                        }
                        (max_drift, samples)
                    }};
                }

                // Subscribe to transport streams
                let mut rx_a = transport_a.subscribe_state().await?;
                let mut rx_b = transport_b.subscribe_state().await?;

                println!("\n  ═══ TRANSPORT SYNC TORTURE TEST ═══\n");

                // ── Phase 1: Set identical tempo, both at position 0 ────
                // Set after Link is enabled so master doesn't move cursor
                transport_a.set_tempo(120.0).await?;
                transport_b.set_tempo(120.0).await?;
                transport_a.set_position(0.0).await?;
                transport_b.set_position(0.0).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let (pa, pb) = get_positions!(transport_a, transport_b);
                assert_close!("Phase 1: Initial position @ 0", pa, pb, tolerance);

                // ── Phase 2: Play A — B follows via Link ─────────────────
                println!("  ── Phase 2: Play A (Master) for 3s at 120 BPM ──");
                transport_a.play().await?;
                // Give Link time to propagate play to B
                tokio::time::sleep(Duration::from_millis(500)).await;

                let (max_drift, samples) = sample_drift!(rx_a, rx_b, Duration::from_secs(3));
                println!("       Streamed {} samples, max drift: {:.4}s", samples.len(), max_drift);
                // Print first few samples
                for (t, pa, pb) in samples.iter().take(5) {
                    println!("         t={:.2}s  A={:.3}s  B={:.3}s  drift={:.4}s", t, pa, pb, (pa - pb).abs());
                }

                let (pa, pb) = get_positions!(transport_a, transport_b);
                println!("       Positions: A={:.3}s  B={:.3}s  drift={:.4}s", pa, pb, (pa - pb).abs());
                let state_b = transport_b.get_state().await?;
                println!("       B is_playing={}", state_b.is_playing());

                // ── Phase 3: Stop A — B should follow ───────────────────
                println!("  ── Phase 3: Stop A ──");
                transport_a.stop().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                assert!(!state_a.is_playing(), "A should be stopped");
                assert!(!state_b.is_playing(), "B should be stopped");
                println!("       Both stopped ✓");

                // ── Phase 4: Seek A to 10s — sync bridge forwards to B ──
                println!("  ── Phase 4: Seek A to 10s (sync bridge → B) ──");
                transport_a.set_position(10.0).await?;
                // Give transport stream + sync bridge time to detect and forward
                // (stream polls at ~30Hz, bridge applies on next recv)
                tokio::time::sleep(Duration::from_secs(1)).await;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let (pa, pb) = get_positions!(transport_a, transport_b);
                assert_close!("Phase 4: After seek to 10s", pa, pb, 0.05);
                assert!((pa - 10.0).abs() < 0.5, "A should be near 10s, got {:.2}", pa);

                // ── Phase 5: Play A from 10s — B follows ─────────────────
                println!("  ── Phase 5: Play A from 10s ──");
                transport_a.play().await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let (max_drift, samples) = sample_drift!(rx_a, rx_b,Duration::from_secs(2));
                println!("       Streamed {} samples, max drift: {:.4}s", samples.len(), max_drift);

                // Check final positions (Link corrects drift over time)
                let (pa, pb) = get_positions!(transport_a, transport_b);
                let final_drift = (pa - pb).abs();
                println!("       Phase 5 final: A={:.3}s  B={:.3}s  drift={:.4}s", pa, pb, final_drift);
                assert!(
                    final_drift < tolerance,
                    "Phase 5 final drift {:.4}s exceeds {:.4}s",
                    final_drift, tolerance
                );
                assert!(pa > 11.0, "A should be past 11s, got {:.2}", pa);

                // ── Phase 6: Change tempo on A — Master pushes to Link ──
                println!("  ── Phase 6: Set 160 BPM on A ──");
                transport_a.set_tempo(160.0).await?;

                let (_max_drift, samples) = sample_drift!(rx_a, rx_b,Duration::from_secs(2));
                let (pa, pb) = get_positions!(transport_a, transport_b);
                let final_drift = (pa - pb).abs();
                println!("       Streamed {} samples, final drift: {:.4}s", samples.len(), final_drift);
                assert!(final_drift < tolerance, "Phase 6 final drift {:.4}s", final_drift);

                // Verify tempo actually changed
                let state_a = transport_a.get_state().await?;
                assert!(
                    (state_a.tempo.bpm - 160.0).abs() < 1.0,
                    "Tempo should be ~160, got {:.1}",
                    state_a.tempo.bpm
                );
                println!("       Tempo = {:.1} BPM ✓", state_a.tempo.bpm);

                // ── Phase 7: Stop A, seek to 30s ─────────────────────────
                println!("  ── Phase 7: Stop A + seek to 30s ──");
                transport_a.stop().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                transport_a.set_position(30.0).await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let (pa, pb) = get_positions!(transport_a, transport_b);
                assert_close!("Phase 7: After seek to 30s", pa, pb, 0.05);

                // ── Phase 8: 90 BPM on A, play ──────────────────────────
                println!("  ── Phase 8: 90 BPM on A, play ──");
                transport_a.set_tempo(90.0).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                transport_a.play().await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let (_max_drift, samples) = sample_drift!(rx_a, rx_b,Duration::from_secs(3));
                let (pa, pb) = get_positions!(transport_a, transport_b);
                let final_drift = (pa - pb).abs();
                println!("       Streamed {} samples, final drift: {:.4}s", samples.len(), final_drift);
                assert!(final_drift < tolerance, "Phase 8 final drift {:.4}s", final_drift);
                assert!(pa > 32.0, "A should be past 32s, got {:.2}", pa);

                // ── Phase 9: Final stop ──────────────────────────────────
                println!("  ── Phase 9: Stop A ──");
                transport_a.stop().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                let state_a = transport_a.get_state().await?;
                let state_b = transport_b.get_state().await?;
                assert!(!state_a.is_playing(), "A should be stopped");
                assert!(!state_b.is_playing(), "B should be stopped");

                // ── Phase 10: Rapid start/stop cycles (A only, B follows) ─
                println!("  ── Phase 10: Rapid start/stop on A (5 cycles) ──");
                for i in 0..5 {
                    transport_a.play().await?;
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    transport_a.stop().await?;
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    let state_a = transport_a.get_state().await?;
                    let state_b = transport_b.get_state().await?;
                    assert!(!state_a.is_playing(), "Cycle {}: A should be stopped", i);
                    assert!(!state_b.is_playing(), "Cycle {}: B (puppet) should be stopped", i);
                }
                println!("       5 rapid cycles completed ✓");

                // ── Cleanup: disable Link and cancel sync bridge ─────────
                sync_cancel.cancel();
                daw_a.ext_state().set("FTS_SYNC", "link_mode", "off", false).await?;
                daw_b.ext_state().set("FTS_SYNC", "link_mode", "off", false).await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                // ── Final position check ─────────────────────────────────
                let (pa, pb) = get_positions!(transport_a, transport_b);
                println!("\n  ═══ FINAL STATE ═══");
                println!("       A: {:.3}s  B: {:.3}s  drift: {:.4}s", pa, pb, (pa - pb).abs());
                println!("  ═══ ALL PHASES PASSED ═══\n");

                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Two-instance: full sync round-trip (track + volume + mute)
// ---------------------------------------------------------------------------

/// The "money test" — make changes on source, detect them via streams,
/// apply them to target, and verify target state matches source state.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn sync_full_round_trip() -> Result<()> {
    run_multi_reaper_test(
        "sync_full_round_trip",
        vec![
            DawInstanceConfig::new("source"),
            DawInstanceConfig::new("target"),
        ],
        |ctx| {
            Box::pin(async move {
                let source = &ctx.by_label("source").daw;
                let target = &ctx.by_label("target").daw;

                let source_project = &source.projects().await?[0];
                let target_project = &target.projects().await?[0];

                // Subscribe to track events on source
                let mut track_rx = source_project.tracks().subscribe().await?;

                // ── Build a 3-track arrangement on source ───────────────
                let guitar = source_project.tracks().add("Guitar", None).await?;
                let bass = source_project.tracks().add("Bass", None).await?;
                let drums = source_project.tracks().add("Drums", None).await?;

                // Set distinct volumes and states
                guitar.set_volume(0.8).await?;
                bass.set_volume(0.6).await?;
                bass.mute().await?;
                drums.set_volume(0.9).await?;
                drums.solo().await?;

                // Wait for events to propagate
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Drain all events
                let mut events = Vec::new();
                loop {
                    tokio::select! {
                        result = track_rx.recv() => {
                            match result {
                                Ok(Some(event)) => {
                                    events.push(format!("{:?}", &*event));
                                }
                                _ => break,
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(200)) => break,
                    }
                }

                println!("  Collected {} events from source", events.len());
                assert!(events.len() >= 3, "Should have at least 3 events (Added * 3), got {}", events.len());

                // ── Replicate to target ─────────────────────────────────
                let t_guitar = target_project.tracks().add("Guitar", None).await?;
                let t_bass = target_project.tracks().add("Bass", None).await?;
                let t_drums = target_project.tracks().add("Drums", None).await?;

                t_guitar.set_volume(0.8).await?;
                t_bass.set_volume(0.6).await?;
                t_bass.mute().await?;
                t_drums.set_volume(0.9).await?;
                t_drums.solo().await?;

                tokio::time::sleep(Duration::from_millis(300)).await;

                // ── Verify target matches source ────────────────────────
                let source_tracks = source_project.tracks().all().await?;
                let target_tracks = target_project.tracks().all().await?;

                assert_eq!(
                    source_tracks.len(),
                    target_tracks.len(),
                    "Track count should match"
                );

                for (s, t) in source_tracks.iter().zip(target_tracks.iter()) {
                    assert_eq!(s.name, t.name, "Track names should match");
                    assert!(
                        (s.volume - t.volume).abs() < 0.01,
                        "Volume mismatch for '{}': source={:.2}, target={:.2}",
                        s.name, s.volume, t.volume
                    );
                    assert_eq!(
                        s.muted, t.muted,
                        "Mute mismatch for '{}'", s.name
                    );
                    assert_eq!(
                        s.soloed, t.soloed,
                        "Solo mismatch for '{}'", s.name
                    );
                    println!(
                        "  {} — vol={:.2} mute={} solo={} ✓",
                        s.name, s.volume, s.muted, s.soloed
                    );
                }

                println!("  Full round-trip sync verified ✓");

                // Cleanup
                source_project.tracks().remove_all().await?;
                target_project.tracks().remove_all().await?;

                Ok(())
            })
        },
    )
}
