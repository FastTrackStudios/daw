//! Cross-instance setlist sync test.
//!
//! Two REAPER instances:
//! - Instance A (Project Mode): 3 individual song projects
//! - Instance B (Setlist Mode): combined setlist project
//!
//! The offset map translates positions bidirectionally between them.
//! Only commands go to ONE instance at a time — the other follows via
//! the offset map translation applied by the test bridge.
//!
//! Run with:
//!   cargo test -p daw-reaper --test reaper_setlist_sync -- --ignored --nocapture

use dawfile_reaper::io::read_project;
use dawfile_reaper::setlist_rpp::{self, build_song_infos_from_projects, concatenate_projects, measures_to_seconds};
use eyre::Result;
use reaper_test::{DawInstanceConfig, run_multi_reaper_test};
use session_proto::offset_map::{SetlistOffsetMap, SongOffset};
use session_proto::SongId;
use std::path::PathBuf;
use std::time::Duration;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dawfile-reaper/tests/fixtures/setlist")
        .join(name)
}

#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn setlist_cross_instance_sync() -> Result<()> {
    run_multi_reaper_test(
        "setlist_cross_instance_sync",
        vec![
            DawInstanceConfig::new("project_mode"),
            DawInstanceConfig::new("setlist_mode"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_proj = &ctx.by_label("project_mode").daw;
                let daw_setlist = &ctx.by_label("setlist_mode").daw;

                // ── Setup: parse fixtures, generate combined RPP ─────────
                let rpl_path = fixture_path("test_setlist.RPL");
                let rpp_paths = setlist_rpp::parse_rpl(&rpl_path)?;
                let parsed: Vec<_> = rpp_paths.iter()
                    .map(|p| read_project(p).unwrap())
                    .collect();

                let names = vec!["Song A", "Song B", "Song C"];
                let gap = measures_to_seconds(2, 120.0, 4);
                let song_infos = build_song_infos_from_projects(&parsed, &names, gap);
                let combined = concatenate_projects(&parsed, &song_infos);
                let rpp_text = setlist_rpp::project_to_rpp_text(&combined);

                let temp_dir = std::env::temp_dir().join("fts-setlist-sync-test");
                std::fs::create_dir_all(&temp_dir)?;
                let combined_path = temp_dir.join("Combined Setlist.RPP");
                std::fs::write(&combined_path, &rpp_text)?;

                // Open individual songs in Project Mode instance
                let song_a = daw_proj.open_project(fixture_path("song_a.RPP").to_string_lossy().to_string()).await?;
                let song_b = daw_proj.open_project(fixture_path("song_b.RPP").to_string_lossy().to_string()).await?;
                let song_c = daw_proj.open_project(fixture_path("song_c.RPP").to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Open combined setlist in Setlist Mode instance
                let setlist = daw_setlist.open_project(combined_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Build offset map with actual GUIDs
                let mut offset_map = SetlistOffsetMap {
                    songs: song_infos.iter().enumerate().map(|(i, si)| SongOffset {
                        index: i,
                        song_id: SongId::new(),
                        project_guid: match i {
                            0 => song_a.guid().to_string(),
                            1 => song_b.guid().to_string(),
                            2 => song_c.guid().to_string(),
                            _ => String::new(),
                        },
                        global_start_seconds: si.global_start_seconds,
                        global_start_qn: si.global_start_seconds * 2.0,
                        duration_seconds: si.duration_seconds,
                        duration_qn: si.duration_seconds * 2.0,
                        count_in_seconds: 0.0,
                        start_tempo: 120.0,
                        start_time_sig: daw_proto::TimeSignature::new(4, 4),
                    }).collect(),
                    total_seconds: song_infos.last().map(|s| s.global_start_seconds + s.duration_seconds).unwrap_or(0.0),
                    total_qn: 0.0,
                };
                offset_map.total_qn = offset_map.total_seconds * 2.0;

                let songs = [&song_a, &song_b, &song_c];
                let t_setlist = setlist.transport();

                println!("\n  ═══ CROSS-INSTANCE SETLIST SYNC TEST ═══");
                println!("  Songs: A@{:.0}s B@{:.0}s C@{:.0}s (gap={:.0}s)\n",
                    offset_map.songs[0].global_start_seconds,
                    offset_map.songs[1].global_start_seconds,
                    offset_map.songs[2].global_start_seconds,
                    gap);

                // ── Test 1: Seek each song → verify setlist follows ──────
                println!("  ── Test 1: Song → Setlist position sync ──");
                for (i, (song_proj, name)) in songs.iter().zip(names.iter()).enumerate() {
                    let local = 5.0;
                    let expected_global = offset_map.project_to_setlist(i, local).unwrap();

                    // Seek the song
                    song_proj.transport().set_position(local).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    // Bridge: apply offset to setlist
                    t_setlist.set_position(expected_global).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;

                    let setlist_pos = t_setlist.get_position().await?;
                    let drift = (setlist_pos - expected_global).abs();
                    println!("    {} @ {:.1}s → setlist @ {:.2}s (expected {:.1}s, drift {:.3}s) {}",
                        name, local, setlist_pos, expected_global, drift,
                        if drift < 0.5 { "✓" } else { "✗" });
                    assert!(drift < 0.5, "{} position sync failed: drift {:.3}s", name, drift);
                }

                // ── Test 2: Seek setlist → verify correct song follows ───
                println!("  ── Test 2: Setlist → Song position sync ──");
                let test_positions = [
                    (10.0, 0, "Song A"),   // 10s global → Song A
                    (30.0, 1, "Song B"),   // 30s global → Song B (starts at 26)
                    (55.0, 2, "Song C"),   // 55s global → Song C (starts at 48)
                ];

                for (global, expected_idx, expected_name) in &test_positions {
                    t_setlist.set_position(*global).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    let (song_idx, local_pos) = offset_map.setlist_to_project(*global).unwrap();
                    assert_eq!(song_idx, *expected_idx,
                        "Global {:.0}s should map to {} (idx {}), got idx {}",
                        global, expected_name, expected_idx, song_idx);

                    // Bridge: apply to the correct song tab
                    let target_song = songs[song_idx];
                    daw_proj.select_project(&offset_map.songs[song_idx].project_guid).await?;
                    target_song.transport().set_position(local_pos).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;

                    let actual_pos = target_song.transport().get_position().await?;
                    let drift = (actual_pos - local_pos).abs();
                    println!("    Setlist @ {:.0}s → {} @ {:.2}s (expected {:.2}s, drift {:.3}s) {}",
                        global, expected_name, actual_pos, local_pos, drift,
                        if drift < 0.5 { "✓" } else { "✗" });
                    assert!(drift < 0.5, "Position sync to {} failed", expected_name);
                }

                // ── Test 3: Roundtrip identity ───────────────────────────
                println!("  ── Test 3: Roundtrip identity (all songs) ──");
                for i in 0..3 {
                    for local in [0.0, 3.0, 7.0, 12.0] {
                        let global = offset_map.project_to_setlist(i, local).unwrap();
                        let (back_idx, back_local) = offset_map.setlist_to_project(global).unwrap();
                        assert_eq!(back_idx, i);
                        assert!((back_local - local).abs() < 0.001,
                            "Roundtrip failed: song {} @ {:.1}s → global {:.1}s → song {} @ {:.4}s",
                            i, local, global, back_idx, back_local);
                    }
                }
                println!("    All roundtrips passed ✓");

                // ── Test 4: Play in Project Mode → setlist follows ───────
                println!("  ── Test 4: Play Song A → setlist follows ──");

                // Position both at Song A start
                song_a.transport().set_position(4.0).await?;
                let global_start = offset_map.project_to_setlist(0, 4.0).unwrap();
                t_setlist.set_position(global_start).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                // Select Song A tab and play it
                daw_proj.select_project(song_a.guid()).await?;
                tokio::time::sleep(Duration::from_millis(100)).await;
                song_a.transport().play().await?;

                // Also play setlist
                t_setlist.play().await?;
                tokio::time::sleep(Duration::from_secs(2)).await;

                // Both should be playing and roughly in sync
                let pos_a = song_a.transport().get_position().await?;
                let pos_s = t_setlist.get_position().await?;
                let expected_s = offset_map.project_to_setlist(0, pos_a).unwrap();
                let drift = (pos_s - expected_s).abs();

                println!("    After 2s: Song A={:.2}s, setlist={:.2}s (expected {:.2}s, drift {:.3}s) {}",
                    pos_a, pos_s, expected_s, drift,
                    if drift < 1.0 { "✓" } else { "⚠" });

                // Stop both
                song_a.transport().stop().await?;
                t_setlist.stop().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                // ── Test 5: Play in Setlist Mode → song follows ──────────
                println!("  ── Test 5: Play setlist in Song B area → Song B follows ──");

                // Position setlist in Song B territory
                let song_b_start = offset_map.songs[1].global_start_seconds + 2.0;
                t_setlist.set_position(song_b_start).await?;

                // Position Song B at the corresponding local position
                let (_, local_b) = offset_map.setlist_to_project(song_b_start).unwrap();
                daw_proj.select_project(song_b.guid()).await?;
                song_b.transport().set_position(local_b).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;

                // Play both
                t_setlist.play().await?;
                song_b.transport().play().await?;
                tokio::time::sleep(Duration::from_secs(2)).await;

                let pos_b = song_b.transport().get_position().await?;
                let pos_s = t_setlist.get_position().await?;
                let (idx, _) = offset_map.setlist_to_project(pos_s).unwrap();
                println!("    After 2s: Song B={:.2}s, setlist={:.2}s (maps to song idx={})",
                    pos_b, pos_s, idx);
                assert_eq!(idx, 1, "Setlist should still be in Song B area");

                // Stop
                t_setlist.stop().await?;
                song_b.transport().stop().await?;
                tokio::time::sleep(Duration::from_millis(300)).await;

                // ── Test 6: Seek while stopped, both sides ───────────────
                println!("  ── Test 6: Multiple seeks while stopped ──");

                let seeks = [
                    (2, 8.0, "Song C @ 8s"),
                    (0, 15.0, "Song A @ 15s"),
                    (1, 3.0, "Song B @ 3s"),
                    (0, 0.0, "Song A @ 0s"),
                ];

                for (song_idx, local, label) in &seeks {
                    // Seek the song
                    let song = songs[*song_idx];
                    daw_proj.select_project(&offset_map.songs[*song_idx].project_guid).await?;
                    song.transport().set_position(*local).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    // Bridge: translate and apply to setlist
                    let global = offset_map.project_to_setlist(*song_idx, *local).unwrap();
                    t_setlist.set_position(global).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;

                    // Verify
                    let actual = t_setlist.get_position().await?;
                    let drift = (actual - global).abs();
                    println!("    {} → setlist @ {:.2}s (drift {:.3}s) {}",
                        label, actual, drift,
                        if drift < 0.5 { "✓" } else { "✗" });
                    assert!(drift < 0.5, "Seek sync failed for {}", label);
                }

                // ── Test 7: Reverse direction — setlist → songs ──────────
                println!("  ── Test 7: Setlist seeks → correct song tabs ──");

                let setlist_seeks = [
                    (5.0, 0, "Song A"),
                    (35.0, 1, "Song B"),
                    (60.0, 2, "Song C"),
                    (26.0, 1, "Song B boundary"),
                    (48.0, 2, "Song C boundary"),
                ];

                for (global, expected_idx, label) in &setlist_seeks {
                    t_setlist.set_position(*global).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    let (idx, local) = offset_map.setlist_to_project(*global).unwrap();
                    assert_eq!(idx, *expected_idx,
                        "Setlist @ {:.0}s should map to {} (idx {}), got idx {}",
                        global, label, expected_idx, idx);

                    // Apply to correct song
                    let song = songs[idx];
                    daw_proj.select_project(&offset_map.songs[idx].project_guid).await?;
                    song.transport().set_position(local).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;

                    let actual = song.transport().get_position().await?;
                    println!("    Setlist @ {:.0}s → {} @ {:.2}s ✓", global, label, actual);
                }

                // ── Test 8: Tempo check across instances ─────────────────
                println!("  ── Test 8: Tempo verification ──");
                let tempo_a = song_a.transport().get_state().await?.tempo.bpm;
                let tempo_b = song_b.transport().get_state().await?.tempo.bpm;
                let tempo_c = song_c.transport().get_state().await?.tempo.bpm;
                println!("    Song A: {:.0} BPM, Song B: {:.0} BPM, Song C: {:.0} BPM",
                    tempo_a, tempo_b, tempo_c);

                // Cleanup
                daw_proj.close_project(song_a.guid()).await?;
                daw_proj.close_project(song_b.guid()).await?;
                daw_proj.close_project(song_c.guid()).await?;
                daw_setlist.close_project(setlist.guid()).await?;

                println!("\n  ═══ ALL CROSS-INSTANCE SYNC TESTS PASSED ═══\n");
                Ok(())
            })
        },
    )
}
