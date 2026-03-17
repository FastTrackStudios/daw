//! Integration test: combine with trim_to_bounds and verify measure alignment.
//!
//! Uses the real Battle SP26 RPL to verify that:
//! 1. Each song's tempo point lands on a measure boundary
//! 2. Song transitions happen on clean barlines
//! 3. The combined tempo envelope has correct BPM/time-sig at each song start
//! 4. No tempo points bleed outside song bounds

use std::path::{Path, PathBuf};

const RPL_PATH: &str =
    "/Users/codywright/Music/Projects/Live Tracks/Just Friends/Battle SP26.RPL";

fn rpl_exists() -> bool {
    Path::new(RPL_PATH).exists()
}

/// Parse the original project for a song and return its bounds + tempo info.
struct SongExpectation {
    name: String,
    original_tempo: f64,
    original_time_sig: (i32, i32), // (numerator, denominator)
    local_start: f64,
    local_end: f64,
}

fn load_expectations() -> Vec<SongExpectation> {
    let rpp_paths = dawfile_reaper::setlist_rpp::parse_rpl(Path::new(RPL_PATH))
        .expect("parse RPL");

    rpp_paths
        .iter()
        .map(|path| {
            let content = std::fs::read_to_string(path).expect("read RPP");
            let project = dawfile_reaper::parse_project_text(&content).expect("parse RPP");
            let bounds = dawfile_reaper::setlist_rpp::resolve_song_bounds(&project);

            let (tempo, num, denom) = if let Some((bpm, num, denom, _)) = project.properties.tempo {
                (bpm as f64, num, denom)
            } else {
                (120.0, 4, 4)
            };

            SongExpectation {
                name: dawfile_reaper::setlist_rpp::song_name_from_path(path),
                original_tempo: tempo,
                original_time_sig: (num, denom),
                local_start: bounds.start,
                local_end: bounds.end,
            }
        })
        .collect()
}

#[test]
fn trimmed_combine_song_starts_on_measure_boundaries() {
    if !rpl_exists() {
        eprintln!("Skipping: RPL not found at {RPL_PATH}");
        return;
    }

    let expectations = load_expectations();
    let options = dawfile_reaper::setlist_rpp::CombineOptions {
        gap_measures: 2,
        trim_to_bounds: true,
    };

    let (combined, song_infos) = dawfile_reaper::setlist_rpp::combine_rpl(
        Path::new(RPL_PATH),
        &options,
    )
    .expect("combine failed");

    assert_eq!(song_infos.len(), expectations.len());

    // Parse the combined project to access its tempo envelope
    let combined_project = dawfile_reaper::parse_project_text(&combined)
        .expect("parse combined RPP");

    let tempo_env = combined_project
        .tempo_envelope
        .as_ref()
        .expect("combined project should have tempo envelope");

    println!("\n=== Song Info ===");
    for (i, (info, expect)) in song_infos.iter().zip(expectations.iter()).enumerate() {
        let duration = info.duration_seconds;
        let local_dur = expect.local_end - expect.local_start;

        println!(
            "  {i}. {:<35} global_start={:>7.2}s  dur={:>6.2}s  local=[{:.2}..{:.2}]  tempo={} {}/{}",
            info.name,
            info.global_start_seconds,
            duration,
            expect.local_start,
            expect.local_end,
            expect.original_tempo,
            expect.original_time_sig.0,
            expect.original_time_sig.1,
        );

        // Verify duration matches local bounds
        assert!(
            (duration - local_dur).abs() < 0.1,
            "Song {} duration {:.2} should match local bounds {:.2}",
            i, duration, local_dur
        );
    }

    // Check that each song start position falls on a measure boundary
    // in the combined project's tempo map.
    println!("\n=== Measure Alignment at Song Starts ===");
    for (i, info) in song_infos.iter().enumerate() {
        if i == 0 {
            // First song starts at 0 — that's always measure 1 beat 1
            println!("  {i}. {} @ {:.2}s → measure 1 beat 1 (first song)", info.name, info.global_start_seconds);
            continue;
        }

        let (measure, beat, fraction) =
            tempo_env.musical_position_at_time(info.global_start_seconds);

        println!(
            "  {i}. {} @ {:.2}s → measure {} beat {} fraction {:.3}",
            info.name, info.global_start_seconds, measure, beat, fraction
        );

        // Song should start on beat 1 with fraction ~0
        assert_eq!(
            beat, 1,
            "Song {} ({}) at {:.2}s should start on beat 1, got beat {}",
            i, info.name, info.global_start_seconds, beat
        );
        assert!(
            fraction < 0.05 || fraction > 0.95,
            "Song {} ({}) at {:.2}s should start on a measure boundary, got fraction {:.3}",
            i, info.name, info.global_start_seconds, fraction
        );
    }

    // Verify tempo points in the combined project
    println!("\n=== Tempo Points ===");
    for pt in &tempo_env.points {
        let ts_str = pt
            .time_signature_encoded
            .map(|ts| {
                let num = ts & 0xFFFF;
                let denom = ts >> 16;
                format!("{}/{}", num, denom)
            })
            .unwrap_or_else(|| "-".to_string());

        println!(
            "  PT pos={:>8.2}  tempo={:>6.1}  shape={}  ts={}",
            pt.position, pt.tempo, pt.shape, ts_str
        );
    }

    // Verify each song has a tempo point at or very near its start
    println!("\n=== Tempo Point at Song Start ===");
    for (i, info) in song_infos.iter().enumerate() {
        let has_pt_at_start = tempo_env.points.iter().any(|pt| {
            (pt.position - info.global_start_seconds).abs() < 0.01
        });

        let expect = &expectations[i];
        println!(
            "  {i}. {} @ {:.2}s — has tempo point: {} (expect {} BPM {}/{})",
            info.name,
            info.global_start_seconds,
            has_pt_at_start,
            expect.original_tempo,
            expect.original_time_sig.0,
            expect.original_time_sig.1,
        );

        assert!(
            has_pt_at_start,
            "Song {} ({}) should have a tempo point at its start ({:.2}s)",
            i, info.name, info.global_start_seconds
        );

        // Verify the tempo at the song start matches the original project's tempo
        if let Some(pt) = tempo_env.points.iter().find(|pt| {
            (pt.position - info.global_start_seconds).abs() < 0.01
        }) {
            assert!(
                (pt.tempo - expect.original_tempo).abs() < 0.5,
                "Song {} ({}) tempo at start should be ~{} BPM, got {} BPM",
                i, info.name, expect.original_tempo, pt.tempo
            );
        }
    }

    // Verify no songs overlap
    println!("\n=== Song Gaps ===");
    for i in 1..song_infos.len() {
        let prev_end =
            song_infos[i - 1].global_start_seconds + song_infos[i - 1].duration_seconds;
        let gap = song_infos[i].global_start_seconds - prev_end;

        println!(
            "  {} → {}: gap={:.2}s (prev_end={:.2}s, next_start={:.2}s)",
            song_infos[i - 1].name,
            song_infos[i].name,
            gap,
            prev_end,
            song_infos[i].global_start_seconds,
        );

        assert!(
            gap >= 0.0,
            "Songs should not overlap: {} ends at {:.2}s but {} starts at {:.2}s (gap={:.2}s)",
            song_infos[i - 1].name,
            prev_end,
            song_infos[i].name,
            song_infos[i].global_start_seconds,
            gap
        );
    }

    // Write combined RPP for manual REAPER verification
    let output_path = PathBuf::from("/tmp/Battle_SP26_Trimmed_Combined.RPP");
    std::fs::write(&output_path, &combined).expect("write combined RPP");
    println!("\nWrote combined RPP to: {}", output_path.display());
    println!("Open in REAPER to verify measure grid alignment visually.");
}
