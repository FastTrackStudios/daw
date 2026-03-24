//! Integration test: verify a CLAP plugin can access the REAPER API.
//!
//! Loads the example-plugin onto a track, then checks that the plugin's
//! timer callback successfully called REAPER APIs (track count, ExtState).
//!
//! Run with:
//!   cargo xtask reaper-test -- example_plugin

use std::time::Duration;

use reaper_test::reaper_test;

const EXAMPLE_PLUGIN_NAMES: &[&str] = &[
    "CLAP: DAW Example Plugin (FastTrackStudio)",
    "CLAP: DAW Example Plugin",
    "DAW Example Plugin",
];

async fn add_example_plugin(track: &daw::TrackHandle) -> eyre::Result<Option<daw::FxHandle>> {
    for name in EXAMPLE_PLUGIN_NAMES {
        if let Ok(fx) = track.fx_chain().add(name).await {
            return Ok(Some(fx));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Test: Plugin loads, parameters work
// ---------------------------------------------------------------------------

#[reaper_test(isolated)]
async fn example_plugin_loads_on_track(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let track = project.tracks().add("Plugin Load Test", None).await?;

    ctx.log("Loading example plugin...");
    let fx = match add_example_plugin(&track).await? {
        Some(fx) => fx,
        None => {
            ctx.log("SKIP: Example plugin not available.");
            return Ok(());
        }
    };

    let info = fx.info().await?;
    ctx.log(&format!("Loaded: {}", info.name));

    // Set gain and read back
    fx.param(0).set(0.75).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let val = fx.param(0).get().await?;
    assert!(
        (val - 0.75).abs() < 0.05,
        "Gain should be ~0.75, got {val:.4}"
    );

    ctx.log("=== PASS: Plugin loaded, parameters work ===");
    Ok(())
}

// ---------------------------------------------------------------------------
// Test: Plugin's timer calls REAPER API and writes to ExtState
// ---------------------------------------------------------------------------

#[reaper_test(isolated)]
async fn example_plugin_calls_reaper_api(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    // Add a few tracks so the plugin has something to count
    project.tracks().add("Track A", None).await?;
    project.tracks().add("Track B", None).await?;
    let track_c = project.tracks().add("Track C", None).await?;

    // Load the plugin on Track C
    ctx.log("Loading example plugin...");
    let _fx = match add_example_plugin(&track_c).await? {
        Some(fx) => fx,
        None => {
            ctx.log("SKIP: Example plugin not available.");
            return Ok(());
        }
    };

    // Wait for the plugin's timer to fire and write to ExtState
    // Timer runs at ~30Hz, so 1 second should be plenty
    ctx.log("Waiting for plugin timer to write ExtState...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Read back what the plugin wrote via REAPER API
    let ext = ctx.daw.ext_state();
    let tick = ext.get("FTS_EXAMPLE_PLUGIN", "tick").await?;
    let track_count = ext.get("FTS_EXAMPLE_PLUGIN", "track_count").await?;

    ctx.log(&format!("Plugin ExtState tick: {:?}", tick));
    ctx.log(&format!("Plugin ExtState track_count: {:?}", track_count));

    // Verify the timer fired (tick > 0)
    let tick_val: u32 = tick.as_deref().unwrap_or("0").parse().unwrap_or(0);
    assert!(
        tick_val > 0,
        "Timer should have fired at least once (tick={tick_val})"
    );
    ctx.log(&format!("Timer fired {tick_val} times"));

    // Verify track count is correct (we added 3 tracks)
    let count: u32 = track_count.as_deref().unwrap_or("0").parse().unwrap_or(0);
    assert!(
        count >= 3,
        "Plugin should see at least 3 tracks, got {count}"
    );
    ctx.log(&format!("Plugin counted {count} tracks (expected >= 3)"));

    ctx.log("=== PASS: CLAP plugin successfully called REAPER API from timer ===");
    Ok(())
}
