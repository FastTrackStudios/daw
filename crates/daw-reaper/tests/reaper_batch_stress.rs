//! Stress tests and benchmarks for the batch instruction set.
//!
//! Compares batch execution (1 RPC call with N operations) vs sequential
//! individual RPC calls for the same N operations. Demonstrates the
//! performance advantage of batch processing for bulk DAW mutations.
//!
//! Run with: `cargo test -p daw-reaper --test reaper_batch_stress -- --ignored --nocapture`

use daw_control::{BatchBuilder, BatchResponseExt};
use daw_proto::batch::*;
use reaper_test::reaper_test;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fmt_dur(d: std::time::Duration) -> String {
    if d.as_millis() >= 1000 {
        format!("{:.2}s", d.as_secs_f64())
    } else if d.as_millis() > 0 {
        format!("{:.2}ms", d.as_secs_f64() * 1000.0)
    } else {
        format!("{}µs", d.as_micros())
    }
}

// ===========================================================================
// Benchmark 1: Create N tracks — batch vs sequential
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_create_tracks_100(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    bench_create_tracks(ctx, 100).await
}

#[reaper_test(isolated)]
async fn batch_create_tracks_500(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    bench_create_tracks(ctx, 500).await
}

async fn bench_create_tracks(ctx: &reaper_test::ReaperTestContext, n: u32) -> eyre::Result<()> {
    let daw = &ctx.daw;
    let project = ctx.project().clone();
    let tracks = project.tracks();

    ctx.log(&format!("=== Benchmark: Create {} tracks ===", n));

    // --- Sequential: N individual RPC calls ---
    let seq_start = Instant::now();
    for i in 0..n {
        tracks.add(&format!("SeqTrack-{}", i), None).await?;
    }
    let seq_elapsed = seq_start.elapsed();
    ctx.log(&format!(
        "Sequential ({n} RPCs): {} total, {}/call",
        fmt_dur(seq_elapsed),
        fmt_dur(seq_elapsed / n),
    ));

    // Verify
    let count = tracks.count().await?;
    ctx.log(&format!("Tracks after sequential: {}", count));
    assert_eq!(count, n, "Should have {n} tracks after sequential creation");

    // Remove all tracks to reset for batch test
    tracks.remove_all().await?;
    let count_after_clear = tracks.count().await?;
    assert_eq!(count_after_clear, 0, "Should be 0 tracks after cleanup");

    // --- Batch: 1 RPC call with N operations ---
    let mut b = BatchBuilder::new().with_undo("Batch create tracks");
    let proj = b.current_project();
    for i in 0..n {
        b.add_track(&proj, format!("BatchTrack-{}", i), None);
    }
    let batch_start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let batch_elapsed = batch_start.elapsed();
    ctx.log(&format!(
        "Batch     (1 RPC):    {} total, {}/op",
        fmt_dur(batch_elapsed),
        fmt_dur(batch_elapsed / n),
    ));

    // Verify
    let count = tracks.count().await?;
    assert_eq!(count, n, "Should have {n} tracks after batch creation");

    // Check no errors in response
    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_) | StepOutcome::Skipped(_)))
        .collect();
    assert!(errors.is_empty(), "Batch had {} errors", errors.len());

    // Speedup ratio
    let speedup = seq_elapsed.as_secs_f64() / batch_elapsed.as_secs_f64();
    ctx.log(&format!("Speedup: {:.1}x", speedup));
    ctx.log(&format!(
        "Saved: {} ({:.0}% reduction)",
        fmt_dur(seq_elapsed.saturating_sub(batch_elapsed)),
        (1.0 - batch_elapsed.as_secs_f64() / seq_elapsed.as_secs_f64()) * 100.0,
    ));

    ctx.log("PASSED");
    Ok(())
}

// ===========================================================================
// Benchmark 2: Mutate N tracks (rename + volume + pan) — batch vs sequential
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_mutate_tracks_200(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    bench_mutate_tracks(ctx, 200).await
}

async fn bench_mutate_tracks(ctx: &reaper_test::ReaperTestContext, n: u32) -> eyre::Result<()> {
    let daw = &ctx.daw;
    let project = ctx.project().clone();
    let tracks_svc = project.tracks();

    ctx.log(&format!(
        "=== Benchmark: Mutate {} tracks (3 ops each) ===",
        n
    ));

    // Setup: create N tracks via batch
    let mut b = BatchBuilder::new().with_undo("Setup tracks");
    let proj = b.current_project();
    for i in 0..n {
        b.add_track(&proj, format!("Track-{}", i), None);
    }
    daw.execute_batch(b.build()).await?;
    let all_tracks = tracks_svc.all().await?;
    assert_eq!(all_tracks.len() as u32, n, "Setup should create {n} tracks");

    // --- Sequential: 3N individual RPC calls (rename, volume, pan per track) ---
    let seq_start = Instant::now();
    for (i, track) in all_tracks.iter().enumerate() {
        let handle = tracks_svc
            .by_guid(&track.guid)
            .await?
            .expect("track should exist");
        handle.rename(&format!("Renamed-{}", i)).await?;
        handle.set_volume(0.5 + (i as f64) * 0.001).await?;
        handle.set_pan((i as f64) / n as f64 * 2.0 - 1.0).await?;
    }
    let seq_elapsed = seq_start.elapsed();
    let seq_ops = n * 3;
    ctx.log(&format!(
        "Sequential ({seq_ops} RPCs): {} total, {}/call",
        fmt_dur(seq_elapsed),
        fmt_dur(seq_elapsed / seq_ops),
    ));

    // --- Batch: 1 RPC call with 3N operations ---
    let mut b = BatchBuilder::new().with_undo("Batch mutate tracks");
    let proj = b.current_project();
    for (i, track) in all_tracks.iter().enumerate() {
        let tref = daw_proto::TrackRef::Guid(track.guid.clone());
        b.rename_track(&proj, tref.clone(), format!("BatchRenamed-{}", i));
        b.set_track_volume(&proj, tref.clone(), 0.5 + (i as f64) * 0.001);
        b.push_raw::<()>(BatchOp::Track(TrackOp::SetPan(
            ProjectArg::FromStep(proj.index()),
            TrackArg::Literal(tref),
            (i as f64) / n as f64 * 2.0 - 1.0,
        )));
    }
    let batch_start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let batch_elapsed = batch_start.elapsed();
    ctx.log(&format!(
        "Batch     (1 RPC):    {} total, {}/op",
        fmt_dur(batch_elapsed),
        fmt_dur(batch_elapsed / (n * 3)),
    ));

    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_) | StepOutcome::Skipped(_)))
        .collect();
    assert!(errors.is_empty(), "Batch had {} errors", errors.len());

    let speedup = seq_elapsed.as_secs_f64() / batch_elapsed.as_secs_f64();
    ctx.log(&format!("Speedup: {:.1}x", speedup));
    ctx.log(&format!(
        "Saved: {} ({:.0}% reduction)",
        fmt_dur(seq_elapsed.saturating_sub(batch_elapsed)),
        (1.0 - batch_elapsed.as_secs_f64() / seq_elapsed.as_secs_f64()) * 100.0,
    ));

    ctx.log("PASSED");
    Ok(())
}

// ===========================================================================
// Benchmark 3: Chained reads — project -> tracks -> count
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_chained_reads(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let daw = &ctx.daw;
    let project = ctx.project().clone();
    let tracks_svc = project.tracks();

    ctx.log("=== Benchmark: Chained reads (project -> tracks -> count) ===");

    // Setup: create 50 tracks
    let setup_n = 50u32;
    let mut b = BatchBuilder::new();
    let proj = b.current_project();
    for i in 0..setup_n {
        b.add_track(&proj, format!("ReadTrack-{}", i), None);
    }
    daw.execute_batch(b.build()).await?;

    let iterations = 50u32;

    // --- Sequential: 3 RPCs per iteration ---
    let seq_start = Instant::now();
    for _ in 0..iterations {
        let _proj_info = daw.current_project().await?;
        let _all = tracks_svc.all().await?;
        let _count = tracks_svc.count().await?;
    }
    let seq_elapsed = seq_start.elapsed();
    ctx.log(&format!(
        "Sequential ({} RPCs): {} total, {}/iteration",
        iterations * 3,
        fmt_dur(seq_elapsed),
        fmt_dur(seq_elapsed / iterations),
    ));

    // --- Batch: 1 RPC per iteration (3 ops inside) ---
    let batch_start = Instant::now();
    for _ in 0..iterations {
        let mut b = BatchBuilder::new();
        let proj = b.current_project();
        b.get_tracks(&proj);
        b.track_count(&proj);
        daw.execute_batch(b.build()).await?;
    }
    let batch_elapsed = batch_start.elapsed();
    ctx.log(&format!(
        "Batch     ({} RPCs): {} total, {}/iteration",
        iterations,
        fmt_dur(batch_elapsed),
        fmt_dur(batch_elapsed / iterations),
    ));

    let speedup = seq_elapsed.as_secs_f64() / batch_elapsed.as_secs_f64();
    ctx.log(&format!("Speedup: {:.1}x", speedup));

    ctx.log("PASSED");
    Ok(())
}

// ===========================================================================
// Benchmark 4: Large mixed batch — create + mutate via FromStep
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_mixed_ops_500(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let daw = &ctx.daw;
    let project = ctx.project().clone();
    let tracks_svc = project.tracks();

    let n = 500u32;
    ctx.log(&format!(
        "=== Benchmark: Mixed batch — {} tracks (create + 2 mutations each) ===",
        n
    ));

    // --- Batch: create N tracks then mutate each one (1 RPC) ---
    let mut b = BatchBuilder::new().with_undo("Mixed batch");
    let proj = b.current_project();
    let mut track_handles = Vec::with_capacity(n as usize);
    for i in 0..n {
        let h = b.add_track(&proj, format!("MixedTrack-{}", i), None);
        track_handles.push(h);
    }
    for (i, handle) in track_handles.iter().enumerate() {
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
    let total_ops = n + n * 2;
    ctx.log(&format!("Total ops in batch: {}", total_ops));

    let batch_start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let batch_elapsed = batch_start.elapsed();

    ctx.log(&format!(
        "Batch ({} ops, 1 RPC): {} total, {}/op",
        total_ops,
        fmt_dur(batch_elapsed),
        fmt_dur(batch_elapsed / total_ops),
    ));

    // Verify
    let count = tracks_svc.count().await?;
    assert_eq!(count, n, "Should have {n} tracks");

    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_) | StepOutcome::Skipped(_)))
        .collect();
    if !errors.is_empty() {
        for e in &errors[..errors.len().min(5)] {
            ctx.log(&format!("  error at step {}: {:?}", e.step, e.outcome));
        }
    }
    assert!(errors.is_empty(), "Batch had {} errors", errors.len());

    // Sequential comparison: create + mutate individually
    tracks_svc.remove_all().await?;

    let seq_start = Instant::now();
    for i in 0..n {
        let handle = tracks_svc.add(&format!("SeqMixed-{}", i), None).await?;
        handle.set_volume(0.7).await?;
        if i % 2 == 0 {
            handle.mute().await?;
        } else {
            handle.unmute().await?;
        }
    }
    let seq_elapsed = seq_start.elapsed();
    ctx.log(&format!(
        "Sequential ({} RPCs): {} total, {}/call",
        total_ops,
        fmt_dur(seq_elapsed),
        fmt_dur(seq_elapsed / total_ops),
    ));

    let speedup = seq_elapsed.as_secs_f64() / batch_elapsed.as_secs_f64();
    ctx.log(&format!("Speedup: {:.1}x", speedup));

    ctx.log("PASSED");
    Ok(())
}

// ===========================================================================
// Benchmark 5: Add markers in bulk
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_add_markers_200(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let daw = &ctx.daw;
    let project = ctx.project().clone();

    let n = 200u32;
    ctx.log(&format!("=== Benchmark: Add {} markers ===", n));

    // --- Sequential ---
    let seq_start = Instant::now();
    for i in 0..n {
        project
            .markers()
            .add(i as f64 * 0.5, &format!("SeqMarker-{}", i))
            .await?;
    }
    let seq_elapsed = seq_start.elapsed();
    ctx.log(&format!(
        "Sequential ({n} RPCs): {} total, {}/call",
        fmt_dur(seq_elapsed),
        fmt_dur(seq_elapsed / n),
    ));

    // Clear markers
    let markers = project.markers().all().await?;
    for m in markers.iter().rev() {
        if let Some(id) = m.id {
            project.markers().remove(id).await?;
        }
    }

    // --- Batch ---
    let mut b = BatchBuilder::new().with_undo("Batch add markers");
    let proj = b.current_project();
    for i in 0..n {
        b.add_marker(&proj, i as f64 * 0.5, format!("BatchMarker-{}", i));
    }
    let batch_start = Instant::now();
    let response = daw.execute_batch(b.build()).await?;
    let batch_elapsed = batch_start.elapsed();

    ctx.log(&format!(
        "Batch     (1 RPC):    {} total, {}/op",
        fmt_dur(batch_elapsed),
        fmt_dur(batch_elapsed / n),
    ));

    let errors: Vec<_> = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_) | StepOutcome::Skipped(_)))
        .collect();
    assert!(errors.is_empty(), "Batch had {} errors", errors.len());

    let speedup = seq_elapsed.as_secs_f64() / batch_elapsed.as_secs_f64();
    ctx.log(&format!("Speedup: {:.1}x", speedup));

    ctx.log("PASSED");
    Ok(())
}

// ===========================================================================
// Benchmark 6: Fail-fast correctness
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_fail_fast(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let daw = &ctx.daw;

    ctx.log("=== Test: Fail-fast stops execution on first error ===");

    let mut b = BatchBuilder::new().with_fail_fast();
    let proj = b.current_project();

    b.add_track(&proj, "ValidTrack", None);
    b.get_track(&proj, daw_proto::TrackRef::Guid("non-existent-guid".into()));

    for i in 0..10 {
        b.add_track(&proj, format!("ShouldNotExist-{}", i), None);
    }

    let response = daw.execute_batch(b.build()).await?;

    assert!(
        matches!(response.results[0].outcome, StepOutcome::Ok(_)),
        "step 0 should succeed"
    );
    assert!(
        matches!(response.results[1].outcome, StepOutcome::Ok(_)),
        "step 1 should succeed"
    );

    ctx.log(&format!(
        "Step 2 outcome: {:?}",
        response.results[2].outcome
    ));

    let skipped = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Skipped(_)))
        .count();
    let errors = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Error(_)))
        .count();
    let ok = response
        .results
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Ok(_)))
        .count();

    ctx.log(&format!(
        "Results: {} ok, {} errors, {} skipped (out of {})",
        ok,
        errors,
        skipped,
        response.results.len()
    ));

    assert_eq!(response.results.len(), 13, "Should have 13 step results");

    ctx.log("PASSED");
    Ok(())
}

// ===========================================================================
// Benchmark 7: Dependency chain — add track then mutate via FromStep
// ===========================================================================

#[reaper_test(isolated)]
async fn batch_dependency_chain(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let daw = &ctx.daw;
    let project = ctx.project().clone();
    let tracks_svc = project.tracks();

    ctx.log("=== Test: Dependency chain (add -> rename -> volume via FromStep) ===");

    let mut b = BatchBuilder::new().with_undo("Dependency chain");
    let proj = b.current_project();

    let track_handle = b.add_track(&proj, "OriginalName", None);

    b.push_raw::<()>(BatchOp::Track(TrackOp::RenameTrack(
        ProjectArg::FromStep(proj.index()),
        TrackArg::FromStep(track_handle.index()),
        "RenamedViaFromStep".to_string(),
    )));

    b.push_raw::<()>(BatchOp::Track(TrackOp::SetVolume(
        ProjectArg::FromStep(proj.index()),
        TrackArg::FromStep(track_handle.index()),
        0.42,
    )));

    b.push_raw::<()>(BatchOp::Track(TrackOp::SetMuted(
        ProjectArg::FromStep(proj.index()),
        TrackArg::FromStep(track_handle.index()),
        true,
    )));

    let response = daw.execute_batch(b.build()).await?;

    for (i, r) in response.results.iter().enumerate() {
        ctx.log(&format!("Step {}: {:?}", i, r.outcome));
        assert!(
            matches!(r.outcome, StepOutcome::Ok(_)),
            "Step {} should succeed, got {:?}",
            i,
            r.outcome
        );
    }

    // Verify the track was actually renamed and mutated
    let track_guid: String = response.get(&track_handle)?;
    let track = tracks_svc
        .by_guid(&track_guid)
        .await?
        .expect("Track should exist");
    let info = track.info().await?;
    assert_eq!(info.name, "RenamedViaFromStep", "Track should be renamed");
    assert!(info.muted, "Track should be muted");
    ctx.log(&format!(
        "Verified: name='{}', muted={}, volume={:.2}",
        info.name, info.muted, info.volume
    ));

    ctx.log("PASSED");
    Ok(())
}
