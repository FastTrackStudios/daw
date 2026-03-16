//! Integration tests for setlist RPP concatenation.
//!
//! Parses minimal RPP fixtures with FTS markers (PREROLL, COUNT-IN, =START,
//! SONGSTART, SONGEND, =END, POSTROLL), concatenates them, and validates the
//! combined project's structure.

use dawfile_reaper::io::read_project;
use dawfile_reaper::setlist_rpp::{
    self, SongInfo, build_song_infos, build_song_infos_from_projects, measures_to_seconds,
    concatenate_projects, resolve_song_bounds,
};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/setlist")
        .join(name)
}

// ─── Fixture Parsing ─────────────────────────────────────────────────────────

#[test]
fn parse_song_a() {
    let project = read_project(fixture_path("song_a.RPP")).expect("Failed to parse song_a.RPP");

    assert_eq!(project.tracks.len(), 5, "Song A: Click/Guide, Click, TRACKS, Guitar, Bass");

    // Click item: 0→22s
    assert_eq!(project.tracks[1].items.len(), 1);
    assert_eq!(project.tracks[1].items[0].length, 22.0);

    // FTS markers present
    let marker_names: Vec<&str> = project.markers_regions.all.iter().map(|m| m.name.as_str()).collect();
    assert!(marker_names.contains(&"PREROLL"));
    assert!(marker_names.contains(&"Count-In"));
    assert!(marker_names.contains(&"=START"));
    assert!(marker_names.contains(&"SONGSTART"));
    assert!(marker_names.contains(&"SONGEND"));
    assert!(marker_names.contains(&"=END"));
    assert!(marker_names.contains(&"POSTROLL"));

    // Bounds resolution
    let bounds = resolve_song_bounds(&project);
    assert_eq!(bounds.start, 0.0, "PREROLL is at 0");
    assert_eq!(bounds.end, 22.0, "POSTROLL is at 22");

    println!("Song A: {} tracks, bounds {:.1}→{:.1}s, {} markers/regions",
        project.tracks.len(), bounds.start, bounds.end, project.markers_regions.all.len());
}

#[test]
fn parse_song_b() {
    let project = read_project(fixture_path("song_b.RPP")).expect("Failed to parse song_b.RPP");

    assert_eq!(project.tracks.len(), 3, "Song B: Click, Keys, Drums");

    // Tempo: 90→110 BPM
    let env = project.tempo_envelope.as_ref().unwrap();
    assert_eq!(env.points.len(), 2);
    assert_eq!(env.points[0].tempo, 90.0);
    assert_eq!(env.points[1].tempo, 110.0);

    // Bounds: COUNT-IN at 0 → POSTROLL at 18
    let bounds = resolve_song_bounds(&project);
    assert_eq!(bounds.start, 0.0);
    assert_eq!(bounds.end, 18.0);

    // Has Solo Section region
    assert!(project.markers_regions.all.iter().any(|m| m.name == "Solo Section" && m.end_position.is_some()));

    println!("Song B: {} tracks, bounds {:.1}→{:.1}s", project.tracks.len(), bounds.start, bounds.end);
}

#[test]
fn parse_song_c() {
    let project = read_project(fixture_path("song_c.RPP")).expect("Failed to parse song_c.RPP");

    assert_eq!(project.tracks.len(), 3, "Song C: Click, Guide, Synth Lead");

    // Bounds: PREROLL at 0 → POSTROLL at ~17.14
    let bounds = resolve_song_bounds(&project);
    assert_eq!(bounds.start, 0.0);
    assert!((bounds.end - 17.142857).abs() < 0.01);

    println!("Song C: {} tracks, bounds {:.1}→{:.1}s", project.tracks.len(), bounds.start, bounds.end);
}

// ─── RPL Parsing ─────────────────────────────────────────────────────────────

#[test]
fn parse_rpl_file() {
    let rpl_path = fixture_path("test_setlist.RPL");
    let paths = setlist_rpp::parse_rpl(&rpl_path).unwrap();
    assert_eq!(paths.len(), 3);
    for p in &paths {
        assert!(p.exists(), "RPP should exist: {}", p.display());
    }
}

#[test]
fn song_name_extraction() {
    assert_eq!(setlist_rpp::song_name_from_path(&PathBuf::from("Belief - John Mayer [Battle SP26].RPP")), "Belief - John Mayer");
    assert_eq!(setlist_rpp::song_name_from_path(&PathBuf::from("song_a.RPP")), "song_a");
}

// ─── Bounds-Based Concatenation ──────────────────────────────────────────────

#[test]
fn concatenate_three_songs_with_bounds() {
    let rpl_path = fixture_path("test_setlist.RPL");
    let rpp_paths = setlist_rpp::parse_rpl(&rpl_path).unwrap();
    let projects: Vec<_> = rpp_paths.iter()
        .map(|p| read_project(p).unwrap())
        .collect();

    let gap = measures_to_seconds(2, 120.0, 4); // 4s gap

    // Build song infos from resolved bounds
    let names: Vec<&str> = vec!["Song A", "Song B", "Song C"];
    let songs = build_song_infos_from_projects(&projects, &names, gap);

    println!("\nSong layout with bounds:");
    for s in &songs {
        println!("  {} @ {:.1}s, duration {:.1}s", s.name, s.global_start_seconds, s.duration_seconds);
    }

    // Song A: PREROLL(0) → POSTROLL(22) = 22s
    assert_eq!(songs[0].global_start_seconds, 0.0);
    assert_eq!(songs[0].duration_seconds, 22.0);

    // Song B: starts at 22 + 4 gap = 26s, COUNT-IN(0) → POSTROLL(18) = 18s
    assert_eq!(songs[1].global_start_seconds, 26.0);
    assert_eq!(songs[1].duration_seconds, 18.0);

    // Song C: starts at 26 + 18 + 4 gap = 48s
    assert_eq!(songs[2].global_start_seconds, 48.0);

    // Concatenate
    let combined = concatenate_projects(&projects, &songs);

    println!("\n═══ COMBINED PROJECT ═══\n");

    // Tracks
    println!("Tracks ({}):", combined.tracks.len());
    for (i, t) in combined.tracks.iter().enumerate() {
        let items = if t.items.is_empty() { String::new() } else { format!(" ({} items)", t.items.len()) };
        println!("  {:>2}. {}{}", i, t.name, items);
    }

    // Tempo envelope — should have trailing tempo markers
    println!("\nTempo:");
    let env = combined.tempo_envelope.as_ref().unwrap();
    for pt in &env.points {
        println!("  {:.2}s → {:.0} BPM (shape={})", pt.position, pt.tempo, pt.shape);
    }
    // All shapes should be 1 (square)
    for pt in &env.points {
        assert_eq!(pt.shape, 1, "All tempo points should be square, got shape={} at {:.2}s", pt.shape, pt.position);
    }

    // Markers/regions
    println!("\nMarkers/Regions ({}):", combined.markers_regions.all.len());
    for mr in &combined.markers_regions.all {
        let kind = if mr.end_position.is_some() { "RGN" } else { "MKR" };
        let end = mr.end_position.map(|e| format!("→{:.1}s", e)).unwrap_or_default();
        let lane = mr.lane.map(|l| format!(" [lane {}]", l)).unwrap_or_default();
        println!("  [{:>2}] {} {:.1}s{} {:?}{}", mr.id, kind, mr.position, end, mr.name, lane);
    }

    // SONG regions should be in lane 3
    let song_regions: Vec<_> = combined.markers_regions.all.iter()
        .filter(|m| m.lane == Some(3) && m.is_region())
        .collect();
    assert_eq!(song_regions.len(), 3, "Should have 3 SONG lane regions");
    assert_eq!(song_regions[0].name, "Song A");
    assert_eq!(song_regions[1].name, "Song B");
    assert_eq!(song_regions[2].name, "Song C");

    // All IDs unique
    let ids: Vec<i32> = combined.markers_regions.all.iter().map(|m| m.id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "All IDs must be unique");

    println!("\n═══ ALL ASSERTIONS PASSED ═══\n");
}
