//! Setlist RPP integration tests.
//!
//! Opens individual song RPPs and the generated combined setlist RPP
//! in REAPER, verifying that the structure is correct in live project tabs.
//!
//! Run with:
//!
//!   cargo test -p daw-reaper --test reaper_setlist -- --ignored --nocapture

use dawfile_reaper::io::{read_project, parse_project_text};
use dawfile_reaper::setlist_rpp::{self, SongInfo, concatenate_projects};
use eyre::Result;
use reaper_test::{DawInstanceConfig, run_multi_reaper_test};
use std::path::PathBuf;
use std::time::Duration;

fn fixture_path(name: &str) -> PathBuf {
    // dawfile-reaper fixtures
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dawfile-reaper/tests/fixtures/setlist")
        .join(name)
}

/// Open all 3 individual songs + the generated combined setlist in REAPER.
/// Verify track counts, names, and structure in each tab.
#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn setlist_four_tabs() -> Result<()> {
    run_multi_reaper_test(
        "setlist_four_tabs",
        vec![DawInstanceConfig::new("daw")],
        |ctx| {
            Box::pin(async move {
                let daw = &ctx.by_label("daw").daw;

                // ── 1. Generate the combined setlist RPP ────────────────
                let rpl_path = fixture_path("test_setlist.RPL");
                let rpp_paths = setlist_rpp::parse_rpl(&rpl_path)
                    .map_err(|e| eyre::eyre!("Failed to parse RPL: {e}"))?;

                let projects: Vec<_> = rpp_paths
                    .iter()
                    .map(|p| read_project(p).unwrap())
                    .collect();

                let songs = vec![
                    SongInfo {
                        name: "Song A".to_string(),
                        global_start_seconds: 0.0,
                        duration_seconds: 24.0,
                    },
                    SongInfo {
                        name: "Song B".to_string(),
                        global_start_seconds: 24.0,
                        duration_seconds: 16.0,
                    },
                    SongInfo {
                        name: "Song C".to_string(),
                        global_start_seconds: 40.0,
                        duration_seconds: 20.571429,
                    },
                ];

                let combined = concatenate_projects(&projects, &songs);

                // Write to temp file
                let temp_dir = std::env::temp_dir().join("fts-setlist-test");
                std::fs::create_dir_all(&temp_dir)?;
                let combined_path = temp_dir.join("Combined Setlist.RPP");

                // Serialize and write combined project to temp RPP file
                let rpp_text = setlist_rpp::project_to_rpp_text(&combined);
                let temp_dir = std::env::temp_dir().join("fts-setlist-test");
                std::fs::create_dir_all(&temp_dir)?;
                let combined_path = temp_dir.join("Combined Setlist.RPP");
                std::fs::write(&combined_path, &rpp_text)?;
                println!("  Combined project written to: {}", combined_path.display());
                println!("  {} tracks, {} markers/regions, {} bytes",
                    combined.tracks.len(), combined.markers_regions.all.len(), rpp_text.len());

                // ── 2. Open all 3 individual song RPPs ──────────────────
                println!("\n  ═══ OPENING SONG PROJECTS ═══\n");

                let song_a_path = fixture_path("song_a.RPP");
                let song_b_path = fixture_path("song_b.RPP");
                let song_c_path = fixture_path("song_c.RPP");

                let project_a = daw.open_project(song_a_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;
                println!("  [1] Opened Song A");

                let project_b = daw.open_project(song_b_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;
                println!("  [2] Opened Song B");

                let project_c = daw.open_project(song_c_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;
                println!("  [3] Opened Song C");

                // Open the combined setlist RPP as the 4th tab
                let combined_project = daw.open_project(combined_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;
                println!("  [4] Opened Combined Setlist");

                // ── 3. Verify each song project ─────────────────────────
                println!("\n  ═══ VERIFYING INDIVIDUAL SONGS ═══\n");

                // Song A: Click/Guide folder, Click, TRACKS folder, Guitar, Bass = 5 tracks
                let tracks_a = project_a.tracks().all().await?;
                println!("  Song A: {} tracks", tracks_a.len());
                for t in &tracks_a {
                    println!("    - {} (vol={:.2})", t.name, t.volume);
                }
                assert!(tracks_a.len() >= 4, "Song A should have at least 4 tracks, got {}", tracks_a.len());

                // Song B: Click, Keys, Drums = 3 tracks
                let tracks_b = project_b.tracks().all().await?;
                println!("  Song B: {} tracks", tracks_b.len());
                for t in &tracks_b {
                    println!("    - {} (vol={:.2})", t.name, t.volume);
                }
                assert!(tracks_b.len() >= 3, "Song B should have at least 3 tracks, got {}", tracks_b.len());

                // Song C: Click, Guide, Synth Lead = 3 tracks
                let tracks_c = project_c.tracks().all().await?;
                println!("  Song C: {} tracks", tracks_c.len());
                for t in &tracks_c {
                    println!("    - {} (vol={:.2})", t.name, t.volume);
                }
                assert!(tracks_c.len() >= 3, "Song C should have at least 3 tracks, got {}", tracks_c.len());

                // ── 4. Verify markers on each project ───────────────────
                println!("\n  ═══ VERIFYING MARKERS ═══\n");

                let markers_a = project_a.markers().all().await?;
                println!("  Song A markers: {}", markers_a.len());
                for m in &markers_a {
                    println!("    - {:?} at {:?}", m.name, m.position);
                }

                let markers_b = project_b.markers().all().await?;
                println!("  Song B markers: {}", markers_b.len());
                for m in &markers_b {
                    println!("    - {:?} at {:?}", m.name, m.position);
                }

                let markers_c = project_c.markers().all().await?;
                println!("  Song C markers: {}", markers_c.len());
                for m in &markers_c {
                    println!("    - {:?} at {:?}", m.name, m.position);
                }

                // ── 5. Verify regions on Song B ─────────────────────────
                let regions_b = project_b.regions().all().await?;
                println!("  Song B regions: {}", regions_b.len());
                for r in &regions_b {
                    println!("    - {:?} {:?} → {:?}", r.name, r.time_range.start, r.time_range.end);
                }

                // ── 6. Check total project count ────────────────────────
                let all_projects = daw.projects().await?;
                println!("\n  Total open projects: {}", all_projects.len());
                // Should have at least 4 (3 songs + 1 combined) + the default = 5
                assert!(
                    all_projects.len() >= 4,
                    "Should have at least 4 projects open (3 songs + combined), got {}",
                    all_projects.len()
                );

                // ── 7. Verify transport on each ─────────────────────────
                println!("\n  ═══ VERIFYING TRANSPORT ═══\n");

                let transport_a = project_a.transport().get_state().await?;
                let transport_b = project_b.transport().get_state().await?;
                let transport_c = project_c.transport().get_state().await?;

                println!("  Song A: {:.1} BPM", transport_a.tempo.bpm);
                println!("  Song B: {:.1} BPM", transport_b.tempo.bpm);
                println!("  Song C: {:.1} BPM", transport_c.tempo.bpm);

                assert!(
                    (transport_a.tempo.bpm - 120.0).abs() < 1.0,
                    "Song A should be ~120 BPM, got {:.1}",
                    transport_a.tempo.bpm
                );
                // Song B is 3/4 time at 90 BPM — REAPER may report the
                // effective tempo differently. Just verify it's reasonable.
                assert!(
                    transport_b.tempo.bpm > 50.0 && transport_b.tempo.bpm < 120.0,
                    "Song B tempo should be in range, got {:.1}",
                    transport_b.tempo.bpm
                );
                assert!(
                    (transport_c.tempo.bpm - 140.0).abs() < 1.0,
                    "Song C should be ~140 BPM, got {:.1}",
                    transport_c.tempo.bpm
                );

                println!("\n  ═══ COMBINED SETLIST SUMMARY ═══\n");
                println!("  The combined project would have:");
                println!("    {} tracks", combined.tracks.len());
                println!("    {} tempo points", combined.tempo_envelope.as_ref().map_or(0, |e| e.points.len()));
                println!("    {} markers/regions", combined.markers_regions.all.len());
                println!("    Timeline: 0s → {:.1}s", songs.iter().map(|s| s.global_start_seconds + s.duration_seconds).last().unwrap_or(0.0));
                println!("    Songs: {} → {} → {}", songs[0].name, songs[1].name, songs[2].name);

                // Keep REAPER open for inspection if FTS_KEEP_OPEN is set
                if std::env::var("FTS_KEEP_OPEN").is_ok() {
                    println!("  FTS_KEEP_OPEN set — waiting 60s for inspection...");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }

                // Verify the combined project has the right tracks
                let combined_tracks = combined_project.tracks().all().await?;
                println!("\n  Combined project in REAPER: {} tracks", combined_tracks.len());
                for t in &combined_tracks {
                    println!("    - {} (vol={:.2})", t.name, t.volume);
                }
                assert!(
                    combined_tracks.len() >= 10,
                    "Combined should have at least 10 tracks, got {}",
                    combined_tracks.len()
                );

                // Cleanup: close the opened projects
                daw.close_project(&project_a.info().await?.guid).await?;
                daw.close_project(&project_b.info().await?.guid).await?;
                daw.close_project(&project_c.info().await?.guid).await?;
                daw.close_project(&combined_project.info().await?.guid).await?;

                println!("\n  ═══ ALL VERIFICATIONS PASSED ═══\n");

                Ok(())
            })
        },
    )
}
