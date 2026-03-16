//! Real-world setlist REAPER test.
//!
//! Opens the Battle SP26 combined setlist + a shell copy in REAPER.
//!
//! Run with:
//!   cargo test -p daw-reaper --test reaper_setlist_real -- --ignored --nocapture

use eyre::Result;
use reaper_test::{DawInstanceConfig, run_multi_reaper_test};
use std::path::PathBuf;
use std::time::Duration;

const SETLIST_DIR: &str = "/Users/codywright/Music/Projects/Live Tracks/Z - SETLISTS/Just Friends Battle of the Bands SP26";
const SETLIST_NAME: &str = "Just Friends Battle of the Bands SP26";

#[test]
#[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
fn open_battle_sp26_setlist() -> Result<()> {
    let tracks_path = PathBuf::from(SETLIST_DIR).join(format!("Tracks - {}.RPP", SETLIST_NAME));
    let vocals_path = PathBuf::from(SETLIST_DIR).join(format!("Vocals - {}.RPP", SETLIST_NAME));

    if !tracks_path.exists() {
        eprintln!("Skipping: run setlist_real_rpl test first to generate files");
        return Ok(());
    }

    run_multi_reaper_test(
        "battle_sp26_setlist",
        vec![
            DawInstanceConfig::new("tracks"),
            DawInstanceConfig::new("vocals"),
        ],
        |ctx| {
            Box::pin(async move {
                let daw_tracks = &ctx.by_label("tracks").daw;
                let daw_vocals = &ctx.by_label("vocals").daw;

                let tracks_path = PathBuf::from(SETLIST_DIR).join(format!("Tracks - {}.RPP", SETLIST_NAME));
                let vocals_path = PathBuf::from(SETLIST_DIR).join(format!("Vocals - {}.RPP", SETLIST_NAME));

                // Open Tracks master in first instance
                println!("  Opening Tracks master...");
                let tracks_proj = daw_tracks.open_project(tracks_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Open Vocals shell in second instance
                println!("  Opening Vocals shell...");
                let vocals_proj = daw_vocals.open_project(vocals_path.to_string_lossy().to_string()).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Verify track counts
                let tracks_count = tracks_proj.tracks().count().await?;
                let vocals_count = vocals_proj.tracks().count().await?;
                println!("  Tracks instance: {} tracks", tracks_count);
                println!("  Vocals instance: {} tracks", vocals_count);

                assert!(tracks_count > 20, "Tracks master should have many tracks, got {}", tracks_count);
                assert!(vocals_count < tracks_count, "Vocals shell should have fewer tracks than master");

                // Verify tempo matches
                let tempo_t = tracks_proj.transport().get_state().await?.tempo.bpm;
                let tempo_v = vocals_proj.transport().get_state().await?.tempo.bpm;
                println!("  Tracks tempo: {:.1} BPM", tempo_t);
                println!("  Vocals tempo: {:.1} BPM", tempo_v);
                assert!((tempo_t - tempo_v).abs() < 1.0, "Tempos should match");

                // List some tracks
                let tracks_list = tracks_proj.tracks().all().await?;
                println!("\n  Tracks instance tracks:");
                for t in tracks_list.iter().take(15) {
                    println!("    - {}", t.name);
                }
                if tracks_list.len() > 15 {
                    println!("    ... and {} more", tracks_list.len() - 15);
                }

                let vocals_list = vocals_proj.tracks().all().await?;
                println!("\n  Vocals instance tracks:");
                for t in &vocals_list {
                    println!("    - {}", t.name);
                }

                // Keep open for inspection
                if std::env::var("FTS_KEEP_OPEN").is_ok() {
                    println!("\n  FTS_KEEP_OPEN set — waiting 120s for inspection...");
                    tokio::time::sleep(Duration::from_secs(120)).await;
                }

                // Cleanup
                daw_tracks.close_project(tracks_proj.guid()).await?;
                daw_vocals.close_project(vocals_proj.guid()).await?;

                println!("\n  ═══ BATTLE SP26 SETLIST OPENED SUCCESSFULLY ═══");
                Ok(())
            })
        },
    )
}
