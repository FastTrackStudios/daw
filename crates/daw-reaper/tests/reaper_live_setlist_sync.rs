//! Live setlist sync integration test.
//!
//! Opens 3 individual song projects + the combined setlist in a single REAPER
//! instance, then edits one of the songs (add marker, move region) and manually
//! replicates to the setlist tab with offset — verifying the same operations
//! that DawSyncBridge performs.
//!
//! Run with:
//!   cargo test -p daw-reaper --test reaper_live_setlist_sync -- --ignored --nocapture

use dawfile_reaper::io::read_project;
use dawfile_reaper::setlist_rpp::{
    self, build_song_infos_from_projects, concatenate_projects, measures_to_seconds,
    project_to_rpp_text,
};
use eyre::Result;
use reaper_test::{DawInstanceConfig, run_multi_reaper_test};
use std::path::PathBuf;
use std::time::Duration;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dawfile-reaper/tests/fixtures/setlist")
        .join(name)
}

/// Test: add a marker in Song B, manually replicate to setlist with offset,
/// verify it lands at the correct position.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn live_sync_marker_propagates_with_offset() -> Result<()> {
    run_multi_reaper_test(
        "live_sync_marker",
        vec![DawInstanceConfig::new("daw")],
        |ctx| {
            Box::pin(async move {
                let daw = &ctx.by_label("daw").daw;

                // ── Setup ───────────────────────────────────────────────────
                let parsed_a = read_project(fixture_path("song_a.RPP"))?;
                let parsed_b = read_project(fixture_path("song_b.RPP"))?;
                let parsed_c = read_project(fixture_path("song_c.RPP"))?;

                let names = vec!["Song A", "Song B", "Song C"];
                let gap = measures_to_seconds(2, 120.0, 4);
                let song_infos = build_song_infos_from_projects(
                    &[parsed_a.clone(), parsed_b.clone(), parsed_c.clone()],
                    &names,
                    gap,
                );
                let combined = concatenate_projects(
                    &[parsed_a, parsed_b, parsed_c],
                    &song_infos,
                );
                let rpp_text = project_to_rpp_text(&combined);

                let temp_dir = std::env::temp_dir().join("fts-live-sync-test");
                std::fs::create_dir_all(&temp_dir)?;
                let combined_path = temp_dir.join("Live Sync Test.RPP");
                std::fs::write(&combined_path, &rpp_text)?;

                // Open all tabs
                let song_b = daw.open_project(fixture_path("song_b.RPP").to_string_lossy()).await?;
                tokio::time::sleep(Duration::from_millis(300)).await;
                let setlist = daw.open_project(combined_path.to_string_lossy()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                let song_b_offset = song_infos[1].global_start_seconds;
                println!("\n  ═══ LIVE SYNC TEST ═══");
                println!("  Song B offset in setlist: {:.1}s\n", song_b_offset);

                // ── Test 1: Add marker in Song B → replicate to setlist ─────
                println!("  ── Test 1: Marker add + offset replication ──");
                let marker_pos_in_song = 5.0;
                let expected_setlist_pos = marker_pos_in_song + song_b_offset;

                // Add marker in Song B
                let marker_id = song_b.markers().add(marker_pos_in_song, "SYNC TEST").await?;
                println!("  Added marker 'SYNC TEST' at {:.1}s in Song B (id={})", marker_pos_in_song, marker_id);

                // Replicate to setlist with offset (this is what DawSyncBridge does)
                let setlist_marker_id = setlist.markers().add(expected_setlist_pos, "SYNC TEST").await?;
                println!("  Replicated to setlist at {:.1}s (id={})", expected_setlist_pos, setlist_marker_id);

                tokio::time::sleep(Duration::from_millis(200)).await;

                // Verify
                let setlist_markers = setlist.markers().all().await?;
                let synced = setlist_markers.iter().find(|m| m.name == "SYNC TEST");
                assert!(synced.is_some(), "SYNC TEST marker should exist in setlist");

                let pos = synced.unwrap().position.time.as_ref()
                    .map(|t| t.as_seconds())
                    .unwrap_or(0.0);
                let drift = (pos - expected_setlist_pos).abs();
                println!("  Verified: setlist marker at {:.3}s (expected {:.1}s, drift {:.3}s) {}",
                    pos, expected_setlist_pos, drift,
                    if drift < 0.1 { "✓" } else { "✗" });
                assert!(drift < 0.1, "Marker drift {:.3}s exceeds tolerance", drift);

                // ── Test 2: Move marker in Song B → update in setlist ────────
                println!("\n  ── Test 2: Marker move + offset update ──");
                let new_pos_in_song = 8.0;
                let new_expected_setlist_pos = new_pos_in_song + song_b_offset;

                song_b.markers().move_to(marker_id, new_pos_in_song).await?;
                println!("  Moved Song B marker to {:.1}s", new_pos_in_song);

                // Replicate the move to setlist
                setlist.markers().move_to(setlist_marker_id, new_expected_setlist_pos).await?;
                println!("  Replicated move to setlist at {:.1}s", new_expected_setlist_pos);

                tokio::time::sleep(Duration::from_millis(200)).await;

                let updated_markers = setlist.markers().all().await?;
                let moved = updated_markers.iter().find(|m| m.name == "SYNC TEST");
                let moved_pos = moved.unwrap().position.time.as_ref()
                    .map(|t| t.as_seconds())
                    .unwrap_or(0.0);
                let drift = (moved_pos - new_expected_setlist_pos).abs();
                println!("  Verified: moved marker at {:.3}s (expected {:.1}s, drift {:.3}s) {}",
                    moved_pos, new_expected_setlist_pos, drift,
                    if drift < 0.1 { "✓" } else { "✗" });
                assert!(drift < 0.1, "Moved marker drift {:.3}s", drift);

                // ── Test 3: Add region in Song B → replicate to setlist ──────
                println!("\n  ── Test 3: Region add + offset replication ──");
                let region_start = 2.0;
                let region_end = 6.0;
                let region_id = song_b.regions().add(region_start, region_end, "SYNC REGION").await?;
                println!("  Added region 'SYNC REGION' {:.1}→{:.1}s in Song B (id={})", region_start, region_end, region_id);

                let setlist_region_start = region_start + song_b_offset;
                let setlist_region_end = region_end + song_b_offset;
                let setlist_region_id = setlist.regions().add(setlist_region_start, setlist_region_end, "SYNC REGION").await?;
                println!("  Replicated to setlist {:.1}→{:.1}s (id={})", setlist_region_start, setlist_region_end, setlist_region_id);

                tokio::time::sleep(Duration::from_millis(200)).await;

                let setlist_regions = setlist.regions().all().await?;
                let synced_region = setlist_regions.iter().find(|r| r.name == "SYNC REGION");
                assert!(synced_region.is_some(), "SYNC REGION should exist in setlist");
                let sr = synced_region.unwrap();
                let sr_start = sr.time_range.start_seconds();
                let sr_drift = (sr_start - setlist_region_start).abs();
                println!("  Verified: region at {:.1}→{:.1}s (expected {:.1}→{:.1}s, drift {:.3}s) {}",
                    sr_start, sr.time_range.end_seconds(),
                    setlist_region_start, setlist_region_end, sr_drift,
                    if sr_drift < 0.1 { "✓" } else { "✗" });
                assert!(sr_drift < 0.1, "Region drift {:.3}s", sr_drift);

                // ── Test 4: Remove marker from Song B → remove from setlist ──
                println!("\n  ── Test 4: Marker remove replication ──");
                song_b.markers().remove(marker_id).await?;
                setlist.markers().remove(setlist_marker_id).await?;
                println!("  Removed 'SYNC TEST' from both Song B and setlist");

                tokio::time::sleep(Duration::from_millis(200)).await;

                let final_markers = setlist.markers().all().await?;
                let still_exists = final_markers.iter().any(|m| m.name == "SYNC TEST");
                assert!(!still_exists, "SYNC TEST should be removed from setlist");
                println!("  Verified: marker removed from setlist ✓");

                println!("\n  ═══ LIVE SYNC TEST COMPLETE ═══\n");
                Ok(())
            })
        },
    )
}
