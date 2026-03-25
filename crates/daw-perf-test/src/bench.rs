//! Benchmark suite comparing native reaper-rs, individual RPC, and batch RPC.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use daw::service::batch::*;
use daw::{BatchBuilder, Daw};
use tracing::info;

// ============================================================================
// Helpers
// ============================================================================

fn fmt_dur(d: Duration) -> String {
    if d.as_millis() >= 1000 {
        format!("{:.2}s", d.as_secs_f64())
    } else if d.as_millis() > 0 {
        format!("{:.2}ms", d.as_secs_f64() * 1000.0)
    } else {
        format!("{}µs", d.as_micros())
    }
}

fn log_result(label: &str, n: u32, elapsed: Duration) {
    info!(
        "  {label:<30} {n:>5} ops  {total:>10}  ({per_op}/op)",
        total = fmt_dur(elapsed),
        per_op = fmt_dur(elapsed / n),
    );
}

fn log_comparison(native: Duration, individual: Duration, batch: Duration) {
    let ind_vs_native = individual.as_secs_f64() / native.as_secs_f64();
    let batch_vs_native = batch.as_secs_f64() / native.as_secs_f64();
    let batch_vs_individual = individual.as_secs_f64() / batch.as_secs_f64();
    info!("  Ratios:");
    info!("    Individual RPC / Native:  {ind_vs_native:.1}x slower");
    info!("    Batch RPC / Native:       {batch_vs_native:.1}x slower");
    info!("    Individual RPC / Batch:   {batch_vs_individual:.1}x (batch speedup)");
}

/// Connect to daw-bridge's Unix socket and return a Daw handle.
async fn connect_to_daw_bridge() -> eyre::Result<Daw> {
    let socket_path = if let Ok(path) = std::env::var("FTS_SOCKET") {
        PathBuf::from(path)
    } else {
        let pid = std::process::id();
        PathBuf::from(format!("/tmp/fts-daw-{pid}.sock"))
    };

    if !socket_path.exists() {
        return Err(eyre::eyre!(
            "daw-bridge socket not found at {}. Is daw-bridge loaded?",
            socket_path.display()
        ));
    }

    let stream = tokio::net::UnixStream::connect(&socket_path).await?;
    let link = vox_stream::StreamLink::unix(stream);
    let handshake = vox::HandshakeResult {
        role: vox::SessionRole::Initiator,
        our_settings: vox::ConnectionSettings {
            parity: vox::Parity::Odd,
            max_concurrent_requests: 64,
        },
        peer_settings: vox::ConnectionSettings {
            parity: vox::Parity::Even,
            max_concurrent_requests: 64,
        },
        peer_supports_retry: true,
        session_resume_key: None,
        peer_resume_key: None,
        our_schema: vec![],
        peer_schema: vec![],
    };
    let (_root_caller, session) = vox::initiator_conduit(vox::BareConduit::new(link), handshake)
        .establish::<vox::DriverCaller>(())
        .await
        .map_err(|e| eyre::eyre!("Failed to establish vox session: {:?}", e))?;

    let conn = session
        .open_connection(
            vox::ConnectionSettings {
                parity: vox::Parity::Odd,
                max_concurrent_requests: 64,
            },
            vec![vox::MetadataEntry {
                key: "role",
                value: vox::MetadataValue::String("perf-test"),
                flags: vox::MetadataFlags::NONE,
            }],
        )
        .await
        .map_err(|e| eyre::eyre!("open_connection failed: {e:?}"))?;

    let mut driver = vox::Driver::new(conn, ());
    let caller = vox::ErasedCaller::new(driver.caller());
    moire::task::spawn(async move { driver.run().await });

    Ok(Daw::new(caller))
}

// ============================================================================
// Native reaper-rs benchmarks (runs closures on main thread)
// ============================================================================

async fn native_create_tracks(n: u32) -> Duration {
    let start = Instant::now();
    daw::reaper::main_thread::query(move || {
        let reaper = reaper_high::Reaper::get();
        let project = reaper.current_project();
        for i in 0..n {
            let name = format!("NativeTrack-{i}");
            if let Ok(track) = project.insert_track_at(project.track_count()) {
                track.set_name(name.as_str());
            }
        }
    })
    .await;
    start.elapsed()
}

async fn native_remove_all_tracks() {
    daw::reaper::main_thread::query(move || {
        let reaper = reaper_high::Reaper::get();
        let project = reaper.current_project();
        while project.track_count() > 0 {
            if let Some(track) = project.track_by_index(project.track_count() - 1) {
                let _ = project.remove_track(&track);
            } else {
                break;
            }
        }
    })
    .await;
}

async fn native_mutate_tracks(n: u32) -> Duration {
    let start = Instant::now();
    daw::reaper::main_thread::query(move || {
        let reaper = reaper_high::Reaper::get();
        let project = reaper.current_project();
        let count = project.track_count().min(n);
        for i in 0..count {
            if let Some(track) = project.track_by_index(i) {
                let name = format!("NativeMutated-{i}");
                track.set_name(name.as_str());
                if let Ok(vol) = reaper_medium::ReaperVolumeValue::new(0.5 + (i as f64) * 0.001) {
                    let _ = track.set_volume_smart(vol, Default::default());
                }
                use reaper_high::GroupingBehavior;
                use reaper_medium::GangBehavior;
                if i % 2 == 0 {
                    track.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                } else {
                    track.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                }
            }
        }
    })
    .await;
    start.elapsed()
}

// ============================================================================
// Individual RPC benchmarks
// ============================================================================

async fn rpc_create_tracks(daw: &Daw, n: u32) -> eyre::Result<Duration> {
    let project = daw.current_project().await?;
    let tracks = project.tracks();
    let start = Instant::now();
    for i in 0..n {
        tracks.add(&format!("RpcTrack-{i}"), None).await?;
    }
    Ok(start.elapsed())
}

async fn rpc_remove_all_tracks(daw: &Daw) -> eyre::Result<()> {
    let project = daw.current_project().await?;
    project.tracks().remove_all().await?;
    Ok(())
}

async fn rpc_mutate_tracks(daw: &Daw, n: u32) -> eyre::Result<Duration> {
    let project = daw.current_project().await?;
    let tracks_svc = project.tracks();
    let all = tracks_svc.all().await?;
    let count = (all.len() as u32).min(n);
    let start = Instant::now();
    for i in 0..count {
        let track = &all[i as usize];
        let handle = tracks_svc
            .by_guid(&track.guid)
            .await?
            .expect("track should exist");
        handle.rename(&format!("RpcMutated-{i}")).await?;
        handle.set_volume(0.5 + (i as f64) * 0.001).await?;
        if i % 2 == 0 {
            handle.mute().await?;
        } else {
            handle.unmute().await?;
        }
    }
    Ok(start.elapsed())
}

// ============================================================================
// Batch RPC benchmarks
// ============================================================================

async fn batch_create_tracks(daw: &Daw, n: u32) -> eyre::Result<Duration> {
    let mut b = BatchBuilder::new().with_undo("Batch create tracks");
    let proj = b.current_project();
    for i in 0..n {
        b.add_track(&proj, format!("BatchTrack-{i}"), None);
    }
    let start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let elapsed = start.elapsed();

    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_)))
        .collect();
    if !errors.is_empty() {
        return Err(eyre::eyre!(
            "Batch create tracks had {} errors",
            errors.len()
        ));
    }
    Ok(elapsed)
}

async fn batch_mutate_tracks(daw: &Daw, n: u32) -> eyre::Result<Duration> {
    let project = daw.current_project().await?;
    let all = project.tracks().all().await?;
    let count = (all.len() as u32).min(n);

    let mut b = BatchBuilder::new().with_undo("Batch mutate tracks");
    let proj = b.current_project();
    for i in 0..count {
        let tref = daw::service::TrackRef::Guid(all[i as usize].guid.clone());
        b.rename_track(&proj, tref.clone(), format!("BatchMutated-{i}"));
        b.set_track_volume(&proj, tref.clone(), 0.5 + (i as f64) * 0.001);
        b.push_raw::<()>(BatchOp::Track(TrackOp::SetMuted(
            ProjectArg::FromStep(proj.index()),
            TrackArg::Literal(tref),
            i % 2 == 0,
        )));
    }

    let start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let elapsed = start.elapsed();

    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_)))
        .collect();
    if !errors.is_empty() {
        return Err(eyre::eyre!(
            "Batch mutate tracks had {} errors",
            errors.len()
        ));
    }
    Ok(elapsed)
}

async fn batch_create_and_mutate(daw: &Daw, n: u32) -> eyre::Result<Duration> {
    let mut b = BatchBuilder::new().with_undo("Batch create+mutate");
    let proj = b.current_project();

    // Create N tracks and immediately mutate each via FromStep reference
    let mut handles = Vec::with_capacity(n as usize);
    for i in 0..n {
        let h = b.add_track(&proj, format!("BatchCM-{i}"), None);
        handles.push(h);
    }
    for (i, handle) in handles.iter().enumerate() {
        b.push_raw::<()>(BatchOp::Track(TrackOp::SetVolume(
            ProjectArg::FromStep(proj.index()),
            TrackArg::FromStep(handle.index()),
            0.7,
        )));
        b.push_raw::<()>(BatchOp::Track(TrackOp::SetMuted(
            ProjectArg::FromStep(proj.index()),
            TrackArg::FromStep(handle.index()),
            i % 2 == 0,
        )));
    }

    let total_ops = n * 3; // create + volume + mute
    let start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let elapsed = start.elapsed();

    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_)))
        .collect();
    if !errors.is_empty() {
        for e in &errors[..errors.len().min(3)] {
            info!("  error at step {}: {:?}", e.step, e.outcome);
        }
        return Err(eyre::eyre!(
            "Batch create+mutate had {} errors",
            errors.len()
        ));
    }

    info!(
        "  (create+mutate batch: {} ops in 1 RPC, {}/op)",
        total_ops,
        fmt_dur(elapsed / total_ops)
    );
    Ok(elapsed)
}

// ============================================================================
// Main benchmark suite
// ============================================================================

pub async fn run_all() -> eyre::Result<()> {
    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║          DAW Performance Benchmark Suite                    ║");
    info!("╚══════════════════════════════════════════════════════════════╝");
    info!("");

    // Connect to daw-bridge over Unix socket
    info!("Connecting to daw-bridge socket...");
    let daw = connect_to_daw_bridge().await?;
    info!("Connected.");
    info!("");

    // Clean slate
    rpc_remove_all_tracks(&daw).await?;

    // ── Benchmark 1: Create tracks ──────────────────────────────────────
    for &n in &[100u32, 500] {
        info!("── Create {n} tracks ──────────────────────────────");

        // Native reaper-rs
        let native_elapsed = native_create_tracks(n).await;
        log_result("Native reaper-rs", n, native_elapsed);
        native_remove_all_tracks().await;

        // Individual RPC
        let rpc_elapsed = rpc_create_tracks(&daw, n).await?;
        log_result("Individual RPC", n, rpc_elapsed);
        rpc_remove_all_tracks(&daw).await?;

        // Batch RPC
        let batch_elapsed = batch_create_tracks(&daw, n).await?;
        log_result("Batch RPC", n, batch_elapsed);

        log_comparison(native_elapsed, rpc_elapsed, batch_elapsed);
        info!("");

        rpc_remove_all_tracks(&daw).await?;
    }

    // ── Benchmark 2: Mutate existing tracks (rename + volume + mute) ────
    for &n in &[100u32, 200] {
        info!("── Mutate {n} tracks (3 ops each) ────────────────");

        // Setup N tracks via native
        native_create_tracks(n).await;

        let native_elapsed = native_mutate_tracks(n).await;
        log_result("Native reaper-rs", n * 3, native_elapsed);

        let rpc_elapsed = rpc_mutate_tracks(&daw, n).await?;
        log_result("Individual RPC", n * 3, rpc_elapsed);

        let batch_elapsed = batch_mutate_tracks(&daw, n).await?;
        log_result("Batch RPC", n * 3, batch_elapsed);

        log_comparison(native_elapsed, rpc_elapsed, batch_elapsed);
        info!("");

        native_remove_all_tracks().await;
    }

    // ── Benchmark 3: Create + mutate in single batch (FromStep chains) ──
    for &n in &[100u32, 500] {
        info!("── Create + mutate {n} tracks (3 ops each, 1 batch) ──");

        // For comparison: sequential create + mutate individually
        let seq_start = Instant::now();
        {
            let project = daw.current_project().await?;
            let tracks = project.tracks();
            for i in 0..n {
                let handle = tracks.add(&format!("SeqCM-{i}"), None).await?;
                handle.set_volume(0.7).await?;
                if i % 2 == 0 {
                    handle.mute().await?;
                }
            }
        }
        let seq_elapsed = seq_start.elapsed();
        log_result("Individual RPC", n * 3, seq_elapsed);
        rpc_remove_all_tracks(&daw).await?;

        // Batch: create + mutate in single call
        let batch_elapsed = batch_create_and_mutate(&daw, n).await?;
        log_result("Batch RPC (FromStep)", n * 3, batch_elapsed);

        let speedup = seq_elapsed.as_secs_f64() / batch_elapsed.as_secs_f64();
        info!("  Individual / Batch: {speedup:.1}x (batch speedup)");
        info!("");

        rpc_remove_all_tracks(&daw).await?;
    }

    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║          Benchmarks complete — see /tmp/daw-perf-test.log   ║");
    info!("╚══════════════════════════════════════════════════════════════╝");

    Ok(())
}
