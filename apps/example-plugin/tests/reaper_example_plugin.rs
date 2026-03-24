//! Integration test: verify the example CLAP plugin loads in REAPER
//! and can access the REAPER API.
//!
//! Run with:
//!   cargo xtask reaper-test -- example_plugin

use std::time::Duration;

use reaper_test::reaper_test;

/// Names to try when loading the example plugin.
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
// Test: Load example plugin and verify it has parameters
// ---------------------------------------------------------------------------

#[reaper_test(isolated)]
async fn example_plugin_loads_on_track(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let track = project.tracks().add("Example Plugin Test", None).await?;

    ctx.log("Loading example plugin...");
    let fx = match add_example_plugin(&track).await? {
        Some(fx) => fx,
        None => {
            ctx.log("SKIP: Example plugin not available. Build and install it first.");
            return Ok(());
        }
    };

    let info = fx.info().await?;
    ctx.log(&format!("Loaded: {}", info.name));

    let params = fx.parameters().await?;
    ctx.log(&format!("{} parameters:", params.len()));
    for p in &params {
        ctx.log(&format!("  {} (idx {}): {:.4}", p.name, p.index, p.value));
    }

    assert!(
        !params.is_empty(),
        "Plugin should have at least one parameter"
    );

    // Set the gain parameter and read it back
    fx.param(0).set(0.75).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let val = fx.param(0).get().await?;
    assert!(
        (val - 0.75).abs() < 0.05,
        "Gain should be ~0.75, got {val:.4}"
    );

    ctx.log(&format!("Set gain to 0.75, read back {val:.4}"));
    ctx.log("=== PASS: Example plugin loaded and parameters work ===");
    Ok(())
}
