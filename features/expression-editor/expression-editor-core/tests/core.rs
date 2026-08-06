//! Regression suite for the portable engine — the headless half.
//!
//! Everything here is reachable without a GPU, a DAW, or a browser,
//! which is the point of keeping the engine dependency-free.

use expression_editor_core::blob;
use expression_editor_core::camera::{self, Bounds, Camera, Content, Viewport};
use expression_editor_core::doc::{Curve, ExpressionDoc, Lane, Note, NoteId, Point, Target, TimeBase};
use expression_editor_core::edit::{Edit, History};
use expression_editor_core::modulation::{CurveTarget, Row, Stack, Wave};
use expression_editor_core::shape::{self, Shape};
use expression_editor_core::tools::{self, Grid, Mods, Selection, Tool};
use expression_editor_core::tuning::{self, Tuning};
use expression_editor_core::{Editor, content_of};

const PPQ: f64 = 960.0;

fn doc_with_note() -> ExpressionDoc {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    let mut n = Note::new(NoteId(1), 0.0, PPQ * 2.0, 60);
    n.channel = Some(2);
    doc.push(n);
    doc
}

// ── curves ───────────────────────────────────────────────────────────

#[test]
fn curve_replaces_rather_than_stacks_at_a_repeated_time() {
    let mut c = Curve::new();
    c.set(100.0, 1.0);
    c.set(100.0, 2.0);
    assert_eq!(c.len(), 1, "a revisited tick must replace, not duplicate");
    assert_eq!(c.points()[0].value, 2.0);
}

#[test]
fn curve_stays_sorted_when_drawn_right_to_left() {
    let mut c = Curve::new();
    for t in [500.0, 100.0, 300.0, 200.0] {
        c.set(t, t / 1000.0);
    }
    let ts: Vec<f64> = c.points().iter().map(|p| p.t).collect();
    assert_eq!(ts, vec![100.0, 200.0, 300.0, 500.0]);
}

#[test]
fn curve_holds_its_endpoints_outside_the_authored_range() {
    let mut c = Curve::new();
    c.set(100.0, 3.0);
    c.set(200.0, 5.0);
    // Not the default — an authored curve must not snap to center at
    // the note edges.
    assert_eq!(c.sample(0.0, 0.0), 3.0);
    assert_eq!(c.sample(999.0, 0.0), 5.0);
    assert_eq!(c.sample(150.0, 0.0), 4.0, "linear between points");
}

#[test]
fn splice_preserves_points_outside_the_interval() {
    let mut c = Curve::from_points(vec![
        Point { t: 0.0, value: 0.0 },
        Point { t: 100.0, value: 1.0 },
        Point { t: 200.0, value: 2.0 },
        Point { t: 300.0, value: 3.0 },
    ]);
    c.splice(100.0, 200.0, &[Point { t: 150.0, value: 9.0 }]);
    let pts: Vec<(f64, f64)> = c.points().iter().map(|p| (p.t, p.value)).collect();
    assert_eq!(pts, vec![(0.0, 0.0), (150.0, 9.0), (300.0, 3.0)]);
}

#[test]
fn reshape_preserves_endpoints_exactly() {
    let mut c = Curve::new();
    c.set(0.0, 0.0);
    c.set(400.0, 12.0);
    for shape in Shape::ALL {
        let mut c2 = c.clone();
        c2.reshape(0.0, 400.0, shape, 32, 0.0);
        assert!((c2.sample(0.0, 0.0) - 0.0).abs() < 1e-9, "{shape:?} start");
        assert!((c2.sample(400.0, 0.0) - 12.0).abs() < 1e-6, "{shape:?} end");
    }
}

#[test]
fn scale_about_below_zero_inverts_the_gesture() {
    let mut c = Curve::from_points(vec![
        Point { t: 0.0, value: 0.0 },
        Point { t: 100.0, value: 2.0 },
    ]);
    c.scale_about(0.0, 100.0, 1.0, -1.0);
    assert_eq!(c.sample(0.0, 0.0), 2.0);
    assert_eq!(c.sample(100.0, 0.0), 0.0);
}

#[test]
fn remap_time_stretches_owned_expression_onto_new_bounds() {
    let mut c = Curve::from_points(vec![
        Point { t: 0.0, value: 0.0 },
        Point { t: 100.0, value: 1.0 },
    ]);
    c.remap_time(0.0, 100.0, 0.0, 400.0);
    assert_eq!(c.points().last().unwrap().t, 400.0);
}

// ── shapes ───────────────────────────────────────────────────────────

#[test]
fn every_shape_is_a_unit_map() {
    for shape in Shape::ALL {
        assert!(shape.amount(0.0).abs() < 1e-9, "{shape:?} at 0");
        assert!((shape.amount(1.0) - 1.0).abs() < 1e-9, "{shape:?} at 1");
        // Monotone: no shape may double back.
        let mut prev = -1.0;
        for i in 0..=50 {
            let v = shape.amount(i as f64 / 50.0);
            assert!(v >= prev - 1e-9, "{shape:?} not monotone at {i}");
            prev = v;
        }
    }
}

#[test]
fn simplify_trims_collinear_runs_but_keeps_the_gesture() {
    let straight: Vec<(f64, f64)> = (0..50).map(|i| (i as f64, i as f64 * 2.0)).collect();
    assert_eq!(shape::simplify(&straight, 0.01).len(), 2);

    let peaked: Vec<(f64, f64)> = (0..=20)
        .map(|i| (i as f64, if i == 10 { 50.0 } else { 0.0 }))
        .collect();
    let kept = shape::simplify(&peaked, 0.5);
    assert!(
        kept.iter().any(|&(_, y)| y == 50.0),
        "the peak must survive simplification"
    );
}

// ── zones ────────────────────────────────────────────────────────────

#[test]
fn a_note_without_splits_still_has_exactly_one_zone() {
    let n = Note::new(NoteId(1), 0.0, 100.0, 60);
    assert_eq!(n.zone_count(), 1);
    assert_eq!(n.zones(), vec![(0.0, 100.0)]);
}

#[test]
fn splits_stay_sorted_and_interior() {
    let mut n = Note::new(NoteId(1), 0.0, 100.0, 60);
    assert!(n.add_split(60.0));
    assert!(n.add_split(30.0));
    assert!(!n.add_split(0.0), "a split at the note start is not interior");
    assert!(!n.add_split(100.0), "nor at the end");
    assert!(!n.add_split(30.0), "nor a duplicate");
    assert_eq!(n.splits, vec![30.0, 60.0]);
    assert_eq!(n.zones(), vec![(0.0, 30.0), (30.0, 60.0), (60.0, 100.0)]);
}

#[test]
fn inserting_a_split_before_the_active_zone_keeps_the_same_zone_active() {
    let mut n = Note::new(NoteId(1), 0.0, 100.0, 60);
    n.add_split(50.0);
    n.target = Target::Zone(1); // the 50..100 half
    n.add_split(25.0);
    assert_eq!(n.target, Target::Zone(2));
    assert_eq!(n.target_span(), (50.0, 100.0), "still the same region");
}

#[test]
fn target_toggles_between_one_zone_and_the_whole_note() {
    assert_eq!(tools::toggle_target(Target::WholeNote, 1), Target::Zone(1));
    assert_eq!(tools::toggle_target(Target::Zone(1), 1), Target::WholeNote);
    assert_eq!(tools::toggle_target(Target::Zone(1), 2), Target::Zone(2));
}

// ── edits ────────────────────────────────────────────────────────────

#[test]
fn drawing_extends_to_the_note_edges_so_no_gap_is_left() {
    let mut doc = doc_with_note();
    let edit = Edit::DrawLane {
        note: NoteId(1),
        lane: Lane::Pitch,
        t0: 500.0,
        t1: 1000.0,
        points: vec![
            Point { t: 500.0, value: 1.0 },
            Point { t: 1000.0, value: 2.0 },
        ],
    };
    assert!(edit.apply(&mut doc));
    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.pitch.sample(n.start, 0.0), 1.0, "held back to note start");
    assert_eq!(n.pitch.sample(n.end, 0.0), 2.0, "held out to note end");
}

#[test]
fn erasing_one_lane_leaves_the_others_untouched() {
    let mut doc = doc_with_note();
    for lane in Lane::ALL {
        Edit::DrawLane {
            note: NoteId(1),
            lane,
            t0: 0.0,
            t1: 1000.0,
            points: vec![
                Point { t: 0.0, value: 0.5 },
                Point { t: 1000.0, value: 0.5 },
            ],
        }
        .apply(&mut doc);
    }
    let before = doc.note(NoteId(1)).unwrap().pitch.clone();
    Edit::EraseLane {
        note: NoteId(1),
        lane: Lane::Pressure,
        // The whole note: drawing held the curve out to the note edges,
        // so erasing only the drawn interval would leave those.
        t0: 0.0,
        t1: PPQ * 2.0,
    }
    .apply(&mut doc);
    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.pitch, before, "pitch must survive a pressure edit byte-for-byte");
    assert!(n.pressure.is_empty());
}

#[test]
fn resize_stretches_expression_and_zones_together() {
    let mut doc = doc_with_note();
    {
        let n = doc.note_mut(NoteId(1)).unwrap();
        n.add_split(PPQ);
        n.pitch.set(0.0, 0.0);
        n.pitch.set(PPQ * 2.0, 4.0);
    }
    assert!(Edit::Resize {
        note: NoteId(1),
        start: 0.0,
        end: PPQ * 4.0,
    }
    .apply(&mut doc));
    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.splits, vec![PPQ * 2.0], "the split scales with the note");
    assert_eq!(n.pitch.points().last().unwrap().t, PPQ * 4.0);
}

#[test]
fn splitting_a_note_gives_both_halves_the_boundary_value() {
    let mut doc = doc_with_note();
    {
        let n = doc.note_mut(NoteId(1)).unwrap();
        n.pitch.set(0.0, 0.0);
        n.pitch.set(PPQ * 2.0, 4.0);
    }
    assert!(Edit::SplitNote {
        note: NoteId(1),
        t: PPQ,
    }
    .apply(&mut doc));
    assert_eq!(doc.notes.len(), 2);
    let left = doc.note(NoteId(1)).unwrap();
    assert_eq!(left.end, PPQ);
    let right = doc.notes.iter().find(|n| n.id != NoteId(1)).unwrap();
    assert_eq!(right.start, PPQ);
    // Both sides hold the value at the split — no jump across the cut.
    assert!((left.pitch.sample(PPQ, 0.0) - right.pitch.sample(PPQ, 0.0)).abs() < 1e-9);
}

#[test]
fn transpose_moves_the_row_and_leaves_the_curve_shape_alone() {
    let mut doc = doc_with_note();
    doc.note_mut(NoteId(1)).unwrap().pitch.set(0.0, 0.25);
    Edit::Transpose {
        notes: vec![NoteId(1)],
        semitones: 3,
    }
    .apply(&mut doc);
    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.row, 63);
    assert_eq!(n.pitch.sample(0.0, 0.0), 0.25, "the gesture moves rigidly");
    assert!((n.sounding_midi(0.0) - 63.25).abs() < 1e-9);
}

#[test]
fn move_time_carries_owned_expression() {
    let mut doc = doc_with_note();
    doc.note_mut(NoteId(1)).unwrap().pitch.set(100.0, 1.0);
    Edit::MoveTime {
        notes: vec![NoteId(1)],
        delta: PPQ,
    }
    .apply(&mut doc);
    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.start, PPQ);
    assert_eq!(n.pitch.points()[0].t, 100.0 + PPQ);
}

// ── MPE safety ───────────────────────────────────────────────────────

#[test]
fn overlapping_notes_sharing_a_channel_are_flagged_ambiguous() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    let mut a = Note::new(NoteId(1), 0.0, PPQ * 2.0, 60);
    a.channel = Some(2);
    let mut b = Note::new(NoteId(2), PPQ, PPQ * 3.0, 64);
    b.channel = Some(2);
    doc.push(a);
    doc.push(b);
    doc.mark_ambiguity();
    assert!(doc.notes.iter().all(|n| n.ambiguous));
}

#[test]
fn notes_on_different_channels_are_never_ambiguous() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    let mut a = Note::new(NoteId(1), 0.0, PPQ * 2.0, 60);
    a.channel = Some(2);
    let mut b = Note::new(NoteId(2), PPQ, PPQ * 3.0, 64);
    b.channel = Some(3);
    doc.push(a);
    doc.push(b);
    doc.mark_ambiguity();
    assert!(doc.notes.iter().all(|n| !n.ambiguous));
}

#[test]
fn channel_assignment_separates_overlapping_and_consecutive_notes() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 16.0);
    // Three overlapping, then one starting exactly where another ends.
    doc.push(Note::new(NoteId(1), 0.0, PPQ * 4.0, 60));
    doc.push(Note::new(NoteId(2), PPQ, PPQ * 5.0, 64));
    doc.push(Note::new(NoteId(3), PPQ * 2.0, PPQ * 6.0, 67));
    doc.push(Note::new(NoteId(4), PPQ * 4.0, PPQ * 8.0, 72));
    let ids: Vec<NoteId> = doc.notes.iter().map(|n| n.id).collect();
    assert!(Edit::AssignChannels {
        notes: ids,
        seed: 42
    }
    .apply(&mut doc));

    for n in &doc.notes {
        let ch = n.channel.expect("every note gets a member channel");
        assert!((2..=16).contains(&ch), "channel 1 stays the MPE master");
    }
    // Touching counts as conflicting: note 4 starts where note 1 ends.
    let ch1 = doc.note(NoteId(1)).unwrap().channel;
    let ch4 = doc.note(NoteId(4)).unwrap().channel;
    assert_ne!(ch1, ch4, "a touching handoff must not reuse the channel");
    assert!(doc.notes.iter().all(|n| !n.ambiguous));
}

#[test]
fn channel_assignment_is_deterministic_per_seed() {
    let build = |seed: u64| {
        let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 16.0);
        for i in 0..6 {
            doc.push(Note::new(
                NoteId(i + 1),
                PPQ * i as f64 * 0.5,
                PPQ * (i as f64 * 0.5 + 2.0),
                60 + i as i32,
            ));
        }
        let ids: Vec<NoteId> = doc.notes.iter().map(|n| n.id).collect();
        Edit::AssignChannels { notes: ids, seed }.apply(&mut doc);
        doc.notes
            .iter()
            .map(|n| n.channel)
            .collect::<Vec<_>>()
    };
    assert_eq!(build(7), build(7));
}

// ── tuning ───────────────────────────────────────────────────────────

#[test]
fn equal_temperament_has_no_offsets() {
    assert!(tuning::EQUAL.is_equal());
    for t in tuning::PRESETS.iter().skip(1) {
        assert!(!t.is_equal(), "{} should not be equal", t.name);
    }
}

#[test]
fn bend_round_trips_through_the_14_bit_word() {
    for st in [-24.0, -1.0, 0.0, 0.5, 12.0, 47.0] {
        let raw = tuning::semitones_to_bend14(st, 48.0);
        let back = tuning::bend14_to_semitones(raw, 48.0);
        assert!((back - st).abs() < 0.01, "{st} round-tripped to {back}");
    }
    assert_eq!(tuning::semitones_to_bend14(0.0, 48.0), 8192, "center");
}

#[test]
fn snapping_offers_microtonal_and_ordinary_centers_together() {
    let t = Tuning {
        temperament: tuning::RAST,
        key_pc: 0,
        snap_12tet: true,
    };
    // E in the key of C is Rast's half-flat third: 50 cents low.
    assert!((t.center(64) - 63.5).abs() < 1e-6);
    let targets = t.targets_near(63.6, 1.0);
    assert!(targets.iter().any(|x| (x.pitch - 63.5).abs() < 1e-6));
    assert!(
        targets.iter().any(|x| (x.pitch - 64.0).abs() < 1e-6),
        "with snap_12tet on, the plain semitone is offered too"
    );
    assert!((t.snap(63.6).pitch - 63.5).abs() < 1e-6);
}

#[test]
fn snapping_without_12tet_lands_only_on_the_temperament() {
    let t = Tuning {
        temperament: tuning::RAST,
        key_pc: 0,
        snap_12tet: false,
    };
    let targets = t.targets_near(63.9, 1.0);
    assert!(
        !targets.iter().any(|x| (x.pitch - 64.0).abs() < 1e-6),
        "the plain semitone must not be offered"
    );
}

#[test]
fn note_names_follow_the_midi_60_is_c4_convention() {
    assert_eq!(tuning::note_name(60), "C4");
    assert_eq!(tuning::note_name(69), "A4");
    assert_eq!(tuning::note_name(0), "C-1");
}

// ── decomposition ────────────────────────────────────────────────────

#[test]
fn decomposition_round_trips_the_curve_it_came_from() {
    // A slide plus vibrato, sampled as if analyzed.
    let mut c = Curve::new();
    for i in 0..200 {
        let t = i as f64 * 10.0;
        let secs = t / 960.0;
        let slide = -0.4 * secs;
        let vib = 0.3 * (core::f64::consts::TAU * 5.5 * secs).sin();
        c.set(t, slide + vib);
    }
    let d = blob::decompose(&c, 0.0, 1990.0, 200, 960.0, 0.0);
    let back = d.recompose(d.center, 1.0, 1.0);
    for i in 0..200 {
        let t = i as f64 * 10.0;
        let a = c.sample(t, 0.0);
        let b = back.sample(t, 0.0);
        assert!((a - b).abs() < 1e-6, "at t={t}: {a} vs {b}");
    }
}

#[test]
fn flattening_both_amounts_gives_a_dead_flat_line() {
    let mut c = Curve::new();
    for i in 0..100 {
        let t = i as f64 * 10.0;
        c.set(t, (i as f64 * 0.3).sin() * 0.5);
    }
    let d = blob::decompose(&c, 0.0, 990.0, 100, 960.0, 0.0);
    let robot = d.recompose(d.center, 0.0, 0.0);
    let values: Vec<f64> = robot.points().iter().map(|p| p.value).collect();
    assert!(values.iter().all(|v| (v - d.center).abs() < 1e-12));
}

#[test]
fn drift_and_vibrato_separate_at_the_three_hertz_line() {
    // 0.5 Hz slide + 6 Hz vibrato over two seconds at 960 units/sec.
    let mut c = Curve::new();
    let n = 400;
    for i in 0..n {
        let secs = i as f64 / 200.0;
        let t = secs * 960.0;
        let drift = 0.8 * (core::f64::consts::TAU * 0.5 * secs).sin();
        let vib = 0.25 * (core::f64::consts::TAU * 6.0 * secs).sin();
        c.set(t, drift + vib);
    }
    let d = blob::decompose(&c, 0.0, 1920.0, n, 960.0, 0.0);
    // The vibrato lands in `modulation`, the slide in `drift`.
    assert!(
        d.modulation_depth() > 0.3,
        "vibrato depth {} should be near 0.5 peak-to-peak",
        d.modulation_depth()
    );
    let drift_pp = d.drift.iter().cloned().fold(f64::MIN, f64::max)
        - d.drift.iter().cloned().fold(f64::MAX, f64::min);
    assert!(drift_pp > 1.0, "the 0.5 Hz slide belongs to drift, got {drift_pp}");
}

#[test]
fn effective_center_finds_where_the_curve_dwells_not_its_mean() {
    // A scoop: starts a fourth low, settles on target for most of the
    // note. The mean would sit well below the target.
    let mut c = Curve::new();
    for i in 0..100 {
        let t = i as f64 * 10.0;
        c.set(t, if i < 15 { -5.0 + i as f64 * 0.33 } else { 0.0 });
    }
    let center = blob::effective_center(&c, 0.0, 990.0, 100, 0.0);
    assert!(
        center.abs() < 0.5,
        "should settle on the target, got {center}"
    );
}

#[test]
fn reblending_pitch_flattens_vibrato_without_moving_the_center() {
    let mut doc = doc_with_note();
    {
        let n = doc.note_mut(NoteId(1)).unwrap();
        for i in 0..100 {
            let t = i as f64 * (PPQ * 2.0 / 100.0);
            n.pitch.set(t, 0.4 * (i as f64 * 0.4).sin());
        }
    }
    assert!(Edit::ReblendPitch {
        note: NoteId(1),
        t0: 0.0,
        t1: PPQ * 2.0,
        drift_amount: 0.0,
        modulation_amount: 0.0,
    }
    .apply(&mut doc));
    let n = doc.note(NoteId(1)).unwrap();
    let (lo, hi) = n.pitch.value_bounds().unwrap();
    assert!(hi - lo < 1e-6, "robot mode should be flat, spread {}", hi - lo);
}

// ── history ──────────────────────────────────────────────────────────

#[test]
fn undo_restores_zones_notes_and_expression_as_one_step() {
    let mut doc = doc_with_note();
    let mut history = History::new(10);
    let before = doc.clone();

    history.apply(&mut doc, &Edit::AddZoneSplit {
        note: NoteId(1),
        t: PPQ,
    });
    assert_eq!(doc.note(NoteId(1)).unwrap().splits.len(), 1);

    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc.note(NoteId(1)).unwrap().splits.len(), 1);
}

#[test]
fn a_failed_edit_records_no_undo_step() {
    let mut doc = doc_with_note();
    let mut history = History::new(10);
    assert!(!history.apply(&mut doc, &Edit::Transpose {
        notes: vec![NoteId(999)],
        semitones: 1
    }));
    assert!(!history.can_undo());
}

#[test]
fn history_is_bounded() {
    let mut doc = doc_with_note();
    let mut history = History::new(3);
    for i in 0..10 {
        history.apply(&mut doc, &Edit::AddZoneSplit {
            note: NoteId(1),
            t: 10.0 + i as f64 * 10.0,
        });
    }
    let mut count = 0;
    while history.undo(&mut doc) {
        count += 1;
    }
    assert_eq!(count, 3);
}

// ── camera ───────────────────────────────────────────────────────────

fn test_content() -> Content {
    Content {
        t_start: 0.0,
        t_end: 4000.0,
        pitch_lo: 58.0,
        pitch_hi: 70.0,
    }
}

#[test]
fn zoom_pins_the_anchor_under_the_pointer() {
    let vp = Viewport::new(800.0, 480.0);
    let mut cam = camera::reset_view(test_content(), vp, 0.03, 0.35);
    let anchor_t = cam.t_at(200.0);
    cam.zoom_time_about(anchor_t, 2.0);
    assert!(
        (cam.x(anchor_t) - 200.0).abs() < 1e-6,
        "the anchored time must stay at the same pixel"
    );

    let anchor_p = cam.pitch_at(120.0, vp);
    cam.zoom_pitch_about(anchor_p, 2.0, vp);
    assert!((cam.y(anchor_p, vp) - 120.0).abs() < 1e-6);
}

#[test]
fn reset_view_frames_the_content_with_headroom() {
    let vp = Viewport::new(800.0, 480.0);
    let c = test_content();
    let cam = camera::reset_view(c, vp, 0.03, 0.35);
    let (lo, hi) = cam.pitch_span(vp);
    assert!(lo < c.pitch_lo && hi > c.pitch_hi, "content must fit inside");
    let (t0, t1) = cam.time_span(vp);
    assert!(t0 < c.t_start && t1 > c.t_end);
    assert!(
        (cam.pitch_center - 64.0).abs() < 1e-6,
        "centered on the content midpoint"
    );
}

#[test]
fn blending_with_no_influences_is_the_identity() {
    let vp = Viewport::new(800.0, 480.0);
    let cam = camera::reset_view(test_content(), vp, 0.03, 0.35);
    assert_eq!(camera::blend(cam, &[]), cam);
}

#[test]
fn a_full_weight_influence_fully_replaces_the_base() {
    let vp = Viewport::new(800.0, 480.0);
    let base = camera::reset_view(test_content(), vp, 0.03, 0.35);
    let target = Camera {
        t0: 1234.0,
        units_per_px: 2.0,
        pitch_center: 70.0,
        px_per_semitone: 20.0,
    };
    let out = camera::blend(base, &[camera::Influence {
        camera: target,
        weight: 1.0,
    }]);
    assert!((out.t0 - target.t0).abs() < 1e-6);
    assert!((out.px_per_semitone - target.px_per_semitone).abs() < 1e-6);
}

#[test]
fn scales_blend_geometrically_so_zoom_stays_even() {
    let base = Camera {
        t0: 0.0,
        units_per_px: 1.0,
        pitch_center: 60.0,
        px_per_semitone: 10.0,
    };
    let target = Camera {
        units_per_px: 100.0,
        ..base
    };
    let out = camera::blend(base, &[camera::Influence {
        camera: target,
        weight: 0.5,
    }]);
    // Geometric midpoint of 1 and 100 is 10, not the arithmetic 50.5 —
    // an arithmetic blend would bias every magnet toward zoomed-out.
    assert!((out.units_per_px - 10.0).abs() < 1e-6, "got {}", out.units_per_px);
}

#[test]
fn the_edge_magnet_is_inert_in_the_middle_of_the_item() {
    let vp = Viewport::new(800.0, 480.0);
    let c = test_content();
    let cam = camera::reset_view(c, vp, 0.03, 0.35);
    assert!(
        camera::edge_magnet(cam, 2000.0, c, vp, 0.35, 0.2).is_none(),
        "no pull at the item center"
    );
    let near_edge = camera::edge_magnet(cam, 3950.0, c, vp, 0.35, 0.2);
    assert!(near_edge.is_some_and(|i| i.weight > 0.9), "full pull at the edge");
}

#[test]
fn the_reset_tail_stays_out_until_the_final_stretch() {
    let vp = Viewport::new(800.0, 480.0);
    let reset = camera::reset_view(test_content(), vp, 0.03, 0.35);
    // Deep zoom-in: nowhere near reset.
    let deep = Camera {
        units_per_px: reset.units_per_px / 50.0,
        px_per_semitone: reset.px_per_semitone * 50.0,
        ..reset
    };
    assert!(
        camera::reset_tail(deep, reset, 0.8).is_none(),
        "the reset magnet must not fight an ordinary zoom-out"
    );
    // Essentially there.
    let close = Camera {
        units_per_px: reset.units_per_px * 0.98,
        ..reset
    };
    assert!(camera::reset_tail(close, reset, 0.8).is_some());
}

#[test]
fn constrain_never_shows_more_than_the_cushioned_item() {
    let vp = Viewport::new(800.0, 480.0);
    let bounds = Bounds {
        t_min: 0.0,
        t_max: 4000.0,
        ..Bounds::default()
    };
    let mut cam = Camera {
        t0: -99999.0,
        units_per_px: 1000.0,
        pitch_center: 200.0,
        px_per_semitone: 0.001,
    };
    cam.constrain(bounds, vp);
    let (t0, t1) = cam.time_span(vp);
    assert!(t1 - t0 <= 4000.0 + 1e-6);
    assert!(cam.pitch_center <= 127.0 && cam.pitch_center >= 0.0);
    assert!(cam.px_per_semitone >= bounds.min_px_per_semitone);
    assert!(t0 >= bounds.t_min - (t1 - t0) * bounds.edge_whitespace - 1e-6);
}

// ── editor integration ───────────────────────────────────────────────

fn test_editor() -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    for i in 0..4 {
        let mut n = Note::new(
            NoteId(i + 1),
            PPQ * i as f64,
            PPQ * (i as f64 + 0.9),
            60 + i as i32 * 2,
        );
        n.channel = Some(2 + i as u8);
        doc.push(n);
    }
    Editor::new(doc, Viewport::new(900.0, 500.0))
}

#[test]
fn a_new_editor_frames_its_content() {
    let ed = test_editor();
    let c = content_of(&ed.doc);
    let (lo, hi) = ed.camera.pitch_span(ed.viewport);
    assert!(lo <= c.pitch_lo && hi >= c.pitch_hi);
    assert_eq!(ed.tool, Tool::Curve, "Curve is the default tool");
    assert_eq!(ed.lane, Lane::Pitch);
}

#[test]
fn v_returns_to_the_same_camera_from_anywhere() {
    let mut ed = test_editor();
    let reset = ed.camera;
    ed.zoom_in_at(300.0, 200.0, 4.0);
    assert_ne!(ed.camera, reset);
    ed.reset_view();
    assert_eq!(ed.camera, reset, "V is exact, not approximate");
}

#[test]
fn zooming_in_never_drifts_toward_reset_view() {
    let mut ed = test_editor();
    let start = ed.camera.units_per_px;
    for _ in 0..5 {
        ed.zoom_in_at(450.0, 250.0, 1.3);
    }
    assert!(
        ed.camera.units_per_px < start,
        "zoom-in must monotonically increase magnification"
    );
}

#[test]
fn repeated_zoom_out_converges_on_reset_view() {
    let mut ed = test_editor();
    ed.zoom_in_at(450.0, 250.0, 20.0);
    for _ in 0..80 {
        ed.zoom_out_at(450.0, 250.0, 1.2);
    }
    let reset = ed.reset_camera();
    assert!(
        (ed.camera.units_per_px / reset.units_per_px - 1.0).abs() < 0.3,
        "zoom-out should land near Reset View: {} vs {}",
        ed.camera.units_per_px,
        reset.units_per_px
    );
}

#[test]
fn hit_testing_prefers_a_note_body_over_empty_canvas() {
    let ed = test_editor();
    let n = &ed.doc.notes[0];
    let x = ed.camera.x((n.start + n.end) * 0.5);
    let y = ed.camera.y(n.row as f64, ed.viewport);
    match ed.hit_test(x, y) {
        expression_editor_core::Hit::Note { id, zone } => {
            assert_eq!(id, n.id);
            assert_eq!(zone, 0);
        }
        other => panic!("expected the note body, got {other:?}"),
    }
}

#[test]
fn hit_testing_prefers_an_edge_handle_over_the_body() {
    let ed = test_editor();
    let n = &ed.doc.notes[0];
    let x = ed.camera.x(n.end);
    let y = ed.camera.y(n.row as f64, ed.viewport);
    assert!(matches!(
        ed.hit_test(x, y),
        expression_editor_core::Hit::NoteEdge { start_edge: false, .. }
    ));
}

#[test]
fn the_whole_row_height_belongs_to_the_note() {
    let ed = test_editor();
    let n = &ed.doc.notes[0];
    let x = ed.camera.x((n.start + n.end) * 0.5);
    // Just inside the top of the row band.
    let y = ed.camera.y(n.row as f64 + 0.45, ed.viewport);
    assert!(matches!(
        ed.hit_test(x, y),
        expression_editor_core::Hit::Note { .. }
    ));
}

#[test]
fn marquee_selects_the_notes_it_covers() {
    let ed = test_editor();
    let mut sel = Selection::default();
    let x0 = ed.camera.x(0.0);
    // Stops just short of the third note's onset — marquee selection is
    // intersection-based, so touching its start would include it.
    let x1 = ed.camera.x(PPQ * 1.95);
    let y0 = ed.camera.y(70.0, ed.viewport);
    let y1 = ed.camera.y(58.0, ed.viewport);
    sel.marquee(&ed.doc, &ed.camera, ed.viewport, (x0, y0), (x1, y1), false);
    assert_eq!(sel.notes.len(), 2, "the first two notes only");
}

#[test]
fn a_drag_belongs_to_the_selection_not_the_note_under_the_pointer() {
    let mut sel = Selection::default();
    sel.add(NoteId(1));
    sel.add(NoteId(2));
    assert_eq!(
        tools::gesture_targets(&sel, Some(NoteId(3))),
        vec![NoteId(1), NoteId(2)]
    );
    assert_eq!(
        tools::gesture_targets(&Selection::default(), Some(NoteId(3))),
        vec![NoteId(3)]
    );
}

#[test]
fn gestures_clamp_directionally_when_they_start_outside_the_span() {
    // Starting left of the note extends only to the left boundary.
    assert_eq!(tools::clamp_gesture((100.0, 200.0), 50.0, 50.0, 150.0), (100.0, 150.0));
    // Starting right extends only to the right boundary.
    assert_eq!(tools::clamp_gesture((100.0, 200.0), 250.0, 150.0, 250.0), (150.0, 200.0));
    // Starting inside clamps both ends.
    assert_eq!(tools::clamp_gesture((100.0, 200.0), 150.0, 50.0, 250.0), (100.0, 200.0));
}

#[test]
fn the_local_grid_reads_out_as_musical_fractions() {
    let mut g = Grid::default();
    assert_eq!(g.label(), "1/16");
    g.triplet = true;
    assert_eq!(g.label(), "1/16T");
    g.triplet = false;
    g.coarser();
    assert_eq!(g.label(), "1/8");
    g.finer();
    g.finer();
    assert_eq!(g.label(), "1/32");
}

#[test]
fn grid_snapping_uses_the_documents_own_origin() {
    let g = Grid::default();
    // 1/16 at 960 ppq = 240 units.
    assert_eq!(g.step(PPQ), 240.0);
    assert_eq!(g.snap(250.0, 0.0, PPQ), 240.0);
    assert_eq!(g.snap(250.0, 100.0, PPQ), 340.0, "origin shifts the phase");
}

#[test]
fn pressure_and_timbre_map_into_a_fixed_two_semitone_box() {
    let ed = test_editor();
    let row = 60;
    for v in [0.0, 0.25, 0.5, 1.0] {
        let y = tools::lane_box_y(&ed.camera, ed.viewport, row, v);
        let back = tools::lane_box_value(&ed.camera, ed.viewport, row, y);
        assert!((back - v).abs() < 1e-9, "{v} round-tripped to {back}");
    }
}

#[test]
fn the_active_lane_draws_last_and_overlays_never_duplicate_it() {
    let mut ed = test_editor();
    ed.lane = Lane::Pressure;
    ed.overlays = vec![Lane::Pitch, Lane::Pressure];
    assert_eq!(ed.draw_order(), vec![Lane::Pitch, Lane::Pressure]);
}

// ── modulation ───────────────────────────────────────────────────────

#[test]
fn an_oscillator_alone_stays_within_its_amplitude() {
    let stack = Stack {
        rows: vec![Row::Oscillator {
            wave: Wave::Sine,
            amplitude: 0.5,
            rate: 4.0,
        }],
    };
    for v in stack.render(200) {
        assert!(v.abs() <= 0.5 + 1e-9);
    }
}

#[test]
fn a_following_curve_row_envelopes_the_oscillator() {
    let stack = Stack::growing_vibrato();
    let rendered = stack.render(200);
    let early = rendered[..20].iter().fold(0.0f64, |a, b| a.max(b.abs()));
    let late = rendered[180..].iter().fold(0.0f64, |a, b| a.max(b.abs()));
    assert!(late > early * 2.0, "vibrato should grow: {early} → {late}");

    let receding = Stack::receding_vibrato().render(200);
    let early = receding[..20].iter().fold(0.0f64, |a, b| a.max(b.abs()));
    let late = receding[180..].iter().fold(0.0f64, |a, b| a.max(b.abs()));
    assert!(early > late * 2.0, "and recede: {early} → {late}");
}

#[test]
fn a_rate_curve_accelerates_phase_instead_of_stepping_it() {
    let stack = Stack {
        rows: vec![
            Row::Oscillator {
                wave: Wave::Sine,
                amplitude: 1.0,
                rate: 8.0,
            },
            Row::Curve {
                shape: Shape::Linear,
                depth: 1.0,
                up: true,
                target: CurveTarget::Rate,
            },
        ],
    };
    // Count zero crossings in the first and last quarter: an
    // accelerating vibrato has more cycles late than early.
    let r = stack.render(2000);
    let crossings = |s: &[f64]| s.windows(2).filter(|w| w[0].signum() != w[1].signum()).count();
    let early = crossings(&r[..500]);
    let late = crossings(&r[1500..]);
    assert!(late > early, "rate should ramp up: {early} → {late}");
    // And the output stays continuous — no jump between cycles.
    let max_step = r.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f64, f64::max);
    assert!(max_step < 0.2, "phase must integrate smoothly, max step {max_step}");
}

#[test]
fn modulation_tapers_to_nothing_at_the_boundaries() {
    let mut values = vec![0.0f64; 200];
    let stack = Stack {
        rows: vec![Row::Oscillator {
            wave: Wave::Square,
            amplitude: 1.0,
            rate: 6.0,
        }],
    };
    expression_editor_core::modulation::apply(&mut values, &stack, 0.1);
    assert!(values[0].abs() < 1e-9, "no step at the start");
    assert!(values[199].abs() < 1e-9, "no step at the end");
    assert!(
        values.iter().any(|v| v.abs() > 0.5),
        "but full depth in the middle"
    );
}

// ── modulation as an edit ────────────────────────────────────────────

#[test]
fn applying_modulation_tapers_at_the_span_edges() {
    let mut doc = doc_with_note();
    {
        let n = doc.note_mut(NoteId(1)).unwrap();
        n.pitch.set(0.0, 0.0);
        n.pitch.set(PPQ * 2.0, 0.0);
    }
    let stack = Stack {
        rows: vec![Row::Oscillator {
            wave: Wave::Sine,
            amplitude: 0.5,
            rate: 8.0,
        }],
    };
    assert!(Edit::ApplyModulation {
        note: NoteId(1),
        lane: Lane::Pitch,
        t0: 0.0,
        t1: PPQ * 2.0,
        stack,
        taper: 0.1,
        samples: 128,
    }
    .apply(&mut doc));

    let n = doc.note(NoteId(1)).unwrap();
    // No step at the boundaries — a modulation dropped in cold at a
    // nonzero phase would click.
    assert!(n.pitch.sample(0.0, 0.0).abs() < 1e-6);
    assert!(n.pitch.sample(PPQ * 2.0, 0.0).abs() < 1e-6);
    let (lo, hi) = n.pitch.value_bounds().unwrap();
    assert!(hi - lo > 0.5, "but full depth inside, got {}", hi - lo);
}

#[test]
fn modulation_preserves_data_outside_its_target_range() {
    let mut doc = doc_with_note();
    {
        let n = doc.note_mut(NoteId(1)).unwrap();
        n.pitch.set(0.0, 3.0);
        n.pitch.set(PPQ * 2.0, 3.0);
    }
    // Target only the second half.
    Edit::ApplyModulation {
        note: NoteId(1),
        lane: Lane::Pitch,
        t0: PPQ,
        t1: PPQ * 2.0,
        stack: Stack::growing_vibrato(),
        taper: 0.1,
        samples: 64,
    }
    .apply(&mut doc);
    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.pitch.sample(0.0, 0.0), 3.0, "the untouched half is intact");
    assert!((n.pitch.sample(PPQ, 0.0) - 3.0).abs() < 1e-6, "and the seam");
}

#[test]
fn restore_puts_a_captured_curve_back_exactly() {
    let mut doc = doc_with_note();
    {
        let n = doc.note_mut(NoteId(1)).unwrap();
        for i in 0..40 {
            n.pitch.set(i as f64 * 48.0, (i as f64 * 0.3).sin());
        }
    }
    let captured = doc.note(NoteId(1)).unwrap().pitch.clone();

    Edit::ApplyModulation {
        note: NoteId(1),
        lane: Lane::Pitch,
        t0: 0.0,
        t1: PPQ * 2.0,
        stack: Stack::growing_vibrato(),
        taper: 0.1,
        samples: 64,
    }
    .apply(&mut doc);
    assert_ne!(doc.note(NoteId(1)).unwrap().pitch, captured);

    Edit::RestoreLane {
        note: NoteId(1),
        lane: Lane::Pitch,
        t0: 0.0,
        t1: PPQ * 2.0,
        points: captured.points().to_vec(),
    }
    .apply(&mut doc);
    assert_eq!(
        doc.note(NoteId(1)).unwrap().pitch,
        captured,
        "cancel must restore byte-for-byte"
    );
}

// ── full MIDI editing ────────────────────────────────────────────────

use expression_editor_core::mouse::{Action, Context, Gesture, ModKey, MouseMap};
use expression_editor_core::rows::{Articulation, DrumMap, RowSpace, StringTuning};

fn doc_with_notes(n: usize) -> ExpressionDoc {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 16.0);
    for i in 0..n {
        let mut note = Note::new(
            NoteId(i as u64 + 1),
            PPQ * i as f64,
            PPQ * (i as f64 + 0.5),
            60 + i as i32,
        );
        note.channel = Some(2 + i as u8);
        doc.push(note);
    }
    doc
}

fn ids(doc: &ExpressionDoc) -> Vec<NoteId> {
    doc.notes.iter().map(|n| n.id).collect()
}

#[test]
fn velocity_nudges_accumulate_and_clamp() {
    let mut doc = doc_with_notes(1);
    Edit::SetVelocity {
        notes: vec![NoteId(1)],
        velocity: 0.5,
    }
    .apply(&mut doc);
    for _ in 0..10 {
        Edit::NudgeVelocity {
            notes: vec![NoteId(1)],
            delta: 0.1,
        }
        .apply(&mut doc);
    }
    assert_eq!(doc.note(NoteId(1)).unwrap().velocity, 1.0, "clamps at full");
}

#[test]
fn channel_nudge_wraps_inside_the_member_range() {
    let mut doc = doc_with_notes(1);
    doc.note_mut(NoteId(1)).unwrap().channel = Some(16);
    Edit::NudgeChannel {
        notes: vec![NoteId(1)],
        delta: 1,
    }
    .apply(&mut doc);
    // Channel 1 is the MPE master and must never be assigned.
    assert_eq!(doc.note(NoteId(1)).unwrap().channel, Some(2));

    Edit::NudgeChannel {
        notes: vec![NoteId(1)],
        delta: -1,
    }
    .apply(&mut doc);
    assert_eq!(doc.note(NoteId(1)).unwrap().channel, Some(16));
}

#[test]
fn muting_is_not_deleting() {
    let mut doc = doc_with_notes(2);
    Edit::ToggleMuted {
        notes: vec![NoteId(1)],
    }
    .apply(&mut doc);
    assert!(doc.note(NoteId(1)).unwrap().muted);
    assert_eq!(doc.notes.len(), 2, "the note is still in the document");
    Edit::ToggleMuted {
        notes: vec![NoteId(1)],
    }
    .apply(&mut doc);
    assert!(!doc.note(NoteId(1)).unwrap().muted);
}

#[test]
fn doubling_and_halving_length_round_trip() {
    let mut doc = doc_with_notes(1);
    let len = doc.note(NoteId(1)).unwrap().len();
    Edit::ScaleLength {
        notes: vec![NoteId(1)],
        factor: 2.0,
    }
    .apply(&mut doc);
    assert!((doc.note(NoteId(1)).unwrap().len() - len * 2.0).abs() < 1e-6);
    Edit::ScaleLength {
        notes: vec![NoteId(1)],
        factor: 0.5,
    }
    .apply(&mut doc);
    assert!((doc.note(NoteId(1)).unwrap().len() - len).abs() < 1e-6);
}

#[test]
fn stretching_positions_arpeggiates_about_the_pivot() {
    let mut doc = doc_with_notes(3);
    let all = ids(&doc);
    Edit::StretchPositions {
        notes: all,
        pivot: 0.0,
        factor: 2.0,
    }
    .apply(&mut doc);
    assert_eq!(doc.note(NoteId(1)).unwrap().start, 0.0, "the pivot is fixed");
    assert!((doc.note(NoteId(2)).unwrap().start - PPQ * 2.0).abs() < 1e-6);
    assert!((doc.note(NoteId(3)).unwrap().start - PPQ * 4.0).abs() < 1e-6);
}

#[test]
fn copying_notes_leaves_the_originals_and_carries_expression() {
    let mut doc = doc_with_notes(1);
    doc.note_mut(NoteId(1)).unwrap().pitch.set(0.0, 0.75);
    Edit::CopyNotes {
        notes: vec![NoteId(1)],
        time_delta: PPQ * 4.0,
        row_delta: 12,
    }
    .apply(&mut doc);
    assert_eq!(doc.notes.len(), 2);
    let copy = doc.notes.iter().find(|n| n.id != NoteId(1)).unwrap();
    assert_eq!(copy.row, 72);
    assert_eq!(copy.start, PPQ * 4.0);
    assert_eq!(copy.pitch.sample(PPQ * 4.0, 0.0), 0.75, "expression came too");
    assert_eq!(doc.note(NoteId(1)).unwrap().start, 0.0, "original untouched");
}

#[test]
fn partial_quantize_pulls_toward_the_grid_without_nailing_to_it() {
    let mut doc = doc_with_notes(1);
    Edit::MoveTime {
        notes: vec![NoteId(1)],
        delta: 100.0,
    }
    .apply(&mut doc);

    let mut half = doc.clone();
    Edit::Quantize {
        notes: vec![NoteId(1)],
        step: PPQ,
        origin: 0.0,
        strength: 0.5,
    }
    .apply(&mut half);
    assert!(
        (half.note(NoteId(1)).unwrap().start - 50.0).abs() < 1e-6,
        "half strength moves half the distance"
    );

    Edit::Quantize {
        notes: vec![NoteId(1)],
        step: PPQ,
        origin: 0.0,
        strength: 1.0,
    }
    .apply(&mut doc);
    assert_eq!(doc.note(NoteId(1)).unwrap().start, 0.0);
}

#[test]
fn legato_extends_each_note_to_the_next_on_its_row() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    doc.push(Note::new(NoteId(1), 0.0, PPQ * 0.25, 60));
    doc.push(Note::new(NoteId(2), PPQ, PPQ * 1.25, 60));
    // A different row must not close the first note's gap.
    doc.push(Note::new(NoteId(3), PPQ * 0.5, PPQ * 0.75, 64));

    Edit::Legato {
        notes: vec![NoteId(1)],
        gap: 0.0,
    }
    .apply(&mut doc);
    assert_eq!(doc.note(NoteId(1)).unwrap().end, PPQ);
}

#[test]
fn a_note_carries_its_lyric() {
    let mut doc = doc_with_notes(1);
    Edit::SetText {
        note: NoteId(1),
        text: Some("Ha".into()),
    }
    .apply(&mut doc);
    assert_eq!(doc.note(NoteId(1)).unwrap().text.as_deref(), Some("Ha"));
    // And a lyric is the note's label, outranking the note name.
    assert_eq!(
        RowSpace::Pitch.note_label(doc.note(NoteId(1)).unwrap()),
        Some("Ha".to_string())
    );
}

// ── guitar / bass ────────────────────────────────────────────────────

#[test]
fn a_string_roll_sounds_open_pitch_plus_fret() {
    let t = StringTuning::guitar_standard();
    assert_eq!(t.pitch(0, 0), 40, "low E");
    assert_eq!(t.pitch(5, 0), 64, "high E");
    assert_eq!(t.pitch(0, 12), 52, "twelfth fret is an octave");
}

#[test]
fn a_capo_raises_every_open_string() {
    let mut t = StringTuning::guitar_standard();
    t.capo = 2;
    assert_eq!(t.pitch(0, 0), 42);
    assert_eq!(t.pitch(5, 3), 69);
}

#[test]
fn fingering_prefers_the_position_nearest_the_hand() {
    let t = StringTuning::guitar_standard();
    // A4 = 69 is reachable on several strings.
    let low = t.best_position(69, 0).unwrap();
    let high = t.best_position(69, 14).unwrap();
    assert_ne!(low, high, "the preferred fret must actually steer the choice");
    assert!(low.1 < high.1, "a hand at the nut plays the lower fret");
}

#[test]
fn moving_a_note_to_another_string_keeps_its_sounding_pitch() {
    let tuning = StringTuning::guitar_standard();
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    doc.row_space = RowSpace::Strings(tuning.clone());
    let mut n = Note::new(NoteId(1), 0.0, PPQ, 5); // high E string
    n.fret = Some(5); // A4
    doc.push(n);
    let before = doc.row_space.pitch_of(doc.note(NoteId(1)).unwrap());

    assert!(Edit::SetString {
        note: NoteId(1),
        string: 4,
    }
    .apply(&mut doc));

    let n = doc.note(NoteId(1)).unwrap();
    assert_eq!(n.row, 4);
    assert_eq!(
        doc.row_space.pitch_of(n),
        before,
        "changing string re-fingers, it does not transpose"
    );
    assert_eq!(n.fret, Some(10));
}

#[test]
fn an_unreachable_string_refuses_the_move() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    doc.row_space = RowSpace::Strings(StringTuning::guitar_standard());
    let mut n = Note::new(NoteId(1), 0.0, PPQ, 5);
    n.fret = Some(0); // E4 — below the low E string's open pitch? no, above
    doc.push(n);
    // E4 = 64 cannot be played on the high E string at a negative fret,
    // and moving a low note up a string can go out of range.
    let mut n2 = Note::new(NoteId(2), 0.0, PPQ, 0);
    n2.fret = Some(0); // E2 = 40
    doc.push(n2);
    assert!(
        !Edit::SetString {
            note: NoteId(2),
            string: 5,
        }
        .apply(&mut doc),
        "E2 is not reachable on the high E string"
    );
}

#[test]
fn a_string_roll_labels_notes_with_their_fret() {
    let space = RowSpace::Strings(StringTuning::guitar_standard());
    let mut n = Note::new(NoteId(1), 0.0, 100.0, 2);
    n.fret = Some(7);
    assert_eq!(space.note_label(&n), Some("7".to_string()));
    assert_eq!(space.row_label(2), "D", "third string is D");
}

#[test]
fn legato_articulations_are_marked_as_such() {
    for a in Articulation::ALL {
        let legato = matches!(
            a,
            Articulation::HammerOn | Articulation::PullOff | Articulation::LegatoSlide
        );
        assert_eq!(a.is_legato(), legato, "{a:?}");
    }
    // Natural harmonics only speak at certain frets.
    assert!(Articulation::NaturalHarmonic
        .valid_frets()
        .unwrap()
        .contains(&12));
    assert!(Articulation::PalmMute.valid_frets().is_none());
}

#[test]
fn setting_an_articulation_sets_the_legato_flag_with_it() {
    let mut doc = doc_with_notes(1);
    Edit::SetArticulation {
        notes: vec![NoteId(1)],
        articulation: Some(Articulation::HammerOn),
    }
    .apply(&mut doc);
    assert!(doc.note(NoteId(1)).unwrap().legato);

    Edit::SetArticulation {
        notes: vec![NoteId(1)],
        articulation: Some(Articulation::PalmMute),
    }
    .apply(&mut doc);
    assert!(!doc.note(NoteId(1)).unwrap().legato);
}

// ── drums ────────────────────────────────────────────────────────────

#[test]
fn drum_rows_map_to_their_pitches_both_ways() {
    let map = DrumMap::general_midi();
    let space = RowSpace::Drums(map.clone());
    let kick_row = map.row_of_pitch(36).unwrap();
    let mut n = Note::new(NoteId(1), 0.0, 100.0, kick_row as i32);
    assert_eq!(space.pitch_of(&n), 36);
    assert_eq!(space.row_label(kick_row as i32), "Kick");
    assert_eq!(space.row_of_pitch(38), map.row_of_pitch(38).map(|r| r as i32));
    n.row = 0;
    assert!(space.draws_diamonds(), "a drum hit has no meaningful length");
}

#[test]
fn drum_rows_have_no_accidental_shading() {
    let space = RowSpace::Drums(DrumMap::general_midi());
    assert!(!space.is_accidental(1), "black keys mean nothing on a kit");
    assert!(RowSpace::Pitch.is_accidental(61));
}

// ── mouse map ────────────────────────────────────────────────────────

#[test]
fn modifiers_resolve_to_their_bound_action() {
    let m = MouseMap::reaper_like();
    let none = Mods::default();
    let ctrl = Mods {
        ctrl: true,
        ..Default::default()
    };
    let alt = Mods {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        m.resolve(Context::PianoRoll, Gesture::Drag, none),
        Action::MarqueeSelect
    );
    assert_eq!(
        m.resolve(Context::Note, Gesture::Drag, ctrl),
        Action::EditNoteVelocity
    );
    assert_eq!(m.resolve(Context::Note, Gesture::Drag, alt), Action::CopyNote);
    assert_eq!(
        m.resolve(Context::NoteEdge, Gesture::Drag, none),
        Action::MoveNoteEdge
    );
}

#[test]
fn an_unbound_modifier_falls_back_to_the_plain_binding() {
    let m = MouseMap::reaper_like();
    let weird = Mods {
        shift: true,
        ctrl: true,
        alt: true,
    };
    // Never dead: a user holding a stray modifier gets the obvious
    // thing rather than nothing.
    assert_eq!(
        m.resolve(Context::Ruler, Gesture::Click, weird),
        Action::MovePlayhead
    );
}

#[test]
fn presets_differ_where_the_products_differ() {
    let none = Mods::default();
    let reaper = MouseMap::reaper_like();
    let drums = MouseMap::drums();
    let riffer = MouseMap::riffer();

    // A drum part is built by sweeping hits, not by marquee.
    assert_eq!(
        reaper.resolve(Context::PianoRoll, Gesture::Drag, none),
        Action::MarqueeSelect
    );
    assert_eq!(
        drums.resolve(Context::PianoRoll, Gesture::Drag, none),
        Action::PaintNotes
    );
    // Riffer inserts on plain click and deletes on double-click.
    assert_eq!(
        riffer.resolve(Context::PianoRoll, Gesture::Click, none),
        Action::InsertNote
    );
    assert_eq!(
        riffer.resolve(Context::Note, Gesture::DoubleClick, none),
        Action::EraseNote
    );
    // And every preset can still be listed for a cheat sheet.
    for name in MouseMap::PRESETS {
        assert!(!MouseMap::preset(name).bindings().is_empty());
    }
}

#[test]
fn rebinding_replaces_rather_than_stacking() {
    let mut m = MouseMap::reaper_like();
    let before = m.bindings().len();
    m.set(
        Context::Note,
        Gesture::Drag,
        ModKey::NONE,
        Action::EditNoteVelocity,
    );
    assert_eq!(m.bindings().len(), before, "no duplicate binding");
    assert_eq!(
        m.resolve(Context::Note, Gesture::Drag, Mods::default()),
        Action::EditNoteVelocity
    );
}

#[test]
fn edits_are_distinguished_from_navigation_for_undo_grouping() {
    assert!(Action::MoveNote.is_edit());
    assert!(Action::EditNoteVelocity.is_edit());
    assert!(!Action::MarqueeSelect.is_edit());
    assert!(!Action::Pan.is_edit());
    assert!(!Action::Audition.is_edit());
    assert!(Action::MoveNoteNoSnap.ignores_snap());
    assert!(!Action::MoveNote.ignores_snap());
}

// ── contextual zoom (MeMagic) ────────────────────────────────────────

use expression_editor_core::zoom::{self, HorizontalMode, SmartZoom, Span, VerticalMode, ZoomModes};

fn spans_at(starts: &[f64], len: f64, row: i32) -> Vec<Span> {
    starts
        .iter()
        .map(|&s| Span {
            start: s,
            end: s + len,
            row,
        })
        .collect()
}

#[test]
fn smart_zoom_follows_local_density_not_the_item_average() {
    // Sixteenths on the left, whole notes on the right.
    let mut spans = spans_at(&[0.0, 240.0, 480.0, 720.0, 960.0, 1200.0], 240.0, 60);
    spans.extend(spans_at(&[10000.0, 14000.0, 18000.0], 3840.0, 60));
    let cfg = SmartZoom::default();

    let dense = zoom::weighted_note_length(&spans, 600.0, cfg).unwrap();
    let sparse = zoom::weighted_note_length(&spans, 14000.0, cfg).unwrap();
    assert!(
        sparse > dense * 3.0,
        "the sparse passage must zoom out much further: {dense} vs {sparse}"
    );
}

#[test]
fn zero_smoothing_ignores_distance_entirely() {
    let mut spans = spans_at(&[0.0, 240.0, 480.0], 240.0, 60);
    spans.extend(spans_at(&[10000.0, 14000.0], 3840.0, 60));
    let cfg = SmartZoom {
        smoothing: 0.0,
        ..Default::default()
    };
    let a = zoom::weighted_note_length(&spans, 200.0, cfg).unwrap();
    let b = zoom::weighted_note_length(&spans, 14000.0, cfg).unwrap();
    assert!(
        (a - b).abs() < 1e-6,
        "with no smoothing every position sees the same average"
    );
}

#[test]
fn smart_zoom_scales_with_the_requested_note_count() {
    let spans = spans_at(&[0.0, 240.0, 480.0, 720.0], 240.0, 60);
    let ten = zoom::weighted_note_length(
        &spans,
        360.0,
        SmartZoom {
            notes_visible: 10.0,
            ..Default::default()
        },
    )
    .unwrap();
    let twenty = zoom::weighted_note_length(
        &spans,
        360.0,
        SmartZoom {
            notes_visible: 20.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(twenty > ten, "more notes means a wider span");
}

#[test]
fn an_empty_stretch_still_reveals_the_nearest_note() {
    let spans = spans_at(&[0.0], 240.0, 60);
    let cfg = SmartZoom::default();
    // Far away from the only note.
    let span = zoom::weighted_note_length(&spans, 100_000.0, cfg).unwrap();
    assert!(
        span >= 100_000.0 * 2.0,
        "the view must widen enough to show something, got {span}"
    );
}

#[test]
fn no_notes_gives_no_smart_span() {
    assert!(zoom::weighted_note_length(&[], 0.0, SmartZoom::default()).is_none());
}

#[test]
fn cursor_alignment_places_the_anchor_in_the_view() {
    assert_eq!(zoom::align(100.0, 40.0, 0.0), (100.0, 140.0), "left edge");
    assert_eq!(zoom::align(100.0, 40.0, 0.5), (80.0, 120.0), "centred");
    assert_eq!(zoom::align(100.0, 40.0, 1.0), (60.0, 100.0), "right edge");
}

#[test]
fn restricting_to_the_item_slides_the_view_rather_than_squashing_it() {
    let vp = Viewport::new(800.0, 480.0);
    let content = Content {
        t_start: 0.0,
        t_end: 10000.0,
        pitch_lo: 55.0,
        pitch_hi: 67.0,
    };
    let cam = camera::reset_view(content, vp, 0.03, 0.35);
    let spans = spans_at(&[0.0, 240.0, 480.0], 240.0, 60);

    let free = zoom::apply_horizontal(
        cam,
        HorizontalMode::SmartNotes,
        &spans,
        0.0,
        content,
        vp,
        3840.0,
        SmartZoom::default(),
    );
    let clamped = zoom::apply_horizontal(
        cam,
        HorizontalMode::SmartNotesInItem,
        &spans,
        0.0,
        content,
        vp,
        3840.0,
        SmartZoom::default(),
    );
    assert!(free.t0 < content.t_start, "unclamped runs off the item start");
    assert_eq!(clamped.t0, content.t_start, "clamped starts at the item");
    assert!(
        (free.units_per_px - clamped.units_per_px).abs() < 1e-9,
        "clamping must slide the view, not change the zoom level"
    );
}

#[test]
fn vertical_fit_respects_the_row_floor_and_ceiling() {
    let vp = Viewport::new(800.0, 480.0);
    let cam = Camera {
        t0: 0.0,
        units_per_px: 10.0,
        pitch_center: 60.0,
        px_per_semitone: 10.0,
    };
    // One note: without a floor this would fill the screen with a
    // single row.
    let one = spans_at(&[0.0], 240.0, 60);
    let cfg = SmartZoom::default();
    let out = zoom::apply_vertical(
        cam,
        VerticalMode::AllNotes,
        &one,
        60.0,
        vp,
        (0.0, 8000.0),
        cfg,
    );
    assert!(out.px_per_semitone <= cfg.max_px_per_row);
    assert!(
        vp.h / out.px_per_semitone >= cfg.min_rows - 1e-6,
        "at least min_rows must stay visible"
    );
}

#[test]
fn notes_in_view_ignores_notes_outside_the_horizontal_span() {
    let vp = Viewport::new(800.0, 480.0);
    let cam = Camera {
        t0: 0.0,
        units_per_px: 1.0,
        pitch_center: 60.0,
        px_per_semitone: 10.0,
    };
    let mut spans = spans_at(&[0.0], 240.0, 60);
    spans.extend(spans_at(&[50_000.0], 240.0, 100)); // far away, high
    let out = zoom::apply_vertical(
        cam,
        VerticalMode::CenterOfNotesInView,
        &spans,
        60.0,
        vp,
        (0.0, 800.0),
        SmartZoom::default(),
    );
    assert!(
        (out.pitch_center - 60.0).abs() < 1e-6,
        "the offscreen note must not drag the view up"
    );
}

#[test]
fn contextual_modes_zoom_differently_per_pointer_region() {
    // Needs a part longer than the smart span wants, or both modes
    // clamp to the item and there is nothing to tell apart.
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 64.0);
    for i in 0..64 {
        doc.push(Note::new(
            NoteId(i + 1),
            PPQ * i as f64 * 0.5,
            PPQ * (i as f64 * 0.5 + 0.4),
            60 + (i % 5) as i32,
        ));
    }
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));
    let anchor = PPQ * 8.0;

    let mut keys = ed.clone();
    keys.smart_zoom(ZoomModes::KEYS, anchor, 62.0);
    let mut notes = ed.clone();
    notes.smart_zoom(ZoomModes::NOTE_AREA, anchor, 62.0);

    // Over the keys the whole item is framed; over the notes the view
    // hugs the local passage.
    assert!(
        notes.camera.units_per_px < keys.camera.units_per_px,
        "note-area zoom should be tighter than item zoom"
    );

    // The ruler leaves pitch alone entirely.
    let before = ed.camera.px_per_semitone;
    ed.smart_zoom(ZoomModes::RULER, anchor, 62.0);
    assert_eq!(ed.camera.px_per_semitone, before);
}

#[test]
fn smart_zoom_keeps_the_anchor_in_view() {
    let mut ed = test_editor();
    let anchor = PPQ * 2.5;
    ed.smart_zoom(ZoomModes::NOTE_AREA, anchor, 62.0);
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    assert!(
        anchor >= t0 && anchor <= t1,
        "anchor {anchor} fell outside {t0}..{t1}"
    );
}

// ── razor edits ──────────────────────────────────────────────────────

use expression_editor_core::razor::{self, RazorArea, RazorSet};

/// One held note per row, spanning the whole bar — so every test can
/// razor a rectangle out of the middle of real material.
fn held_doc() -> ExpressionDoc {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 16.0);
    for (i, row) in [60, 64, 67].iter().enumerate() {
        let mut n = Note::new(NoteId(i as u64 + 1), 0.0, PPQ * 8.0, *row);
        n.pitch.set(0.0, 0.0);
        n.pitch.set(PPQ * 8.0, 2.0);
        doc.push(n);
    }
    doc
}

#[test]
fn a_razor_slices_notes_at_its_edges_rather_than_selecting_whole_ones() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 4.0, 60, 60);
    let inside = razor::carve(&mut doc, area);

    assert_eq!(inside.len(), 1, "exactly the middle piece");
    let piece = doc.note(inside[0]).unwrap();
    assert!((piece.start - PPQ * 2.0).abs() < 1e-6);
    assert!((piece.end - PPQ * 4.0).abs() < 1e-6);
    // The held note became three: before, inside, after.
    let row60: Vec<_> = doc.notes.iter().filter(|n| n.row == 60).collect();
    assert_eq!(row60.len(), 3);
    // Rows the razor did not cover are untouched.
    assert_eq!(doc.notes.iter().filter(|n| n.row == 64).count(), 1);
}

#[test]
fn slicing_preserves_the_expression_at_the_cut() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 4.0, 60, 60);
    let inside = razor::carve(&mut doc, area);
    let piece = doc.note(inside[0]).unwrap();
    // The pitch curve ramps 0→2 across 8 beats, so at beat 2 it is 0.5.
    assert!(
        (piece.pitch.sample(PPQ * 2.0, 0.0) - 0.5).abs() < 1e-6,
        "the cut must inherit the curve value, not reset to centre"
    );
}

#[test]
fn deleting_an_area_leaves_a_hole_and_keeps_the_rest() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 4.0, 60, 60);
    assert!(razor::delete_contents(&mut doc, area));

    let row60: Vec<_> = doc.notes.iter().filter(|n| n.row == 60).collect();
    assert_eq!(row60.len(), 2, "before and after survive");
    assert!(
        !row60.iter().any(|n| n.start < PPQ * 4.0 && n.end > PPQ * 2.0),
        "nothing is left inside the hole"
    );
}

#[test]
fn moving_an_area_carries_its_contents_and_clears_the_destination() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 3.0, 60, 60);
    assert!(razor::move_contents(&mut doc, area, PPQ * 4.0, 0, false));

    // The moved piece landed at 6..7.
    assert!(
        doc.notes
            .iter()
            .any(|n| n.row == 60 && (n.start - PPQ * 6.0).abs() < 1e-6),
        "the piece moved"
    );
    // And it replaced what was there rather than overlapping it.
    let overlapping = doc
        .notes
        .iter()
        .filter(|n| n.row == 60 && n.start < PPQ * 7.0 && n.end > PPQ * 6.0)
        .count();
    assert_eq!(overlapping, 1, "destination was cleared, not stacked on");
}

#[test]
fn copying_an_area_leaves_the_original_in_place() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 3.0, 60, 60);
    assert!(razor::move_contents(&mut doc, area, PPQ * 4.0, 0, true));

    assert!(
        doc.notes
            .iter()
            .any(|n| n.row == 60 && (n.start - PPQ * 2.0).abs() < 1e-6),
        "original still there"
    );
    assert!(
        doc.notes
            .iter()
            .any(|n| n.row == 60 && (n.start - PPQ * 6.0).abs() < 1e-6),
        "and a copy landed"
    );
}

#[test]
fn a_nudge_does_not_delete_its_own_source() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 4.0, 60, 60);
    // Destination overlaps the source — the classic way to lose data.
    assert!(razor::move_contents(&mut doc, area, PPQ * 0.5, 0, false));
    assert!(
        doc.notes
            .iter()
            .any(|n| n.row == 60 && (n.start - PPQ * 2.5).abs() < 1e-6),
        "the nudged piece survives"
    );
}

#[test]
fn moving_an_area_vertically_transposes_its_contents() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 3.0, 60, 60);
    razor::move_contents(&mut doc, area, 0.0, 5, false);
    assert!(doc.notes.iter().any(|n| n.row == 65));
}

#[test]
fn stretching_an_area_scales_positions_and_lengths_together() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 16.0);
    for i in 0..4 {
        doc.push(Note::new(
            NoteId(i + 1),
            PPQ * i as f64 * 0.25,
            PPQ * (i as f64 * 0.25 + 0.25),
            60,
        ));
    }
    let area = RazorArea::new(0.0, PPQ, 60, 60);
    assert!(razor::stretch_contents(&mut doc, area, 0.0, PPQ * 2.0));

    let mut starts: Vec<f64> = doc.notes.iter().map(|n| n.start).collect();
    starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // Sixteenths became eighths.
    assert!((starts[1] - PPQ * 0.5).abs() < 1e-6, "got {starts:?}");
    let n = doc.notes.iter().find(|n| n.start == 0.0).unwrap();
    assert!((n.len() - PPQ * 0.5).abs() < 1e-6, "lengths scaled too");
}

#[test]
fn reversing_an_area_mirrors_it_about_its_own_centre() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 16.0);
    doc.push(Note::new(NoteId(1), 0.0, PPQ * 0.25, 60));
    doc.push(Note::new(NoteId(2), PPQ * 0.75, PPQ, 60));
    let area = RazorArea::new(0.0, PPQ, 60, 60);
    assert!(razor::reverse_contents(&mut doc, area));

    let mut starts: Vec<f64> = doc.notes.iter().map(|n| n.start).collect();
    starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // The note at 0..0.25 mirrors to 0.75..1, and vice versa.
    assert!((starts[0] - 0.0).abs() < 1e-6, "got {starts:?}");
    assert!((starts[1] - PPQ * 0.75).abs() < 1e-6, "got {starts:?}");
}

#[test]
fn razor_velocity_applies_only_inside_the_rectangle() {
    let mut doc = held_doc();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 4.0, 60, 60);
    assert!(razor::set_velocity(&mut doc, area, 0.25));

    let inside = doc
        .notes
        .iter()
        .find(|n| n.row == 60 && n.start >= PPQ * 2.0 && n.end <= PPQ * 4.0)
        .unwrap();
    assert_eq!(inside.velocity, 0.25);
    // Another row is untouched.
    let other = doc.notes.iter().find(|n| n.row == 64).unwrap();
    assert_ne!(other.velocity, 0.25);
}

#[test]
fn clearing_a_lane_across_an_area_keeps_the_notes() {
    let mut doc = held_doc();
    let before = doc.notes.len();
    let area = RazorArea::new(PPQ * 2.0, PPQ * 4.0, 60, 60);
    assert!(razor::clear_lane(&mut doc, area, Lane::Pitch));
    assert_eq!(doc.notes.len(), before, "notes survive a lane clear");
}

#[test]
fn overlapping_areas_on_the_same_rows_merge() {
    let mut set = RazorSet::default();
    set.add(RazorArea::new(0.0, 100.0, 60, 60));
    set.add(RazorArea::new(50.0, 200.0, 60, 60));
    assert_eq!(set.areas.len(), 1, "merged");
    assert_eq!(set.areas[0].t0, 0.0);
    assert_eq!(set.areas[0].t1, 200.0);

    // Different rows stay separate.
    set.add(RazorArea::new(0.0, 100.0, 64, 64));
    assert_eq!(set.areas.len(), 2);
}

#[test]
fn an_empty_area_is_never_added() {
    let mut set = RazorSet::default();
    set.add(RazorArea::new(100.0, 100.0, 60, 60));
    assert!(set.is_empty(), "a zero-width drag is not a razor");
}

#[test]
fn hit_testing_a_razor_set_finds_the_topmost_area() {
    let mut set = RazorSet::default();
    set.add(RazorArea::new(0.0, 100.0, 60, 62));
    assert!(set.at(50.0, 61).is_some());
    assert!(set.at(50.0, 70).is_none(), "outside the row range");
    assert!(set.at(150.0, 61).is_none(), "outside the time range");
    assert!(set.remove_at(50.0, 61));
    assert!(set.is_empty());
}

#[test]
fn razor_bindings_do_not_steal_the_plain_marquee_drag() {
    let m = MouseMap::reaper_like();
    // Creating a razor is a deliberate modifier gesture.
    assert_eq!(
        m.resolve(Context::PianoRoll, Gesture::Drag, Mods::default()),
        Action::MarqueeSelect
    );
    assert_eq!(
        m.resolve(
            Context::PianoRoll,
            Gesture::Drag,
            Mods {
                shift: true,
                alt: true,
                ..Default::default()
            }
        ),
        Action::RazorCreate
    );
    // And once an area exists, dragging inside it moves its contents.
    assert_eq!(
        m.resolve(Context::RazorArea, Gesture::Drag, Mods::default()),
        Action::RazorMoveContents
    );
    assert_eq!(
        m.resolve(
            Context::RazorArea,
            Gesture::Drag,
            Mods {
                alt: true,
                ..Default::default()
            }
        ),
        Action::RazorCopyContents
    );
}

#[test]
fn no_preset_binds_the_same_gesture_twice() {
    // A duplicate binding is silently unreachable: `resolve` finds the
    // first and the second never fires. The literal tables are hand
    // written, so this invariant needs guarding.
    for name in MouseMap::PRESETS {
        let m = MouseMap::preset(name);
        let mut seen: Vec<(Context, Gesture, ModKey)> = Vec::new();
        for b in m.bindings() {
            let key = (b.context, b.gesture, b.mods);
            assert!(
                !seen.contains(&key),
                "{name}: duplicate binding for {:?} {:?} {}",
                b.context,
                b.gesture,
                b.mods.notation()
            );
            seen.push(key);
        }
    }
}

// ── chords (keyflow-backed) ──────────────────────────────────────────

use expression_editor_core::chord;

#[test]
fn triads_and_sevenths_are_recognised() {
    let name = |pitches: &[i32]| chord::identify(pitches).map(|c| chord::name(&c));
    assert_eq!(name(&[60, 64, 67]).as_deref(), Some("C"));
    assert_eq!(name(&[60, 63, 67]).as_deref(), Some("Cm"));
    // A seventh must not be read as a plain triad.
    assert_eq!(name(&[60, 64, 67, 71]).as_deref(), Some("Cmaj7"));
    assert_eq!(name(&[60, 64, 67, 70]).as_deref(), Some("C7"));
    assert_eq!(name(&[60, 63, 67, 70]).as_deref(), Some("Cm7"));
}

#[test]
fn an_inversion_reads_as_a_slash_chord_not_a_different_chord() {
    // C major with E in the bass.
    let c = chord::identify(&[64, 67, 72]).unwrap();
    assert_eq!(
        chord::name(&c),
        "C/E",
        "first inversion of C is a slash chord, not Em"
    );
}

#[test]
fn a_single_note_is_not_a_chord() {
    assert!(chord::identify(&[60]).is_none());
    assert!(chord::identify(&[]).is_none());
}

#[test]
fn the_chord_box_reads_the_selection() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    for (i, row) in [60, 64, 67].iter().enumerate() {
        doc.push(Note::new(NoteId(i as u64 + 1), 0.0, PPQ * 2.0, *row));
    }
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));

    // Nothing selected and no playhead: nothing to say.
    assert!(ed.current_chord().is_none());

    ed.selection.notes = vec![NoteId(1), NoteId(2), NoteId(3)];
    assert_eq!(ed.chord_pitches(), vec![60, 64, 67]);
    assert_eq!(
        ed.current_chord().map(|c| chord::name(&c)).as_deref(),
        Some("C")
    );
}

#[test]
fn with_nothing_selected_the_box_follows_the_playhead() {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    for (i, row) in [60, 64, 67].iter().enumerate() {
        doc.push(Note::new(NoteId(i as u64 + 1), 0.0, PPQ * 2.0, *row));
    }
    // A note that starts later must not be counted early.
    doc.push(Note::new(NoteId(9), PPQ * 4.0, PPQ * 6.0, 70));
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));
    ed.playhead = Some(PPQ);
    assert_eq!(ed.chord_pitches(), vec![60, 64, 67]);

    // Muted notes are not sounding, so they are not in the chord.
    ed.doc.note_mut(NoteId(2)).unwrap().muted = true;
    assert_eq!(ed.chord_pitches(), vec![60, 67]);
}

#[test]
fn a_string_roll_reports_sounding_pitches_not_string_numbers() {
    let tuning = StringTuning::guitar_standard();
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    doc.row_space = RowSpace::Strings(tuning.clone());
    // An open C major shape on the top three strings: G B E.
    for (i, (string, fret)) in [(3usize, 0u8), (4, 1), (5, 0)].iter().enumerate() {
        let mut n = Note::new(NoteId(i as u64 + 1), 0.0, PPQ, *string as i32);
        n.fret = Some(*fret);
        doc.push(n);
    }
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));
    ed.row_space = RowSpace::Strings(tuning);
    ed.selection.notes = vec![NoteId(1), NoteId(2), NoteId(3)];
    // G=55, C=60, E=64 — the chord box must see pitches, not rows 3/4/5.
    assert_eq!(ed.chord_pitches(), vec![55, 60, 64]);
    assert!(ed.current_chord().is_some());
}
