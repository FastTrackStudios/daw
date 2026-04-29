//! Walk every `.logicx` fixture and print a structured summary.
//! Run with `cargo test -p dawfile-logic --test fixture_summary -- --nocapture`.

use dawfile_logic::read_session;
use std::path::{Path, PathBuf};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixtures() -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(FIXTURE_DIR)
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            // .logicx is a directory bundle
            (p.extension()?.to_str()? == "logicx").then_some(p)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn dump_all_logic_fixtures() {
    let files = fixtures();
    assert!(!files.is_empty());

    eprintln!("\n=== {} Logic fixtures ===", files.len());
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy();
        let s = read_session(path).unwrap_or_else(|e| panic!("parse {name}: {e}"));

        let total_clips: usize = s.tracks.iter().map(|t| t.clips.len()).sum();

        eprintln!(
            "── {name}\n   creator={} variant={:?} sr={} bpm={:.2} {}/{} key={}-{}\n   \
             tracks={} clips={} markers={} tempo_events={} summing_groups={} chunks={}",
            s.creator_version,
            s.variant_name,
            s.sample_rate,
            s.bpm,
            s.time_sig_numerator,
            s.time_sig_denominator,
            s.key,
            s.key_gender,
            s.tracks.len(),
            total_clips,
            s.markers.len(),
            s.tempo_events.len(),
            s.summing_groups.len(),
            s.chunks.len(),
        );

        for (i, t) in s.tracks.iter().enumerate().take(8) {
            eprintln!(
                "   track[{i}]: {:?} ch={} db={:?} mute={} solo={} clips={}",
                t.name,
                t.channel,
                t.fader_db,
                t.muted,
                t.soloed,
                t.clips.len()
            );
        }
    }
}
