//! Every action the mouse map can name actually does something.
//!
//! The bug these exist for is not a crash — it is a gesture that reads
//! correctly in the map, draws a correct cursor, and then performs
//! something else. `run_action` used to end in a `_ => None` arm, so an
//! action nobody had written a case for fell through to the legacy path
//! and quietly did whatever *that* did. Ten shipped bindings were in
//! that state, including `StretchNotes` on Shift+edge and the Riffer
//! preset's primary note drag.
//!
//! Driven through `interaction::pointer_down` and friends rather than a
//! mounted DOM: the claim here is about the action table, and a headless
//! Blitz render would only make the failure slower to read. The DOM path
//! is covered by `tools.rs`.
//!
//! Most of these actions are not in a shipped preset — they are the
//! vocabulary a user or a host binds *to*. So each test binds the one it
//! is about, which is also exactly how it would be reached in practice.

use expression_editor_core::doc::{ExpressionDoc, Note, NoteId, TimeBase};
use expression_editor_core::mouse::{Action, Context, Gesture, ModKey};
use expression_editor_core::tools::Mods;
use expression_editor_core::{Editor, RazorArea, Viewport};
use expression_editor_ui::interaction::{self, Drag};

const PPQ: f64 = 960.0;
const ROW: i32 = 60;

/// Four notes on one row, a beat apart, each half a beat long.
fn four_notes() -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    for i in 0..4u64 {
        doc.push(Note::new(
            NoteId(i + 1),
            PPQ * i as f64,
            PPQ * i as f64 + PPQ * 0.5,
            ROW,
        ));
    }
    let mut ed = Editor::new(doc, Viewport::new(1000.0, 480.0));
    ed.reset_view();
    // Off, so the assertions are about the gesture and not about which
    // grid line it landed on.
    ed.grid.enabled = false;
    ed
}

fn plain() -> Mods {
    Mods::default()
}

fn at(ed: &Editor, t: f64) -> (f64, f64) {
    (ed.camera.x(t), ed.camera.y(ROW as f64, ed.viewport))
}

fn span(ed: &Editor, id: NoteId) -> (f64, f64) {
    let n = ed.doc.note(id).expect("note is live");
    (n.start, n.end)
}

/// As [`span`], but tolerant of a note that no longer exists.
///
/// A razor move *carves*, so a note that straddled the area boundary is
/// split and the original id is gone. That is the gesture working, not
/// the test failing.
fn maybe_span(ed: &Editor, id: NoteId) -> Option<(f64, f64)> {
    ed.doc.note(id).map(|n| (n.start, n.end))
}

/// How many pixels a row is worth, so a vertical drag can be stated in
/// rows rather than in a pixel count that means different things at
/// different zooms — `reset_view` fits the content, and this fixture's
/// content is one row tall.
fn row_px(ed: &Editor) -> f64 {
    ed.camera.vertical.px_per_row
}

/// Bind `action` and run a drag from `t0` to `t1` on the note row.
fn drag(ed: &mut Editor, context: Context, action: Action, t0: f64, t1: f64) -> Drag {
    ed.mouse.set(context, Gesture::Drag, ModKey::NONE, action);
    let (x0, y) = at(ed, t0);
    let (x1, _) = at(ed, t1);
    let mut d = interaction::pointer_down(ed, x0, y, plain(), 0);
    interaction::pointer_move(ed, &mut d, x1, y, plain());
    interaction::pointer_up(ed, d, x1, y, plain())
}

// ── stretching ──────────────────────────────────────────────────────

#[test]
fn stretching_scales_the_whole_selection_about_the_far_end() {
    let mut ed = four_notes();
    ed.selection.notes = vec![NoteId(1), NoteId(2), NoteId(3), NoteId(4)];
    // Grab the last note's end and pull it out to twice its distance
    // from the first note's start.
    let pivot = PPQ * 0.0;
    let grabbed = PPQ * 3.5;
    drag(
        &mut ed,
        Context::NoteEdge,
        Action::StretchNotes,
        grabbed,
        pivot + (grabbed - pivot) * 2.0,
    );

    // The pivot end has not moved, and everything else is twice as far
    // from it — lengths included.
    assert_eq!(span(&ed, NoteId(1)).0, pivot, "the pivot moved");
    let (s4, e4) = span(&ed, NoteId(4));
    assert!(
        (s4 - PPQ * 6.0).abs() < 1.0,
        "note 4 should start at 6 beats, started at {}",
        s4 / PPQ,
    );
    assert!(
        (e4 - PPQ * 7.0).abs() < 1.0,
        "note 4 should end at 7 beats, ended at {}",
        e4 / PPQ,
    );
}

#[test]
fn stretching_positions_leaves_lengths_alone() {
    let mut ed = four_notes();
    ed.selection.notes = vec![NoteId(1), NoteId(2), NoteId(3), NoteId(4)];
    let before = span(&ed, NoteId(4));
    let len = before.1 - before.0;
    let grabbed = PPQ * 3.5;
    drag(
        &mut ed,
        Context::NoteEdge,
        Action::StretchNotePositions,
        grabbed,
        grabbed * 2.0,
    );

    let (s4, e4) = span(&ed, NoteId(4));
    assert!(s4 > before.0, "positions did not spread");
    assert!(
        (e4 - s4 - len).abs() < 1.0,
        "arpeggiate changed a note's length: {} -> {}",
        len,
        e4 - s4,
    );
}

/// The distinction that makes them two actions rather than one.
#[test]
fn the_two_stretches_disagree() {
    let mut lengths = four_notes();
    let mut positions = four_notes();
    for ed in [&mut lengths, &mut positions] {
        ed.selection.notes = vec![NoteId(1), NoteId(4)];
    }
    drag(
        &mut lengths,
        Context::NoteEdge,
        Action::StretchNotes,
        PPQ * 3.5,
        PPQ * 7.0,
    );
    drag(
        &mut positions,
        Context::NoteEdge,
        Action::StretchNotePositions,
        PPQ * 3.5,
        PPQ * 7.0,
    );
    assert_ne!(span(&lengths, NoteId(4)), span(&positions, NoteId(4)));
}

// ── axis locking ────────────────────────────────────────────────────

/// Both axes were live for every move action until `Axis` existed, so
/// this is the regression that mattered most: the Riffer preset's plain
/// note drag is `MoveNoteVertically`, and it moved notes in time.
#[test]
fn a_vertical_move_does_not_travel_in_time() {
    let mut ed = four_notes();
    let before = span(&ed, NoteId(2));
    ed.mouse
        .set(Context::Note, Gesture::Drag, ModKey::NONE, Action::MoveNoteVertically);

    let (x, y) = at(&ed, PPQ * 1.25);
    // Three whole rows: this fixture is one row tall, so `reset_view`
    // leaves a row worth hundreds of pixels and a fixed 60px drag
    // rounds to no transposition at all.
    let dy = row_px(&ed) * 3.0;
    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    // A long diagonal: plenty of horizontal travel to leak through.
    interaction::pointer_move(&mut ed, &mut d, x + 240.0, y - dy, plain());
    interaction::pointer_up(&mut ed, d, x + 240.0, y - dy, plain());

    assert_eq!(
        span(&ed, NoteId(2)),
        before,
        "a vertical move changed the note's timing",
    );
    assert_ne!(
        ed.doc.note(NoteId(2)).unwrap().row,
        ROW,
        "a vertical move did not change the note's pitch",
    );
}

#[test]
fn a_horizontal_move_does_not_change_pitch() {
    let mut ed = four_notes();
    ed.mouse.set(
        Context::Note,
        Gesture::Drag,
        ModKey::NONE,
        Action::MoveNoteHorizontally,
    );
    let (x, y) = at(&ed, PPQ * 1.25);
    let dy = row_px(&ed) * 3.0;
    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    interaction::pointer_move(&mut ed, &mut d, x + 240.0, y - dy, plain());
    interaction::pointer_up(&mut ed, d, x + 240.0, y - dy, plain());

    assert_eq!(
        ed.doc.note(NoteId(2)).unwrap().row,
        ROW,
        "a horizontal move transposed the note",
    );
    assert_ne!(
        span(&ed, NoteId(2)).0,
        PPQ,
        "a horizontal move did not move the note",
    );
}

/// `MoveNoteOneAxis` commits to whichever axis the drag opens along, and
/// then holds it — which is the behaviour Shift has claimed since the
/// map was written and never had.
#[test]
fn one_axis_commits_to_the_first_direction() {
    for (dx, rows, moves_in_time) in [(200.0, 0.02, true), (8.0, -3.0, false)] {
        let mut ed = four_notes();
        ed.mouse
            .set(Context::Note, Gesture::Drag, ModKey::NONE, Action::MoveNoteOneAxis);
        let (x, y) = at(&ed, PPQ * 1.25);
        let dy = row_px(&ed) * rows;
        let far = row_px(&ed) * 3.0;
        let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
        // Commit along the dominant axis first, then wander.
        interaction::pointer_move(&mut ed, &mut d, x + dx, y + dy, plain());
        interaction::pointer_move(&mut ed, &mut d, x + 200.0, y - far, plain());
        interaction::pointer_up(&mut ed, d, x + 200.0, y - far, plain());

        let moved_time = (span(&ed, NoteId(2)).0 - PPQ).abs() > 1.0;
        let moved_pitch = ed.doc.note(NoteId(2)).unwrap().row != ROW;
        assert_eq!(
            moved_time, moves_in_time,
            "commit to the wrong axis for delta ({dx}, {dy})",
        );
        assert_eq!(moved_pitch, !moves_in_time);
    }
}

#[test]
fn moving_one_note_can_ignore_the_selection() {
    let mut ed = four_notes();
    ed.selection.notes = vec![NoteId(1), NoteId(2), NoteId(3), NoteId(4)];
    let untouched = span(&ed, NoteId(1));
    ed.mouse.set(
        Context::Note,
        Gesture::Drag,
        ModKey::NONE,
        Action::MoveNoteIgnoringSelection,
    );
    let (x, y) = at(&ed, PPQ * 1.25);
    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    interaction::pointer_move(&mut ed, &mut d, x + 120.0, y, plain());
    interaction::pointer_up(&mut ed, d, x + 120.0, y, plain());

    assert_ne!(span(&ed, NoteId(2)), (PPQ, PPQ * 1.5), "the grabbed note stayed put");
    assert_eq!(span(&ed, NoteId(1)), untouched, "the rest of the selection moved");
}

// ── razor ───────────────────────────────────────────────────────────

fn with_razor() -> Editor {
    let mut ed = four_notes();
    ed.razor
        .add(RazorArea::new(PPQ * 0.5, PPQ * 2.5, ROW - 2, ROW + 2));
    ed
}

#[test]
fn dragging_a_razor_edge_resizes_it() {
    let mut ed = with_razor();
    let before = ed.razor.areas[0];
    drag(
        &mut ed,
        Context::RazorEdge,
        Action::RazorResizeArea,
        PPQ * 2.5,
        PPQ * 3.5,
    );
    let after = ed.razor.areas[0];
    assert_eq!(after.t0, before.t0, "the fixed edge moved");
    assert!(after.t1 > before.t1, "the dragged edge did not move");
}

/// Moving the rectangle without its contents is what makes a razor a
/// repositionable *selection*. It was bound to Shift and did nothing.
#[test]
fn an_area_only_move_leaves_the_notes_behind() {
    let mut ed = with_razor();
    let before: Vec<(f64, f64)> = (1..=4).map(|i| span(&ed, NoteId(i))).collect();
    drag(
        &mut ed,
        Context::RazorArea,
        Action::RazorMoveAreaOnly,
        PPQ * 1.5,
        PPQ * 4.0,
    );

    let after: Vec<(f64, f64)> = (1..=4).map(|i| span(&ed, NoteId(i))).collect();
    assert_eq!(after, before, "an area-only move carried the notes with it");
    assert!(
        ed.razor.areas[0].t0 > PPQ * 0.5,
        "the area itself did not move",
    );
}

/// The contrast that gives the previous test its meaning.
#[test]
fn an_ordinary_razor_move_does_carry_the_notes() {
    let mut ed = with_razor();
    let before: Vec<Option<(f64, f64)>> = (1..=4).map(|i| maybe_span(&ed, NoteId(i))).collect();
    drag(
        &mut ed,
        Context::RazorArea,
        Action::RazorMoveContents,
        PPQ * 1.5,
        PPQ * 4.0,
    );
    let after: Vec<Option<(f64, f64)>> = (1..=4).map(|i| maybe_span(&ed, NoteId(i))).collect();
    assert_ne!(after, before, "a razor move left every note where it was");
}

// ── selection and the rest ──────────────────────────────────────────

#[test]
fn a_touch_sweep_selects_what_it_crosses() {
    // Bound in both contexts, because a sweep has to survive starting
    // on a note as readily as on open roll — the context is resolved at
    // the press, and a sweep bound only to `PianoRoll` dies the moment
    // it begins over one.
    let mut ed = four_notes();
    for context in [Context::PianoRoll, Context::Note] {
        ed.mouse
            .set(context, Gesture::Drag, ModKey::NONE, Action::SelectTouched);
    }
    // The note under the press counts as touched.
    let (px, py) = at(&ed, PPQ * 0.25);
    let d = interaction::pointer_down(&mut ed, px, py, plain(), 0);
    interaction::pointer_up(&mut ed, d, px, py, plain());
    assert!(
        ed.selection.contains(NoteId(1)),
        "the note under the press was not touched",
    );

    // And a sweep across the row picks up the rest.
    let mut ed = four_notes();
    for context in [Context::PianoRoll, Context::Note] {
        ed.mouse
            .set(context, Gesture::Drag, ModKey::NONE, Action::SelectTouched);
    }
    let (x, y) = at(&ed, PPQ * 0.25);
    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    for i in 1..=12 {
        let (sx, _) = at(&ed, PPQ * 0.25 + PPQ * 0.3 * i as f64);
        interaction::pointer_move(&mut ed, &mut d, sx, y, plain());
    }
    interaction::pointer_up(&mut ed, d, x, y, plain());
    assert!(
        ed.selection.notes.len() >= 3,
        "a sweep across four notes selected {}",
        ed.selection.notes.len(),
    );
}

#[test]
fn selecting_a_measure_takes_the_measure() {
    let mut ed = four_notes();
    ed.mouse.set(
        Context::PianoRoll,
        Gesture::Click,
        ModKey::NONE,
        Action::SelectAllInMeasure,
    );
    let (x, y) = at(&ed, PPQ * 0.25);
    let _ = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    assert!(
        !ed.selection.notes.is_empty(),
        "selecting a measure selected nothing",
    );
}

#[test]
fn nudging_the_channel_moves_it() {
    let mut ed = four_notes();
    ed.selection.notes = vec![NoteId(2)];
    let before = ed.doc.note(NoteId(2)).unwrap().channel;
    ed.mouse.set(
        Context::Note,
        Gesture::Drag,
        ModKey::NONE,
        Action::SetNoteChannelHigher,
    );
    let (x, y) = at(&ed, PPQ * 1.25);
    let _ = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    assert_ne!(
        ed.doc.note(NoteId(2)).unwrap().channel,
        before,
        "the channel did not move",
    );
}

/// The whole point of the Lyrics preset, which bound this to
/// double-click and had nothing behind it.
#[test]
fn editing_a_lyric_arms_the_field() {
    let mut ed = four_notes();
    ed.mouse.set(
        Context::Note,
        Gesture::Drag,
        ModKey::NONE,
        Action::EditLyric,
    );
    let (x, y) = at(&ed, PPQ * 1.25);
    let _ = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    assert_eq!(
        ed.editing_lyric,
        Some(NoteId(2)),
        "double-clicking a note did not open its lyric",
    );
}

#[test]
fn sweeping_the_eraser_deletes_what_it_crosses() {
    let mut ed = four_notes();
    ed.mouse
        .set(Context::Note, Gesture::Drag, ModKey::NONE, Action::EraseNotes);
    let (x, y) = at(&ed, PPQ * 1.25);
    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    assert!(ed.doc.note(NoteId(2)).is_none(), "the pressed note survived");
    for i in 1..=12 {
        let (sx, _) = at(&ed, PPQ * 1.25 + PPQ * 0.25 * i as f64);
        interaction::pointer_move(&mut ed, &mut d, sx, y, plain());
    }
    interaction::pointer_up(&mut ed, d, x, y, plain());
    assert!(
        ed.doc.notes.len() < 3,
        "the sweep left {} notes",
        ed.doc.notes.len(),
    );
}

#[test]
fn anchored_zoom_keeps_the_time_under_the_pointer() {
    let mut ed = four_notes();
    // Both contexts: zooming is navigation, and navigation must not
    // depend on the pointer having found open roll first.
    for context in [Context::PianoRoll, Context::Note, Context::NoteEdge] {
        ed.mouse
            .set(context, Gesture::Drag, ModKey::NONE, Action::ZoomAnchored);
    }
    let (x, y) = at(&ed, PPQ * 3.0);
    let before = ed.camera.units_per_px;

    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    interaction::pointer_move(&mut ed, &mut d, x, y - 100.0, plain());
    interaction::pointer_up(&mut ed, d, x, y - 100.0, plain());

    assert!(ed.camera.units_per_px < before, "the drag did not zoom in");
    assert!(
        (ed.camera.t_at(x) - PPQ * 3.0).abs() < PPQ * 0.05,
        "the anchor drifted to {}",
        ed.camera.t_at(x) / PPQ,
    );
}

// ── the zoom tool ───────────────────────────────────────────────────

/// The point you grabbed stays under the pointer, wherever you grabbed.
///
/// A zoom that reframes about a fixed point is only honest if the fixed
/// point is the one you chose. This got written as
/// `centre = anchor - (y - h/2)/ppr` where the camera reads
/// `slot = centre + (h/2 - y)/ppr` — the same expression negated, so it
/// was exact at the vertical centre and wrong in proportion to the
/// distance from it. Every test that grabbed mid-height passed, and the
/// surface lurched for anyone who grabbed near an edge.
///
/// So this asserts at three heights, and the two off-centre ones are the
/// whole point.
#[test]
fn a_zoom_holds_the_point_it_was_started_on() {
    for grab in [0.2, 0.5, 0.85] {
        let mut ed = four_notes();
        ed.tool = expression_editor_core::Tool::Zoom;
        // A keyboard's worth of rows on screen, rather than the single
        // row `reset_view` fits this fixture to. The bug scales with
        // pixels-per-row: at the fixture's default zoom one row is most
        // of the window and the error lands inside a row, which is a
        // test that only just notices. This is the geometry a user has.
        ed.camera.vertical.px_per_row = 10.0;
        ed.camera.vertical.center = ROW as f64;
        let (x, h) = (400.0, ed.viewport.h);
        let y = h * grab;

        let want_t = ed.camera.t_at(x);
        let want_row = ed.camera.pitch_at(y, ed.viewport);

        // Up and to the right: zoom in on both axes at once, so neither
        // can be right by accident of the other not having moved.
        let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
        assert!(
            matches!(d, Drag::ZoomTool { .. }),
            "the zoom tool did not claim the drag at height {grab}",
        );
        interaction::pointer_move(&mut ed, &mut d, x + 120.0, y - 90.0, plain());

        let got_t = ed.camera.t_at(x);
        let got_row = ed.camera.pitch_at(y, ed.viewport);
        let row_tol = 0.5;

        assert!(
            (got_row - want_row).abs() < row_tol,
            "grabbing at {grab} of the height moved the row under the \
             pointer by {:.2} rows (wanted {want_row:.2}, got {got_row:.2})",
            got_row - want_row,
        );
        assert!(
            (got_t - want_t).abs() < ed.camera.units_per_px,
            "grabbing at {grab} of the height moved the time under the \
             pointer by {:.2} units",
            got_t - want_t,
        );

        // And it did zoom — an anchor that holds because nothing
        // happened would pass everything above.
        assert!(
            ed.camera.vertical.px_per_row > 10.0,
            "the drag never zoomed in vertically at height {grab}",
        );
    }
}
