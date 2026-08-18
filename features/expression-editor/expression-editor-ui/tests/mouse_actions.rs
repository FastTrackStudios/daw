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

// ── the razor as a tool ─────────────────────────────────────────────

/// Arming the razor puts on the plain drag what `Ctrl` puts on a
/// modified one.
///
/// The razor was reachable only through a modifier, which is fine for
/// one cut and tiring for ten. As a tool it is the same gesture through
/// the same code path — worth asserting, because two spellings of one
/// action is exactly how the two drift apart.
#[test]
fn the_razor_tool_sweeps_an_area_on_a_plain_drag() {
    let mut ed = four_notes();
    ed.tool = expression_editor_core::Tool::Razor;
    let (x0, y) = at(&ed, PPQ * 1.0);
    let (x1, _) = at(&ed, PPQ * 3.0);

    let mut d = interaction::pointer_down(&mut ed, x0, y, plain(), 0);
    assert!(
        matches!(d, Drag::RazorCreate { .. }),
        "the razor tool did not claim the plain drag",
    );
    interaction::pointer_move(&mut ed, &mut d, x1, y, plain());
    interaction::pointer_up(&mut ed, d, x1, y, plain());

    assert_eq!(ed.razor.areas.len(), 1, "the sweep committed no area");
    let a = ed.razor.areas[0];
    assert!(
        (a.t0 - PPQ).abs() < 1.0 && (a.t1 - PPQ * 3.0).abs() < 1.0,
        "the area landed at {}..{} beats",
        a.t0 / PPQ,
        a.t1 / PPQ,
    );
}

/// A razor drag that starts *on* a note cuts rather than picking it up.
///
/// The whole point of the tool is sweeping across material. If a drag
/// beginning over a note moved the note instead, every cut would have to
/// start in a gap — which on dense material is nowhere.
#[test]
fn the_razor_tool_cuts_across_notes_rather_than_moving_them() {
    let mut ed = four_notes();
    ed.tool = expression_editor_core::Tool::Razor;
    let before = span(&ed, NoteId(2));

    // Start the drag squarely inside note 2.
    let (x0, y) = at(&ed, PPQ * 1.25);
    let (x1, _) = at(&ed, PPQ * 3.25);
    let mut d = interaction::pointer_down(&mut ed, x0, y, plain(), 0);
    assert!(
        matches!(d, Drag::RazorCreate { .. }),
        "a drag starting on a note did not reach the razor",
    );
    interaction::pointer_move(&mut ed, &mut d, x1, y, plain());
    interaction::pointer_up(&mut ed, d, x1, y, plain());

    assert_eq!(
        span(&ed, NoteId(2)),
        before,
        "the razor moved the note it started on",
    );
    assert_eq!(ed.razor.areas.len(), 1, "no area was swept");
}

/// The rectangle drawn during the sweep is the one that gets committed.
///
/// This is the bug the preview exists to prevent rather than to reveal:
/// a razor used to appear only on release, so the surface could not be
/// wrong about it — it simply said nothing. Now that it draws, the value
/// it draws and the value it commits have to be the same one, or the
/// preview becomes a lie that is worse than the silence it replaced.
#[test]
fn the_previewed_area_is_the_committed_area() {
    let mut ed = four_notes();
    ed.tool = expression_editor_core::Tool::Razor;
    // Snapping on, so preview and commit have something to disagree
    // about — with a free grid both readings land in the same place.
    ed.grid.enabled = true;

    let (x0, y) = at(&ed, PPQ * 1.1);
    let (x1, _) = at(&ed, PPQ * 2.9);
    let mut d = interaction::pointer_down(&mut ed, x0, y, plain(), 0);
    interaction::pointer_move(&mut ed, &mut d, x1, y, plain());

    let previewed = match d {
        Drag::RazorCreate { pending, .. } => pending.expect("a sweep this wide has an area"),
        _ => panic!("not a razor drag"),
    };
    interaction::pointer_up(&mut ed, d, x1, y, plain());

    assert_eq!(
        ed.razor.areas.as_slice(),
        &[previewed],
        "the committed area is not the one that was drawn",
    );
}

/// A click is not a sweep.
///
/// A press and release in one spot used to commit a hairline razor:
/// invisible, and still able to swallow the next operation that asked
/// what areas were active.
#[test]
fn a_click_with_the_razor_leaves_no_area() {
    let mut ed = four_notes();
    ed.tool = expression_editor_core::Tool::Razor;
    let (x, y) = at(&ed, PPQ * 1.5);

    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    // A pixel of tremor, which is what a real click has.
    interaction::pointer_move(&mut ed, &mut d, x + 1.0, y, plain());
    interaction::pointer_up(&mut ed, d, x + 1.0, y, plain());

    assert!(
        ed.razor.is_empty(),
        "a click left {} area(s) behind",
        ed.razor.areas.len(),
    );
}

// ── handles work on the selection ───────────────────────────────────

/// Two selected notes, each with a little pitch contour of its own.
fn two_selected_with_contours() -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    for (i, row) in [(0u64, 60), (1, 64)] {
        let mut n = Note::new(NoteId(i + 1), PPQ * i as f64 * 2.0, PPQ * (i as f64 * 2.0 + 2.0), row);
        // Different contours, so a gesture that flattens both onto one
        // shared value cannot pass by looking like a shift.
        for k in 0..32 {
            let f = k as f64 / 31.0;
            let t = n.start + (n.end - n.start) * f;
            n.pitch.set(t, if i == 0 { f * 0.4 } else { -f * 0.2 });
        }
        doc.push(n);
    }
    let mut ed = Editor::new(doc, Viewport::new(1000.0, 480.0));
    ed.mode = expression_editor_core::Mode::Vocals;
    ed.reset_view();
    ed.snap_pitch = false;
    ed.selection.notes = vec![NoteId(1), NoteId(2)];
    ed
}

fn sampled(ed: &Editor, id: NoteId) -> f64 {
    let n = ed.doc.note(id).expect("note is live");
    n.row as f64 + n.pitch.sample((n.start + n.end) * 0.5, 0.0)
}

/// Dragging a handle on one selected note moves every selected note.
///
/// The handles were the last gesture on this surface that ignored the
/// selection: you could select a phrase, grab a handle to shape it, and
/// shape exactly one note. Every other gesture — move, stretch, velocity
/// — already worked on the selection, so this was the odd one out rather
/// than a deliberate exception.
#[test]
fn a_handle_drag_moves_the_whole_selection() {
    let mut ed = two_selected_with_contours();
    let before = (sampled(&ed, NoteId(1)), sampled(&ed, NoteId(2)));

    // Grab the fine-pitch handle on note 1 — whichever one the geometry
    // layer actually put on screen, rather than a guessed rectangle.
    let sets = expression_editor_ui::canvas::note_handles(&ed);
    let set = sets
        .iter()
        .find(|s| s.id == NoteId(1))
        .expect("the selected note has handles");
    let r = set
        .rects
        .iter()
        .find(|r| r.handle == expression_editor_core::handles::Handle::FinePitch)
        .expect("a note this wide carries the fine-pitch handle");
    let (x, y) = (r.x + r.w * 0.5, r.y + r.h * 0.5);

    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    assert!(
        matches!(d, Drag::Handle(_)),
        "the press did not open a handle drag",
    );
    interaction::pointer_move(&mut ed, &mut d, x, y - 40.0, plain());
    interaction::pointer_up(&mut ed, d, x, y - 40.0, plain());

    let after = (sampled(&ed, NoteId(1)), sampled(&ed, NoteId(2)));
    assert!(
        (after.0 - before.0).abs() > 1e-6,
        "the grabbed note did not move",
    );
    assert!(
        (after.1 - before.1).abs() > 1e-6,
        "the other selected note was left behind: {} -> {}",
        before.1,
        after.1,
    );
    // The same delta, not the same value: each note keeps what it was
    // shaped to and rises from there.
    assert!(
        ((after.0 - before.0) - (after.1 - before.1)).abs() < 1e-6,
        "the two notes moved by different amounts: {} vs {}",
        after.0 - before.0,
        after.1 - before.1,
    );
}

/// An unselected note is not dragged along by a handle on another one.
#[test]
fn a_handle_drag_leaves_unselected_notes_alone() {
    let mut ed = two_selected_with_contours();
    ed.selection.notes = vec![NoteId(1)];
    let before = sampled(&ed, NoteId(2));

    let sets = expression_editor_ui::canvas::note_handles(&ed);
    let set = sets.iter().find(|s| s.id == NoteId(1)).expect("handles");
    let r = set
        .rects
        .iter()
        .find(|r| r.handle == expression_editor_core::handles::Handle::FinePitch)
        .expect("fine-pitch handle");
    let (x, y) = (r.x + r.w * 0.5, r.y + r.h * 0.5);

    let mut d = interaction::pointer_down(&mut ed, x, y, plain(), 0);
    interaction::pointer_move(&mut ed, &mut d, x, y - 40.0, plain());
    interaction::pointer_up(&mut ed, d, x, y - 40.0, plain());

    assert!(
        (sampled(&ed, NoteId(2)) - before).abs() < 1e-9,
        "an unselected note followed the drag",
    );
}

// ── dragging a razor's contents ─────────────────────────────────────

/// A drag is many moves, and the result must match the one move it adds
/// up to.
///
/// This is the bug that made razor dragging unusable. Moving a razor's
/// contents *carves* — it splits notes at the area boundaries and clears
/// the ground it lands on — so it cannot be re-run against a document it
/// has already carved. It was: every frame re-split the original
/// rectangle, which by then held whatever had drifted into it rather
/// than the material that started there. The notes that moved first
/// stopped moving, notes that were never selected got dragged along, and
/// the take came apart.
///
/// One long step and forty small ones now agree, which is the property a
/// live drag needs and the one that was missing.
#[test]
fn dragging_a_razor_in_many_steps_lands_where_one_step_would() {
    let one_step = {
        let mut ed = four_notes();
        ed.razor
            .add(RazorArea::new(0.0, PPQ * 1.5, ROW - 1, ROW + 1));
        // The middle of the area, not its edge: a press near a boundary
        // is `Context::RazorEdge` and resizes instead, which is a
        // different gesture and would leave this testing nothing.
        let (x0, y) = at(&ed, PPQ * 0.75);
        let (x1, _) = at(&ed, PPQ * 4.75);
        ed.mouse.set(
            Context::RazorArea,
            Gesture::Drag,
            ModKey::NONE,
            Action::RazorMoveContents,
        );
        let mut d = interaction::pointer_down(&mut ed, x0, y, plain(), 0);
        assert!(matches!(d, Drag::RazorDrag { .. }), "not a razor move");
        interaction::pointer_move(&mut ed, &mut d, x1, y, plain());
        interaction::pointer_up(&mut ed, d, x1, y, plain());
        ed
    };

    let many_steps = {
        let mut ed = four_notes();
        ed.razor
            .add(RazorArea::new(0.0, PPQ * 1.5, ROW - 1, ROW + 1));
        let (x0, y) = at(&ed, PPQ * 0.75);
        let (x1, _) = at(&ed, PPQ * 4.75);
        ed.mouse.set(
            Context::RazorArea,
            Gesture::Drag,
            ModKey::NONE,
            Action::RazorMoveContents,
        );
        let mut d = interaction::pointer_down(&mut ed, x0, y, plain(), 0);
        assert!(matches!(d, Drag::RazorDrag { .. }), "not a razor move");
        // Forty frames of a real drag, which is where the damage
        // accumulated.
        for i in 1..=40 {
            let t = i as f64 / 40.0;
            interaction::pointer_move(&mut ed, &mut d, x0 + (x1 - x0) * t, y, plain());
        }
        interaction::pointer_up(&mut ed, d, x1, y, plain());
        ed
    };

    let describe = |ed: &Editor| {
        let mut v: Vec<(i64, i64, i32)> = ed
            .doc
            .notes
            .iter()
            .map(|n| (n.start.round() as i64, n.end.round() as i64, n.row))
            .collect();
        v.sort_unstable();
        v
    };

    assert_eq!(
        describe(&many_steps),
        describe(&one_step),
        "a smooth drag and a single jump disagreed — the drag was \
         accumulating carves",
    );
    // And it did not shred: the same number of notes as a single move
    // produces, not a pile of fragments.
    assert!(
        many_steps.doc.notes.len() <= four_notes().doc.notes.len() + 1,
        "the drag left {} notes behind, from {}",
        many_steps.doc.notes.len(),
        four_notes().doc.notes.len(),
    );
}

/// Reverting to recompute must not eat the undo step.
#[test]
fn a_razor_drag_is_still_one_undo() {
    let mut ed = four_notes();
    let before: Vec<_> = ed.doc.notes.iter().map(|n| (n.start, n.row)).collect();
    ed.razor
        .add(RazorArea::new(0.0, PPQ * 1.5, ROW - 1, ROW + 1));
    ed.mouse.set(
        Context::RazorArea,
        Gesture::Drag,
        ModKey::NONE,
        Action::RazorMoveContents,
    );

    let (x0, y) = at(&ed, PPQ * 0.75);
    let (x1, _) = at(&ed, PPQ * 4.75);
    let mut d = interaction::pointer_down(&mut ed, x0, y, plain(), 0);
    assert!(matches!(d, Drag::RazorDrag { .. }), "not a razor move");
    for i in 1..=10 {
        let t = i as f64 / 10.0;
        interaction::pointer_move(&mut ed, &mut d, x0 + (x1 - x0) * t, y, plain());
    }
    interaction::pointer_up(&mut ed, d, x1, y, plain());

    assert!(ed.undo(), "the drag opened no undo step");
    let after: Vec<_> = ed.doc.notes.iter().map(|n| (n.start, n.row)).collect();
    assert_eq!(after, before, "one undo did not put the material back");
}

// ── razor mode ──────────────────────────────────────────────────────
//
// The verb table from MRE's `MRE_CMD.pdf` (see `spec/midi-editor.md`).
// Driven through `key_down`, because the claim is that the *keys* reach
// them — the operations themselves are `razor::`'s and tested there.

/// A razor over the first two notes, with the tool armed.
fn razor_mode(area_end: f64) -> Editor {
    let mut ed = four_notes();
    ed.tool = expression_editor_core::Tool::Razor;
    ed.razor.add(RazorArea::new(0.0, area_end, ROW - 2, ROW + 2));
    ed
}

fn press(ed: &mut Editor, key: &str) -> bool {
    interaction::key_down(ed, &Drag::None, key, plain())
}

fn press_ctrl(ed: &mut Editor, key: &str) -> bool {
    let mods = Mods {
        ctrl: true,
        ..Default::default()
    };
    interaction::key_down(ed, &Drag::None, key, mods)
}

fn starts(ed: &Editor) -> Vec<i64> {
    let mut v: Vec<i64> = ed.doc.notes.iter().map(|n| n.start.round() as i64).collect();
    v.sort_unstable();
    v
}

/// `R` retrogrades; `Ctrl+R` reverses the pitches and keeps the rhythm.
///
/// MRE's split, and the one that makes them two commands rather than
/// one: retrograde rewrites the phrase, `Ctrl+R` reharmonises a groove
/// that already works.
#[test]
fn r_retrogrades_and_ctrl_r_keeps_the_rhythm() {
    // Two notes at different pitches, both *inside* the area — the
    // rectangle is ROW-2..ROW+2, and a note outside it is not material
    // either of these operates on.
    let mut ed = razor_mode(PPQ * 2.0);
    ed.doc.notes[1].row = ROW + 2;
    let rhythm = starts(&ed);
    let rows_before: Vec<i32> = ed.doc.notes.iter().map(|n| n.row).collect();

    assert!(press_ctrl(&mut ed, "r"), "Ctrl+R did nothing");
    assert_eq!(starts(&ed), rhythm, "Ctrl+R moved something in time");
    let rows_after: Vec<i32> = ed.doc.notes.iter().map(|n| n.row).collect();
    assert_ne!(rows_after, rows_before, "Ctrl+R left the pitches alone");

    // Plain R is the one that moves material in time.
    let mut ed = razor_mode(PPQ * 2.0);
    assert!(press(&mut ed, "r"), "R did nothing");
    assert_ne!(starts(&ed), rhythm, "R left the rhythm untouched");
}

/// `V` inverts about the material's own centre, so twice is identity.
#[test]
fn v_inverts_and_inverting_twice_is_where_you_started() {
    let mut ed = razor_mode(PPQ * 2.0);
    ed.doc.notes[1].row = ROW + 2;
    let before: Vec<i32> = ed.doc.notes.iter().map(|n| n.row).collect();

    assert!(press(&mut ed, "v"), "V did nothing");
    let once: Vec<i32> = ed.doc.notes.iter().map(|n| n.row).collect();
    assert_ne!(once, before, "V did not move any pitch");

    assert!(press(&mut ed, "v"));
    let twice: Vec<i32> = ed.doc.notes.iter().map(|n| n.row).collect();
    assert_eq!(twice, before, "inverting twice did not come back");
}

/// `X` deletes the contents, `S` selects them, `U` puts them back out.
#[test]
fn x_deletes_and_s_and_u_move_the_selection() {
    let mut ed = razor_mode(PPQ * 2.0);
    assert!(press(&mut ed, "s"), "S selected nothing");
    assert!(!ed.selection.is_empty(), "S left the selection empty");
    assert!(
        !ed.razor.is_empty(),
        "S dropped the areas — selecting is a step towards another \
         razor operation, not a way of finishing with it",
    );

    assert!(press(&mut ed, "u"), "U did nothing");
    assert!(ed.selection.is_empty(), "U left notes selected");

    let before = ed.doc.notes.len();
    assert!(press(&mut ed, "x"), "X deleted nothing");
    assert!(ed.doc.notes.len() < before, "X left every note in place");
    assert!(!ed.razor.is_empty(), "X dropped the areas");
}

/// `F` makes the area full-range, so it stops being about pitch.
#[test]
fn f_makes_the_area_cover_every_row() {
    let mut ed = razor_mode(PPQ * 2.0);
    assert!(press(&mut ed, "f"), "F did nothing");
    let a = ed.razor.areas[0];
    assert_eq!((a.row_lo, a.row_hi), (0, 127), "F did not go full-range");
}

/// The sticky modes toggle, and each key is its own way out.
#[test]
fn the_sticky_modes_toggle_off_again() {
    use expression_editor_core::razor::RazorAxis;
    let mut ed = razor_mode(PPQ * 2.0);

    press(&mut ed, "i");
    assert!(ed.razor_insert, "I did not arm insert mode");
    press(&mut ed, "i");
    assert!(!ed.razor_insert, "I did not turn itself off");

    press(&mut ed, "h");
    assert_eq!(ed.razor_axis, Some(RazorAxis::Horizontal));
    // Mutually exclusive: L replaces H rather than joining it.
    press(&mut ed, "l");
    assert_eq!(ed.razor_axis, Some(RazorAxis::Vertical));
    press(&mut ed, "l");
    assert_eq!(ed.razor_axis, None, "L did not turn itself off");
}

/// Razor mode takes only its own letters.
///
/// A mode that swallowed every key would cost you undo the moment you
/// picked up the razor. Only the verbs are claimed.
#[test]
fn razor_mode_leaves_the_ordinary_keys_alone() {
    let mut ed = razor_mode(PPQ * 2.0);
    let before = ed.doc.notes.len();
    press(&mut ed, "x");
    assert!(ed.doc.notes.len() < before);

    // Undo is not a razor verb, and still works.
    assert!(press_ctrl(&mut ed, "z"), "Ctrl+Z was swallowed by razor mode");
    assert_eq!(ed.doc.notes.len(), before, "undo did not restore the notes");
}

/// And the letters mean tools again once the razor is put down.
#[test]
fn the_verbs_are_only_live_while_the_razor_is() {
    let mut ed = four_notes();
    ed.tool = expression_editor_core::Tool::Razor;
    // Armed, but nothing drawn: there is nothing for a verb to act on,
    // so the letters go back to being tool shortcuts.
    press(&mut ed, "s");
    assert_eq!(
        ed.tool,
        expression_editor_core::Tool::Select,
        "`s` did not fall through to the Select shortcut",
    );
}

/// Escape backs out one step at a time: areas first, then the mode.
#[test]
fn escape_drops_the_areas_then_the_mode() {
    let mut ed = razor_mode(PPQ * 2.0);
    ed.razor_insert = true;

    assert!(press(&mut ed, "Escape"), "the first Escape did nothing");
    assert!(ed.razor.is_empty(), "the first Escape kept the areas");
    assert_eq!(
        ed.tool,
        expression_editor_core::Tool::Razor,
        "the first Escape also left the mode — that is two intentions",
    );

    assert!(press(&mut ed, "Escape"), "the second Escape did nothing");
    assert_eq!(ed.tool, expression_editor_core::Tool::Select);
    assert!(!ed.razor_insert, "the sticky mode outlived the razor");
}

/// Delete acts on the razor, not on whatever was selected before it.
#[test]
fn delete_takes_the_razor_over_the_selection() {
    let mut ed = razor_mode(PPQ * 2.0);
    // A note outside the area is selected, as it would be if you had
    // been working normally and then reached for the razor.
    ed.selection.notes = vec![NoteId(4)];
    let outside = span(&ed, NoteId(4));

    assert!(press(&mut ed, "Delete"), "Delete did nothing");
    assert_eq!(
        maybe_span(&ed, NoteId(4)),
        Some(outside),
        "Delete took the selection instead of the razor",
    );
}

/// The arrows nudge the areas, and Shift resizes instead.
#[test]
fn the_arrows_move_and_resize_the_areas() {
    let mut ed = razor_mode(PPQ * 2.0);
    ed.grid.enabled = true;
    let before = ed.razor.areas[0];

    press(&mut ed, "ArrowRight");
    let moved = ed.razor.areas[0];
    assert!(moved.t0 > before.t0, "Right did not move the area");
    assert!(
        (moved.width() - before.width()).abs() < 1.0,
        "a move changed the area's width",
    );

    let mods = Mods {
        shift: true,
        ..Default::default()
    };
    interaction::key_down(&mut ed, &Drag::None, "ArrowRight", mods);
    let resized = ed.razor.areas[0];
    assert!(resized.width() > moved.width(), "Shift+Right did not resize");
    assert_eq!(resized.t0, moved.t0, "a resize moved the area's start");
}
