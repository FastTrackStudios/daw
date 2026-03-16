//! Integration tests for event-driven auto-color.
//!
//! These tests require the FTS REAPER extension to be loaded with the
//! AutoColorSurface control surface registered. Run with:
//!
//!   cargo xtask reaper-test auto_color

use reaper_test::{reaper_test, ReaperTestContext};
use std::time::Duration;
use tokio::time::sleep;

/// Helper: settle time for REAPER to process changes and CSurf to fire.
/// The CSurf run() cycle is ~33ms, but we need a bit more for track creation
/// RPCs to round-trip and the recolor to apply.
const SETTLE: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Explicit color-all command
// ---------------------------------------------------------------------------

/// Create tracks with known instrument names, run color_all, verify each
/// track gets a non-default color assigned.
#[reaper_test(isolated)]
async fn auto_color_all_applies_colors(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    // Well-known instrument names that dynamic-template should classify
    let names = [
        "Kick In",
        "Snare Top",
        "OH L",
        "Bass DI",
        "Electric Guitar",
        "Lead Vocal",
        "Piano",
        "Synth Pad",
    ];

    for name in &names {
        project.tracks().add(name, None).await?;
    }
    sleep(Duration::from_millis(300)).await;

    // Run the explicit color-all action
    let ok = project.run_command("FTS_AUTO_COLOR_AUTO_COLOR_ALL_TRACKS").await?;
    assert!(ok, "FTS_AUTO_COLOR_AUTO_COLOR_ALL_TRACKS command should be found");
    sleep(SETTLE).await;

    // Verify every track got a color
    let tracks = project.tracks().all().await?;
    assert_eq!(tracks.len(), names.len(), "should have all tracks");

    let mut colored_count = 0;
    for track in &tracks {
        if track.color.is_some() {
            colored_count += 1;
            println!(
                "  [{}] {:?} → color=0x{:06X}",
                track.index,
                track.name,
                track.color.unwrap()
            );
        } else {
            println!("  [{}] {:?} → NO COLOR", track.index, track.name);
        }
    }

    assert!(
        colored_count >= names.len() - 1,
        "at least {} of {} tracks should be colored, got {}",
        names.len() - 1,
        names.len(),
        colored_count,
    );

    // Distinct track types should get distinct colors
    let unique_colors: std::collections::HashSet<u32> = tracks
        .iter()
        .filter_map(|t| t.color)
        .collect();
    assert!(
        unique_colors.len() >= 3,
        "should have at least 3 distinct colors across {} instrument groups, got {}",
        names.len(),
        unique_colors.len(),
    );
    println!(
        "\n{} distinct colors across {} tracks",
        unique_colors.len(),
        tracks.len()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Clear all restores default colors
// ---------------------------------------------------------------------------

/// After coloring, clear_all should remove all custom colors.
#[reaper_test(isolated)]
async fn auto_color_clear_all_removes_colors(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    project.tracks().add("Kick In", None).await?;
    project.tracks().add("Snare Top", None).await?;
    project.tracks().add("Bass DI", None).await?;
    sleep(Duration::from_millis(200)).await;

    // Apply colors first
    project.run_command("FTS_AUTO_COLOR_AUTO_COLOR_ALL_TRACKS").await?;
    sleep(SETTLE).await;

    let before = project.tracks().all().await?;
    let colored_before = before.iter().filter(|t| t.color.is_some()).count();
    assert!(colored_before > 0, "tracks should be colored before clear");

    // Clear all
    project.run_command("FTS_AUTO_COLOR_CLEAR_ALL_TRACK_COLORS").await?;
    sleep(SETTLE).await;

    let after = project.tracks().all().await?;
    let colored_after = after.iter().filter(|t| t.color.is_some()).count();
    assert_eq!(
        colored_after, 0,
        "all colors should be cleared after clear_all"
    );
    println!("Clear all: {} colored → {} colored", colored_before, colored_after);

    Ok(())
}

// ---------------------------------------------------------------------------
// Toggle enables event-driven auto-color
// ---------------------------------------------------------------------------

/// Toggle on, then add a new track — it should get colored automatically
/// via the CSurf SetTrackListChange callback (no explicit color command).
#[reaper_test(isolated)]
async fn auto_color_toggle_colors_new_tracks(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    // Start with a track so toggle has something to color initially
    project.tracks().add("Kick In", None).await?;
    sleep(Duration::from_millis(200)).await;

    // Toggle auto-color ON
    let ok = project.run_command("FTS_AUTO_COLOR_TOGGLE_AUTO_COLOR").await?;
    assert!(ok, "toggle command should be found");
    sleep(SETTLE).await;

    // Verify initial track is colored
    let tracks = project.tracks().all().await?;
    assert!(
        tracks[0].color.is_some(),
        "Kick In should be colored after toggle on"
    );
    println!("After toggle ON: Kick In → 0x{:06X}", tracks[0].color.unwrap());

    // Add a new track — CSurf should auto-color it via set_track_list_change
    project.tracks().add("Lead Vocal", None).await?;
    // Give the CSurf run() cycle time to fire (needs ~33ms, we give more for safety)
    sleep(SETTLE).await;

    let tracks = project.tracks().all().await?;
    let vocal_track = tracks.iter().find(|t| t.name == "Lead Vocal");
    assert!(vocal_track.is_some(), "Lead Vocal track should exist");
    let vocal_track = vocal_track.unwrap();
    assert!(
        vocal_track.color.is_some(),
        "Lead Vocal should be auto-colored after being added (CSurf event-driven)"
    );
    println!(
        "New track auto-colored: Lead Vocal → 0x{:06X}",
        vocal_track.color.unwrap()
    );

    // Toggle auto-color OFF (clears colors)
    project.run_command("FTS_AUTO_COLOR_TOGGLE_AUTO_COLOR").await?;
    sleep(SETTLE).await;

    let tracks = project.tracks().all().await?;
    let still_colored = tracks.iter().filter(|t| t.color.is_some()).count();
    assert_eq!(
        still_colored, 0,
        "all colors should be cleared after toggle off"
    );
    println!("After toggle OFF: all colors cleared");

    Ok(())
}

// ---------------------------------------------------------------------------
// Rename triggers recolor via CSurf SetTrackTitle
// ---------------------------------------------------------------------------

/// With auto-color toggled on, renaming a track should trigger a recolor
/// via the CSurf set_track_title callback.
#[reaper_test(isolated)]
async fn auto_color_recolors_on_rename(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    // Create a track with one instrument name
    let track = project.tracks().add("Kick In", None).await?;
    sleep(Duration::from_millis(200)).await;

    // Toggle auto-color ON
    project.run_command("FTS_AUTO_COLOR_TOGGLE_AUTO_COLOR").await?;
    sleep(SETTLE).await;

    let tracks = project.tracks().all().await?;
    let kick_color = tracks[0].color;
    assert!(kick_color.is_some(), "Kick In should be colored");
    println!("Before rename: Kick In → 0x{:06X}", kick_color.unwrap());

    // Rename to a completely different instrument group
    track.rename("Lead Vocal").await?;
    // CSurf set_track_title should fire → mark dirty → run() recolors
    sleep(SETTLE).await;

    let tracks = project.tracks().all().await?;
    let new_color = tracks[0].color;
    assert!(new_color.is_some(), "renamed track should still be colored");
    println!("After rename: Lead Vocal → 0x{:06X}", new_color.unwrap());

    // The color should have changed since it's a different instrument group
    assert_ne!(
        kick_color, new_color,
        "color should change when track is renamed from Kick In to Lead Vocal"
    );

    // Clean up: toggle off
    project.run_command("FTS_AUTO_COLOR_TOGGLE_AUTO_COLOR").await?;
    sleep(SETTLE).await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Color selected only colors selected tracks
// ---------------------------------------------------------------------------

#[reaper_test(isolated)]
async fn auto_color_selected_only(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    let _kick = project.tracks().add("Kick In", None).await?;
    let snare = project.tracks().add("Snare Top", None).await?;
    let _bass = project.tracks().add("Bass DI", None).await?;
    sleep(Duration::from_millis(200)).await;

    // Select only the snare track (deselects all others)
    snare.select_exclusive().await?;
    sleep(Duration::from_millis(100)).await;

    // Color selected only
    project.run_command("FTS_AUTO_COLOR_AUTO_COLOR_SELECTED").await?;
    sleep(SETTLE).await;

    let tracks = project.tracks().all().await?;
    for t in &tracks {
        println!(
            "  [{}] {:?} → color={:?}",
            t.index, t.name, t.color
        );
    }

    let snare_track = tracks.iter().find(|t| t.name == "Snare Top").unwrap();
    assert!(
        snare_track.color.is_some(),
        "selected track (Snare Top) should be colored"
    );

    // Non-selected tracks should NOT be colored
    let non_selected_colored = tracks
        .iter()
        .filter(|t| t.name != "Snare Top" && t.color.is_some())
        .count();
    assert_eq!(
        non_selected_colored, 0,
        "non-selected tracks should not be colored by color_selected"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

#[reaper_test(isolated)]
async fn auto_color_cleanup(ctx: &ReaperTestContext) -> eyre::Result<()> {
    // Make sure auto-color is off so we don't interfere with other tests
    // Toggle twice to ensure it's off (idempotent)
    project_cleanup(ctx).await?;
    println!("auto_color tests cleanup: PASS");
    Ok(())
}

async fn project_cleanup(ctx: &ReaperTestContext) -> eyre::Result<()> {
    reaper_test::cleanup_all_projects(&ctx.daw).await?;
    Ok(())
}
