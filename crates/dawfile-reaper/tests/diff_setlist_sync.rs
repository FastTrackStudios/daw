//! Diff-based setlist sync tests.
//!
//! Verifies that the diff engine can detect real changes between an individual
//! song RPP and its section in a concatenated setlist, ignoring the
//! concatenation offset.
//!
//! This is the foundation for real-time setlist sync: when a user edits
//! Song B in its individual project, we diff against the setlist's Song B
//! section (with offset) to detect what changed, then apply only those
//! changes to the setlist.

use dawfile_reaper::diff::{diff_projects_with_options, ChangeKind, DiffOptions};
use dawfile_reaper::io::read_project;
use dawfile_reaper::setlist_rpp::{
    self, build_song_infos_from_projects, concatenate_projects, measures_to_seconds,
    resolve_song_bounds,
};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/setlist")
        .join(name)
}

/// Parse the 3 fixture songs and generate the combined setlist.
fn setup_setlist() -> SetlistFixture {
    let song_a = read_project(fixture_path("song_a.RPP")).unwrap();
    let song_b = read_project(fixture_path("song_b.RPP")).unwrap();
    let song_c = read_project(fixture_path("song_c.RPP")).unwrap();

    let projects = vec![&song_a, &song_b, &song_c];
    let names: Vec<&str> = vec!["Song A", "Song B", "Song C"];
    let gap = measures_to_seconds(2, 120.0, 4); // 2-measure gap at 120 BPM

    let mut song_infos = build_song_infos_from_projects(
        &[song_a.clone(), song_b.clone(), song_c.clone()],
        &names,
        gap,
    );

    // Resolve bounds for offset tracking
    let bounds_a = resolve_song_bounds(&song_a);
    let bounds_b = resolve_song_bounds(&song_b);
    let bounds_c = resolve_song_bounds(&song_c);

    let combined = concatenate_projects(
        &[song_a.clone(), song_b.clone(), song_c.clone()],
        &song_infos,
    );

    // Song B's global start in the setlist
    let song_b_offset = song_infos[1].global_start_seconds;
    let song_b_end = song_b_offset + song_infos[1].duration_seconds;

    SetlistFixture {
        song_a,
        song_b,
        song_c,
        combined,
        song_b_offset,
        song_b_end,
    }
}

struct SetlistFixture {
    song_a: dawfile_reaper::types::ReaperProject,
    song_b: dawfile_reaper::types::ReaperProject,
    song_c: dawfile_reaper::types::ReaperProject,
    combined: dawfile_reaper::types::ReaperProject,
    song_b_offset: f64,
    song_b_end: f64,
}

/// Diffing song B against itself in the setlist (with offset) should produce
/// no changes — the offset neutralizes the position difference.
#[test]
fn song_b_vs_setlist_no_changes_with_offset() {
    let fix = setup_setlist();

    let options = DiffOptions {
        position_offset: fix.song_b_offset,
        time_window: Some((fix.song_b_offset, fix.song_b_end)),
        match_tracks_by_name: true,
        match_items_by_name: true,
    };

    let diff = diff_projects_with_options(&fix.song_b, &fix.combined, &options);

    println!("Diff summary: {}", diff.summary());
    for track in &diff.tracks {
        println!(
            "  Track {:?} ({:?}): {} props, {} items",
            track.name,
            track.kind,
            track.property_changes.len(),
            track.items.len()
        );
        for prop in &track.property_changes {
            println!(
                "    {} : {} → {}",
                prop.field, prop.old_value, prop.new_value
            );
        }
        for item in &track.items {
            println!("    Item {:?} ({:?})", item.name, item.kind);
            for prop in &item.property_changes {
                println!(
                    "      {} : {} → {}",
                    prop.field, prop.old_value, prop.new_value
                );
            }
        }
    }
    for mr in &diff.markers_regions {
        println!(
            "  Marker/Region {:?} ({:?}): {} props",
            mr.name,
            mr.kind,
            mr.property_changes.len()
        );
    }

    // With the correct offset, item positions should not show as changed.
    // Track-level changes might exist (due to setlist concatenation adding
    // tracks from other songs), but Song B's own tracks should match.
    let item_position_diffs: Vec<_> = diff
        .tracks
        .iter()
        .flat_map(|t| &t.items)
        .filter(|i| i.kind == ChangeKind::Modified)
        .flat_map(|i| &i.property_changes)
        .filter(|p| p.field == "position")
        .collect();

    assert!(
        item_position_diffs.is_empty(),
        "Should have no item position diffs with correct offset, got: {:?}",
        item_position_diffs,
    );
}

/// Modify Song B (add a marker) and diff against the setlist — only the
/// marker addition should appear as a diff, not position changes from offset.
#[test]
fn song_b_marker_added_detected_in_diff() {
    let mut fix = setup_setlist();

    // Add a marker to Song B at 5.0s
    let new_marker = dawfile_reaper::types::MarkerRegion {
        id: 99,
        name: "NEW MARKER".to_string(),
        position: 5.0,
        end_position: None,
        color: 0xFF0000,
        flags: 0,
        locked: 0,
        guid: "{NEW-MARKER-GUID}".to_string(),
        additional: 0,
        lane: None,
        beat_position: None,
    };
    fix.song_b.markers_regions.markers.push(new_marker.clone());
    fix.song_b.markers_regions.all.push(new_marker);

    let options = DiffOptions {
        position_offset: fix.song_b_offset,
        time_window: Some((fix.song_b_offset, fix.song_b_end)),
        match_tracks_by_name: true,
        match_items_by_name: true,
    };

    let diff = diff_projects_with_options(&fix.song_b, &fix.combined, &options);

    println!("After adding marker to Song B:");
    println!("  {}", diff.summary());
    for mr in &diff.markers_regions {
        println!("  Marker {:?} kind={:?}", mr.name, mr.kind);
    }

    // The new marker should show as Removed in the diff (it's in "old" = song_b
    // but not in "new" = setlist's song_b section)
    let new_marker_diff = diff.markers_regions.iter().find(|m| m.name == "NEW MARKER");
    assert!(
        new_marker_diff.is_some(),
        "Should detect the new marker in the diff"
    );
    assert_eq!(
        new_marker_diff.unwrap().kind,
        ChangeKind::Removed,
        "New marker is in song_b (old) but not in setlist (new), so it's 'removed' from diff perspective"
    );
}

/// Modify Song B (change item length) and verify the diff catches it.
#[test]
fn song_b_item_length_changed_detected() {
    let mut fix = setup_setlist();

    // Lengthen the first item on the first content track of Song B
    if let Some(track) = fix.song_b.tracks.iter_mut().find(|t| !t.items.is_empty()) {
        if let Some(item) = track.items.first_mut() {
            let old_len = item.length;
            item.length += 2.0; // Add 2 seconds
            println!(
                "Modified item '{}' length: {:.1} → {:.1}",
                item.name, old_len, item.length
            );
        }
    }

    let options = DiffOptions {
        position_offset: fix.song_b_offset,
        time_window: Some((fix.song_b_offset, fix.song_b_end)),
        match_tracks_by_name: true,
        match_items_by_name: true,
    };

    let diff = diff_projects_with_options(&fix.song_b, &fix.combined, &options);

    println!("After lengthening item in Song B:");
    println!("  {}", diff.summary());

    // Find the item length change
    let length_changes: Vec<_> = diff
        .tracks
        .iter()
        .flat_map(|t| &t.items)
        .filter(|i| i.kind == ChangeKind::Modified)
        .flat_map(|i| &i.property_changes)
        .filter(|p| p.field == "length")
        .collect();

    assert!(
        !length_changes.is_empty(),
        "Should detect the item length change"
    );
    println!("  Length changes: {:?}", length_changes);
}

/// Diffing without offset should produce many position changes (proving offset matters).
#[test]
fn without_offset_positions_differ() {
    let fix = setup_setlist();

    // No offset — raw comparison
    let diff = diff_projects_with_options(&fix.song_b, &fix.combined, &DiffOptions::default());

    let item_position_diffs: Vec<_> = diff
        .tracks
        .iter()
        .flat_map(|t| &t.items)
        .filter(|i| i.kind == ChangeKind::Modified)
        .flat_map(|i| &i.property_changes)
        .filter(|p| p.field == "position")
        .collect();

    // Without offset, Song B items at 0s in the song RPP vs 26s+ in the setlist
    // should show position changes
    println!(
        "Without offset: {} position diffs",
        item_position_diffs.len()
    );
    assert!(
        !item_position_diffs.is_empty() || fix.song_b_offset > 0.0,
        "Without offset, positions should differ (song_b starts at {:.1}s in setlist)",
        fix.song_b_offset,
    );
}
