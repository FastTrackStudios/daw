//! A workspace where every track has its own mode, shown at once.
//!
//! The point of the feature is comparing *different parts* on one
//! timeline — a vocal against its reference MIDI against the kit — so
//! these are about tracks disagreeing with each other and the editor
//! coping, not about any one surface.

use expression_editor_core::rows::{RowSpace, SliceBands};
use expression_editor_core::tracks::{Track, Workspace};
use expression_editor_core::{Editor, ExpressionDoc, Mode, TimeBase, Viewport};

fn doc() -> ExpressionDoc {
    ExpressionDoc::new(TimeBase::Frames { frame_rate: 100.0 }, 0.0, 400.0)
}

fn viewport() -> Viewport {
    Viewport {
        w: 1000.0,
        h: 600.0,
    }
}

/// A vocal, its reference MIDI, a guitar and a kit — the arrangement the
/// stacked view exists for.
fn band() -> Editor {
    let mut ed = Editor::new(doc(), viewport());
    ed.set_mode(Mode::PitchedAudio);
    ed.tracks.rename(0, "Lead Vox");
    for (name, mode) in [
        ("Ref MIDI", Mode::Midi),
        ("Guitar", Mode::Guitar),
        ("Kit", Mode::UnpitchedAudio),
    ] {
        ed.tracks.push(Track::in_mode(name, doc(), mode));
    }
    ed
}

#[test]
fn each_track_keeps_its_own_mode() {
    let ed = band();
    let modes: Vec<Mode> = ed.tracks.tracks().iter().map(|t| t.mode).collect();
    assert_eq!(
        modes,
        vec![Mode::PitchedAudio, Mode::Midi, Mode::Guitar, Mode::UnpitchedAudio]
    );
}

#[test]
fn switching_track_brings_its_surface_with_it() {
    // Without this the document changes and nothing else does, so a kit
    // gets edited on whatever surface the vocal left behind.
    let mut ed = band();
    assert_eq!(ed.mode, Mode::PitchedAudio);

    assert!(ed.switch_track(3));
    assert_eq!(ed.mode, Mode::UnpitchedAudio);
    assert!(matches!(ed.row_space, RowSpace::Bands(_)));

    assert!(ed.switch_track(2));
    assert_eq!(ed.mode, Mode::Guitar);
    assert!(matches!(ed.row_space, RowSpace::Strings(_)));
}

#[test]
fn changing_mode_changes_the_active_tracks_mode() {
    // `Editor::mode` is a view of the active track's, not a second
    // source of truth — a stack drawn from the workspace must agree with
    // the surface on screen.
    let mut ed = band();
    ed.set_mode(Mode::Vocals);
    assert_eq!(ed.tracks.track(0).unwrap().mode, Mode::Vocals);
    // And the others are untouched.
    assert_eq!(ed.tracks.track(3).unwrap().mode, Mode::UnpitchedAudio);
}

#[test]
fn a_tuned_row_space_survives_leaving_the_track_and_coming_back() {
    // Switching tracks re-applies the mode preset. A preset that
    // overwrote the row space would throw away band splits the user had
    // moved — an edit silently undone by navigating away.
    let mut ed = band();
    assert!(ed.switch_track(3));

    let custom = SliceBands {
        splits: vec![100.0, 500.0, 4000.0],
        names: vec!["Sub".into(), "Body".into(), "Snap".into(), "Air".into()],
    };
    ed.doc.row_space = RowSpace::Bands(custom.clone());

    assert!(ed.switch_track(0));
    assert!(ed.switch_track(3));
    match &ed.doc.row_space {
        RowSpace::Bands(b) => assert_eq!(b, &custom, "the splits came back changed"),
        other => panic!("expected band space, got {other:?}"),
    }
}

#[test]
fn the_stack_covers_the_height_exactly_and_keeps_track_order() {
    let ed = band();
    let rows = ed.tracks.stack(600.0, 1.0, 20.0);
    assert_eq!(rows.len(), 4);

    assert_eq!(rows[0].y, 0.0);
    for pair in rows.windows(2) {
        assert!(
            (pair[0].y + pair[0].height - pair[1].y).abs() < 1e-3,
            "lanes must meet: {pair:?}"
        );
    }
    let last = rows.last().unwrap();
    assert!((last.y + last.height - 600.0).abs() < 1e-3);
    assert_eq!(
        rows.iter().map(|r| r.track).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn a_vocal_lane_gets_more_room_than_a_slice_strip() {
    // Splitting evenly gives the kit the same height as the vocal, and
    // two octaves of pitch do not fit where three bands do.
    let ed = band();
    let rows = ed.tracks.stack(600.0, 1.0, 0.0);
    let vox = rows.iter().find(|r| r.track == 0).unwrap().height;
    let kit = rows.iter().find(|r| r.track == 3).unwrap().height;
    assert!(vox > kit, "vocal {vox} should be taller than kit {kit}");
}

#[test]
fn the_lane_being_edited_can_be_given_extra_room() {
    let mut ed = band();
    let plain = ed.tracks.stack(600.0, 1.0, 0.0);
    let boosted = ed.tracks.stack(600.0, 3.0, 0.0);
    let of = |rows: &[expression_editor_core::tracks::StackRow], t: usize| {
        rows.iter().find(|r| r.track == t).unwrap().height
    };
    assert!(of(&boosted, 0) > of(&plain, 0));
    // And the boost follows the active track rather than the first one.
    assert!(ed.switch_track(3));
    let moved = ed.tracks.stack(600.0, 3.0, 0.0);
    assert!(of(&moved, 3) > of(&boosted, 3));
    assert!(of(&moved, 0) < of(&boosted, 0));
}

#[test]
fn every_lane_stays_clickable_however_many_there_are() {
    // A lane squeezed to nothing cannot be clicked to select, so a user
    // cannot get back out of the state they fell into.
    let mut ed = band();
    for i in 0..20 {
        ed.tracks
            .push(Track::in_mode(format!("T{i}"), doc(), Mode::Midi));
    }
    let rows = ed.tracks.stack(600.0, 1.0, 12.0);
    assert!(rows.iter().all(|r| r.height >= 12.0 - 1e-3));
    let last = rows.last().unwrap();
    assert!(
        last.y + last.height <= 600.0 + 1e-3,
        "the floor must not overflow the viewport"
    );
}

#[test]
fn an_impossible_floor_degrades_to_an_even_split_rather_than_overflowing() {
    let mut ed = band();
    for i in 0..40 {
        ed.tracks
            .push(Track::in_mode(format!("T{i}"), doc(), Mode::Midi));
    }
    // 44 lanes, 40 px each, 600 px of room: cannot be done.
    let rows = ed.tracks.stack(600.0, 1.0, 40.0);
    let last = rows.last().unwrap();
    assert!((last.y + last.height - 600.0).abs() < 1e-3);
    let first = rows.first().unwrap().height;
    assert!(rows.iter().all(|r| (r.height - first).abs() < 1e-3));
}

#[test]
fn hidden_tracks_leave_the_stack_but_keep_their_place() {
    let mut ed = band();
    ed.tracks.track_mut(1).unwrap().hidden = true;
    let rows = ed.tracks.stack(600.0, 1.0, 0.0);
    assert_eq!(
        rows.iter().map(|r| r.track).collect::<Vec<_>>(),
        vec![0, 2, 3],
        "hiding removes a lane without renumbering the others"
    );
    // Still in the workspace, still editable by index.
    assert_eq!(ed.tracks.len(), 4);
    assert_eq!(ed.tracks.track(1).unwrap().name, "Ref MIDI");
}

#[test]
fn a_y_coordinate_resolves_to_the_lane_under_it() {
    let ed = band();
    let rows = ed.tracks.stack(600.0, 1.0, 0.0);
    for r in &rows {
        let mid = r.y + r.height / 2.0;
        assert_eq!(Workspace::row_at(&rows, mid), Some(r.track));
    }
    // Just inside the top edge belongs to that lane, not the one above.
    for r in &rows {
        assert_eq!(Workspace::row_at(&rows, r.y), Some(r.track));
    }
    assert_eq!(Workspace::row_at(&rows, -1.0), None);
    assert_eq!(Workspace::row_at(&rows, 601.0), None);
}

#[test]
fn a_hand_sized_lane_is_not_resized_by_a_mode_change() {
    let mut t = Track::in_mode("Kit", doc(), Mode::UnpitchedAudio);
    let natural = t.weight;
    t.set_mode(Mode::PitchedAudio);
    assert!(
        (t.weight - natural).abs() > f32::EPSILON,
        "an untouched lane follows its mode"
    );

    t.weight = 5.0;
    t.set_mode(Mode::UnpitchedAudio);
    assert_eq!(t.weight, 5.0, "a hand-sized lane keeps its height");
}
