//! Integration tests for setlist RPP concatenation.
//!
//! Parses minimal RPP fixtures, concatenates them, and validates the
//! combined project's structure — tracks, items, tempo, markers, regions.

use dawfile_reaper::io::read_project;
use dawfile_reaper::setlist_rpp::{SongInfo, build_song_infos, measures_to_seconds, concatenate_projects, concatenate_tracks, concatenate_tempo_envelopes, generate_markers_regions};
use dawfile_reaper::types::track::FolderState;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/setlist")
        .join(name)
}

/// Parse an RPL file: each line is a relative path to an RPP file.
fn parse_rpl(rpl_path: &std::path::Path) -> Vec<PathBuf> {
    let content = std::fs::read_to_string(rpl_path).expect("Failed to read RPL file");
    let parent = rpl_path.parent().unwrap_or(std::path::Path::new("."));
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let path = PathBuf::from(line.trim());
            if path.is_absolute() {
                path
            } else {
                parent.join(path)
            }
        })
        .collect()
}

/// Extract song name from RPP filename (strip extension and bracketed suffixes).
fn song_name_from_path(path: &std::path::Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    // Strip trailing bracketed content like " [Battle SP26]"
    let name = stem.split('[').next().unwrap_or(&stem).trim();
    name.to_string()
}

// ─── Fixture Parsing ─────────────────────────────────────────────────────────

#[test]
fn parse_song_a() {
    let project = read_project(fixture_path("song_a.RPP")).expect("Failed to parse song_a.RPP");

    assert_eq!(project.tracks.len(), 5, "Song A should have 5 tracks");
    assert_eq!(project.tracks[0].name, "Click/Guide");
    assert_eq!(project.tracks[1].name, "Click");
    assert_eq!(project.tracks[2].name, "TRACKS");
    assert_eq!(project.tracks[3].name, "Guitar");
    assert_eq!(project.tracks[4].name, "Bass");

    // Click track has 1 item at position 0, length 24
    assert_eq!(project.tracks[1].items.len(), 1);
    assert_eq!(project.tracks[1].items[0].position, 0.0);
    assert_eq!(project.tracks[1].items[0].length, 24.0);

    // Tempo: 120 BPM 4/4
    let env = project.tempo_envelope.as_ref().expect("Should have tempo envelope");
    assert_eq!(env.points.len(), 1);
    assert_eq!(env.points[0].tempo, 120.0);

    // Markers
    assert!(!project.markers_regions.all.is_empty());
    println!("Song A: {} tracks, {} markers/regions", project.tracks.len(), project.markers_regions.all.len());
}

#[test]
fn parse_song_b() {
    let project = read_project(fixture_path("song_b.RPP")).expect("Failed to parse song_b.RPP");

    assert_eq!(project.tracks.len(), 3, "Song B should have 3 tracks (Click, Keys, Drums)");
    assert_eq!(project.tracks[0].name, "Click");
    assert_eq!(project.tracks[1].name, "Keys");
    assert_eq!(project.tracks[2].name, "Drums");

    // Tempo: starts at 90 BPM 3/4, changes to 110 BPM at ~10.67s
    let env = project.tempo_envelope.as_ref().expect("Should have tempo envelope");
    assert_eq!(env.points.len(), 2, "Song B has a tempo change");
    assert_eq!(env.points[0].tempo, 90.0);
    assert_eq!(env.points[1].tempo, 110.0);

    // Has a region ("Solo Section" from 2s to 12s)
    assert!(project.markers_regions.all.iter().any(|m| m.name == "Solo Section" && m.end_position.is_some()));

    println!("Song B: {} tracks, {} tempo points, {} markers/regions",
        project.tracks.len(), env.points.len(), project.markers_regions.all.len());
}

#[test]
fn parse_song_c() {
    let project = read_project(fixture_path("song_c.RPP")).expect("Failed to parse song_c.RPP");

    assert_eq!(project.tracks.len(), 3, "Song C should have 3 tracks");
    assert_eq!(project.tracks[0].name, "Click");
    assert_eq!(project.tracks[1].name, "Guide");
    assert_eq!(project.tracks[2].name, "Synth Lead");

    // Tempo: 140 BPM 4/4
    let env = project.tempo_envelope.as_ref().expect("Should have tempo envelope");
    assert_eq!(env.points[0].tempo, 140.0);

    // Song C has both Click and Guide tracks — both should merge into shared tracks
    assert!(project.tracks[0].items.len() == 1);
    assert!(project.tracks[1].items.len() == 1);

    println!("Song C: {} tracks, {} markers/regions", project.tracks.len(), project.markers_regions.all.len());
}

// ─── RPL Parsing ─────────────────────────────────────────────────────────────

#[test]
fn parse_rpl_file() {
    let rpl_path = fixture_path("test_setlist.RPL");
    let paths = parse_rpl(&rpl_path);

    assert_eq!(paths.len(), 3);
    assert!(paths[0].ends_with("song_a.RPP"));
    assert!(paths[1].ends_with("song_b.RPP"));
    assert!(paths[2].ends_with("song_c.RPP"));

    // All should exist
    for p in &paths {
        assert!(p.exists(), "RPP file should exist: {}", p.display());
    }
}

#[test]
fn song_name_extraction() {
    assert_eq!(song_name_from_path(&PathBuf::from("Belief - John Mayer [Battle SP26].RPP")), "Belief - John Mayer");
    assert_eq!(song_name_from_path(&PathBuf::from("Vienna - Couch.RPP")), "Vienna - Couch");
    assert_eq!(song_name_from_path(&PathBuf::from("song_a.RPP")), "song_a");
}

// ─── Full Concatenation ──────────────────────────────────────────────────────

#[test]
fn concatenate_three_songs_full() {
    let rpl_path = fixture_path("test_setlist.RPL");
    let rpp_paths = parse_rpl(&rpl_path);

    // Parse all projects
    let projects: Vec<_> = rpp_paths.iter()
        .map(|p| read_project(p).expect(&format!("Failed to parse {}", p.display())))
        .collect();

    // Song A: 24s at 120 BPM, Song B: 16s at 90 BPM, Song C: ~20.57s at 140 BPM
    // 2-measure gap at 120 BPM 4/4 = 4 seconds between each song
    let gap = measures_to_seconds(2, 120.0, 4);
    assert_eq!(gap, 4.0);

    let songs = build_song_infos(
        &[("Song A", 24.0), ("Song B", 16.0), ("Song C", 20.571429)],
        gap,
    );

    let combined = concatenate_projects(&projects, &songs);

    println!("\n═══ COMBINED PROJECT STRUCTURE ═══\n");

    // ── Track Structure ──────────────────────────────────────────────────
    println!("Tracks ({}):", combined.tracks.len());
    for (i, track) in combined.tracks.iter().enumerate() {
        let folder_info = track.folder.as_ref().map(|f| format!(" [{:?} indent={}]", f.folder_state, f.indentation)).unwrap_or_default();
        let item_info = if track.items.is_empty() { String::new() } else { format!(" ({} items)", track.items.len()) };
        println!("  {:>2}. {}{}{}", i, track.name, folder_info, item_info);
    }

    // Verify Click/Guide folder exists with shared items
    let click_guide = combined.tracks.iter().find(|t| t.name == "Click/Guide");
    assert!(click_guide.is_some(), "Should have Click/Guide folder");
    assert_eq!(
        click_guide.unwrap().folder.as_ref().unwrap().folder_state,
        FolderState::FolderParent,
        "Click/Guide should be a folder"
    );

    // Shared Click track should have items from all 3 songs
    let click = combined.tracks.iter().find(|t| t.name == "Click").unwrap();
    assert_eq!(click.items.len(), 3, "Click should have items from all 3 songs");
    assert_eq!(click.items[0].position, 0.0, "Song A click at 0s");
    assert_eq!(click.items[1].position, 28.0, "Song B click at 28s (24 + 4 gap)");
    assert!((click.items[2].position - 48.0).abs() < 0.01, "Song C click at 48s (28 + 16 + 4 gap)");

    // Shared Guide track should have items from Song C only
    let guide = combined.tracks.iter().find(|t| t.name == "Guide").unwrap();
    assert_eq!(guide.items.len(), 1, "Guide should have 1 item from Song C");
    assert!((guide.items[0].position - 48.0).abs() < 0.01, "Song C guide at 48s");

    // TRACKS folder with per-song subfolders
    let tracks_folder = combined.tracks.iter().find(|t| t.name == "TRACKS");
    assert!(tracks_folder.is_some(), "Should have TRACKS folder");

    // Song A folder with Guitar and Bass
    let song_a_folder = combined.tracks.iter().find(|t| t.name == "Song A");
    assert!(song_a_folder.is_some(), "Should have Song A folder");
    let guitar = combined.tracks.iter().find(|t| t.name == "Guitar");
    assert!(guitar.is_some(), "Should have Guitar track from Song A");
    assert_eq!(guitar.unwrap().items[0].position, 0.0, "Guitar item at 0s");

    let bass = combined.tracks.iter().find(|t| t.name == "Bass");
    assert!(bass.is_some(), "Should have Bass track from Song A");

    // Song B folder with Keys and Drums
    let song_b_folder = combined.tracks.iter().find(|t| t.name == "Song B");
    assert!(song_b_folder.is_some(), "Should have Song B folder");
    let keys = combined.tracks.iter().find(|t| t.name == "Keys");
    assert!(keys.is_some(), "Should have Keys track from Song B");
    assert_eq!(keys.unwrap().items[0].position, 28.0, "Keys item offset to 28s");

    let drums = combined.tracks.iter().find(|t| t.name == "Drums");
    assert!(drums.is_some(), "Should have Drums track from Song B");
    assert_eq!(drums.unwrap().items[0].position, 28.0, "Drums item offset to 28s");

    // Song C folder with Synth Lead
    let song_c_folder = combined.tracks.iter().find(|t| t.name == "Song C");
    assert!(song_c_folder.is_some(), "Should have Song C folder");
    let synth = combined.tracks.iter().find(|t| t.name == "Synth Lead");
    assert!(synth.is_some(), "Should have Synth Lead track from Song C");
    assert!((synth.unwrap().items[0].position - 48.0).abs() < 0.01, "Synth Lead offset to 48s");

    // ── Tempo Envelope ───────────────────────────────────────────────────
    println!("\nTempo Envelope:");
    let env = combined.tempo_envelope.as_ref().unwrap();
    for pt in &env.points {
        println!("  {:.2}s → {:.1} BPM (shape={})", pt.position, pt.tempo, pt.shape);
    }

    // Song A: 120 BPM at 0s
    assert_eq!(env.points[0].position, 0.0);
    assert_eq!(env.points[0].tempo, 120.0);

    // Song B: 90 BPM at 28s (boundary, square shape), 110 BPM at 28+10.67=38.67s
    assert_eq!(env.points[1].position, 28.0);
    assert_eq!(env.points[1].tempo, 90.0);
    assert_eq!(env.points[1].shape, 1, "Song B boundary should be square");

    assert!((env.points[2].position - 38.666667).abs() < 0.01, "Song B tempo change at ~38.67s");
    assert_eq!(env.points[2].tempo, 110.0);

    // Song C: 140 BPM at 48s
    assert_eq!(env.points[3].position, 48.0);
    assert_eq!(env.points[3].tempo, 140.0);
    assert_eq!(env.points[3].shape, 1, "Song C boundary should be square");

    // ── Markers & Regions ────────────────────────────────────────────────
    println!("\nMarkers & Regions ({}):", combined.markers_regions.all.len());
    for mr in &combined.markers_regions.all {
        let kind = if mr.end_position.is_some() { "REGION" } else { "MARKER" };
        let end_str = mr.end_position.map(|e| format!(" → {:.2}s", e)).unwrap_or_default();
        println!("  [{:>2}] {} {:.2}s{} \"{}\"", mr.id, kind, mr.position, end_str, mr.name);
    }

    // Should have 3 song-spanning regions
    let song_regions: Vec<_> = combined.markers_regions.regions.iter()
        .filter(|r| r.name == "Song A" || r.name == "Song B" || r.name == "Song C")
        .collect();
    assert_eq!(song_regions.len(), 3, "Should have 3 song regions");

    // Song A region: 0 → 24
    let ra = combined.markers_regions.regions.iter().find(|r| r.name == "Song A").unwrap();
    assert_eq!(ra.position, 0.0);
    assert_eq!(ra.end_position, Some(24.0));

    // Song B region: 28 → 44
    let rb = combined.markers_regions.regions.iter().find(|r| r.name == "Song B").unwrap();
    assert_eq!(rb.position, 28.0);
    assert_eq!(rb.end_position, Some(44.0));

    // Song C region: 48 → 68.57
    let rc = combined.markers_regions.regions.iter().find(|r| r.name == "Song C").unwrap();
    assert!((rc.position - 48.0).abs() < 0.01);
    assert!((rc.end_position.unwrap() - 68.571429).abs() < 0.01);

    // Song B's "Solo Section" region should be offset to 28+2=30s → 28+12=40s
    let solo = combined.markers_regions.all.iter().find(|m| m.name == "Solo Section");
    assert!(solo.is_some(), "Should have Solo Section region from Song B");
    let solo = solo.unwrap();
    assert_eq!(solo.position, 30.0, "Solo Section offset to 30s");
    assert_eq!(solo.end_position, Some(40.0), "Solo Section end offset to 40s");

    // All IDs should be unique
    let ids: Vec<i32> = combined.markers_regions.all.iter().map(|m| m.id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "All marker/region IDs must be unique");

    println!("\n═══ ALL ASSERTIONS PASSED ═══\n");
}
