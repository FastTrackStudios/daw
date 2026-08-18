//! The painted cursor: what it resolves to, and that it reaches screen.
//!
//! Two halves, deliberately. [`cursor_at`] is geometry — "is the pointer
//! on the end of that note?" — and is tested directly against a camera,
//! because a DOM test cannot say *why* a glyph was wrong. The mount
//! tests then pin the things only a real DOM can answer: that the layer
//! exists, that it moves, and that it is transparent to the pointer.
//!
//! The last one is the whole risk of drawing your own cursor. A layer
//! that sits under the mouse and eats its events replaces every gesture
//! on the surface with nothing, and it would look exactly like a cursor
//! that works.

use dioxus::prelude::*;
use dioxus_test::{by_testid, render};
use expression_editor_core::cursor::Cursor;
use expression_editor_core::doc::{ExpressionDoc, Note, NoteId, TimeBase};
use expression_editor_core::tools::Mods;
use expression_editor_core::{Editor, Mode, MouseMap, Tool, Viewport};
use expression_editor_ui::cursor::cursor_at;
use expression_editor_ui::ExpressionEditor;

const PPQ: f64 = 960.0;

const NOTE_START: f64 = PPQ;
const NOTE_END: f64 = PPQ * 2.0;
const NOTE_ROW: i32 = 60;

/// One note in the middle of a four-beat window, so both of its ends are
/// comfortably on screen with empty roll either side.
fn one_note() -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 4.0);
    doc.push(Note::new(NoteId(1), NOTE_START, NOTE_END, NOTE_ROW));
    let mut ed = Editor::new(doc, Viewport::new(900.0, 480.0));
    ed.reset_view();
    ed
}

fn plain() -> Mods {
    Mods::default()
}

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

/// Roll-local coordinates of a point on the note.
fn at(ed: &Editor, t: f64) -> (f64, f64) {
    (ed.camera.x(t), ed.camera.y(NOTE_ROW as f64, ed.viewport))
}

#[test]
fn the_ends_of_a_note_are_brackets() {
    let ed = one_note();
    let (x0, y) = at(&ed, NOTE_START);
    let (x1, _) = at(&ed, NOTE_END);
    assert_eq!(cursor_at(&ed, x0, y, plain(), false), Cursor::EdgeLeft);
    assert_eq!(cursor_at(&ed, x1, y, plain(), false), Cursor::EdgeRight);
}

#[test]
fn the_body_of_a_note_moves_and_the_roll_selects() {
    let ed = one_note();
    let (x, y) = at(&ed, (NOTE_START + NOTE_END) * 0.5);
    assert_eq!(cursor_at(&ed, x, y, plain(), false), Cursor::Move);

    // Well clear of the note, and clear of its edge tolerance.
    let (empty_x, _) = at(&ed, PPQ * 3.5);
    assert_eq!(
        cursor_at(&ed, empty_x, y, plain(), false),
        Cursor::Crosshair,
    );
}

/// The bracket has to flip *within* one note, which is the case a test
/// that only ever looks at one end cannot see.
#[test]
fn the_bracket_flips_across_the_note() {
    let ed = one_note();
    let (x0, y) = at(&ed, NOTE_START);
    let (x1, _) = at(&ed, NOTE_END);
    assert_ne!(
        cursor_at(&ed, x0, y, plain(), false),
        cursor_at(&ed, x1, y, plain(), false),
    );
}

#[test]
fn modifiers_are_previewed_without_moving() {
    let ed = one_note();
    let (x, y) = at(&ed, (NOTE_START + NOTE_END) * 0.5);
    // Same pixel, different held key, different answer — which is what
    // makes it a preview of the gesture rather than a readout of where
    // the mouse is.
    assert_eq!(cursor_at(&ed, x, y, plain(), false), Cursor::Move);
    assert_eq!(cursor_at(&ed, x, y, alt(), false), Cursor::Copy);
}

#[test]
fn the_armed_tool_shows_through() {
    let mut ed = one_note();
    let (x, y) = at(&ed, PPQ * 3.5);
    assert_eq!(cursor_at(&ed, x, y, plain(), false), Cursor::Crosshair);
    ed.tool = Tool::Pen;
    assert_eq!(cursor_at(&ed, x, y, plain(), false), Cursor::Pencil);
    ed.tool = Tool::NoteErase;
    assert_eq!(cursor_at(&ed, x, y, plain(), false), Cursor::NoteEraser);
}

/// The mode preset reaches the cursor through the map, with neither
/// knowing about the other.
#[test]
fn a_drum_roll_shows_a_brush() {
    let mut ed = one_note();
    ed.mouse = MouseMap::drums();
    let (x, y) = at(&ed, PPQ * 3.5);
    assert_eq!(cursor_at(&ed, x, y, plain(), false), Cursor::Brush);
}

/// A handle is drawn in front of the note and pressed before it, so it
/// has to be resolved before it too.
#[test]
fn handles_take_the_cursor_where_a_mode_has_them() {
    let mut ed = one_note();
    ed.mode = Mode::Vocals;
    assert!(ed.mode.has_handles());
    // Handles are laid out only on the *selected* notes — Vovious draws
    // them on what you are working on, not on every note at once. So the
    // cursor cannot claim one on an unselected note either.
    let (bx, by) = at(&ed, (NOTE_START + NOTE_END) * 0.5);
    assert!(
        expression_editor_ui::canvas::note_handles(&ed).is_empty(),
        "an unselected note laid out handles",
    );
    assert!(
        !matches!(cursor_at(&ed, bx, by, plain(), false), Cursor::Handle(_)),
        "an unselected note offered a handle cursor",
    );
    ed.selection.set_single(NoteId(1));
    // Aimed at the real laid-out rects rather than a guess at where the
    // strip is: the layout drops handles on short notes, so a computed
    // offset would silently be testing the note body instead.
    let sets = expression_editor_ui::canvas::note_handles(&ed);
    let rect = sets
        .iter()
        .flat_map(|s| s.rects.iter())
        .find(|r| r.handle != expression_editor_core::Handle::Pitch)
        .expect("a vocal-mode note lays out its strip handles");
    let (x, y) = rect.center();
    let glyph = cursor_at(&ed, x, y, plain(), false);
    assert_eq!(
        glyph,
        Cursor::Handle(rect.handle),
        "the {:?} handle's own rect resolved to {glyph:?}",
        rect.handle,
    );
}

/// While the drawer holds a target, editing is blocked — and the cursor
/// has to say so before the click that does nothing.
#[test]
fn a_locked_surface_says_so() {
    let ed = one_note();
    let (x, y) = at(&ed, (NOTE_START + NOTE_END) * 0.5);
    assert_eq!(cursor_at(&ed, x, y, plain(), true), Cursor::Forbidden);
}

/// Navigation survives the lock: only *editing* is blocked, and a cursor
/// that forbade panning would be describing a restriction that is not
/// there.
#[test]
fn a_locked_surface_still_pans() {
    let ed = one_note();
    let (x, y) = at(&ed, PPQ * 3.5);
    let pan = Mods {
        shift: true,
        ctrl: true,
        alt: false,
    };
    assert_eq!(cursor_at(&ed, x, y, pan, true), Cursor::Hand);
}

// ── on the real DOM ─────────────────────────────────────────────────

#[component]
fn Surface() -> Element {
    let editor = use_signal(one_note);
    use_hook(|| expression_editor_ui::available_space(1000.0, 620.0));
    rsx! { ExpressionEditor { editor } }
}

/// Nothing is *drawn* until the pointer arrives — a glyph parked at the
/// origin on mount is a second cursor on screen.
///
/// The layer itself is mounted from the first render regardless, and
/// must be: `CustomWidgetAttr` is write-once, so an `<object>` created
/// on a later render gets no widget, lays out 0x0, and is skipped by
/// blitz-paint without a word. Hiding is a style, not a mount.
#[tokio::test]
async fn there_is_no_cursor_before_the_pointer_arrives() -> dioxus_test::Result<()> {
    let tester = render(Surface).with_window_size(1000, 620).build();
    tester.query(by_testid("roll")).immediately()?;
    let el = tester.query(by_testid("cursor")).immediately()?;
    assert_eq!(
        el.attribute("data-cursor").as_deref(),
        Some("none"),
        "a glyph was resolved before the pointer had been over the roll",
    );
    assert!(
        el.attribute("style").unwrap_or_default().contains("visibility: hidden"),
        "the untouched cursor layer is visible",
    );
    Ok(())
}

#[tokio::test]
async fn the_cursor_appears_and_follows_the_pointer() -> dioxus_test::Result<()> {
    let tester = render(Surface).with_window_size(1000, 620).build();
    let roll = tester.query(by_testid("roll")).immediately()?;
    let (ox, oy) = roll.document_origin();
    let (w, h) = roll.size();

    // Read back the inline style rather than the laid-out box: the
    // layer is what *places* the glyph, and its placement is the claim.
    // A widget `<object>`'s resolved origin is the renderer's business
    // and reports (0, 0) here whatever the style says.
    let placement = |t: &dioxus_test::DocumentTester| -> dioxus_test::Result<String> {
        Ok(t.query(by_testid("cursor"))
            .immediately()?
            .attribute("style")
            .unwrap_or_default())
    };

    tester.pointer_move(ox + w as f64 * 0.3, oy + h as f64 * 0.5, false);
    tester.drain();
    let first = placement(&tester)?;

    tester.pointer_move(ox + w as f64 * 0.6, oy + h as f64 * 0.7, false);
    tester.drain();
    let second = placement(&tester)?;

    assert!(
        first.contains("left:") && first.contains("top:"),
        "the cursor layer is not positioned at all: {first}",
    );
    assert_ne!(
        first, second,
        "the cursor did not follow the pointer",
    );
    Ok(())
}

/// The one that matters. A cursor layer that is not transparent to the
/// pointer silently disables the entire surface, and every glyph would
/// still look right while it did.
#[tokio::test]
async fn the_cursor_layer_does_not_swallow_gestures() -> dioxus_test::Result<()> {
    let tester = render(Surface).with_window_size(1000, 620).build();
    let roll = tester.query(by_testid("roll")).immediately()?;
    let (ox, oy) = roll.document_origin();
    let (w, h) = roll.size();
    let y = oy + h as f64 * 0.5;

    // Move first, so the layer is mounted and sitting under the pointer
    // for the press that follows — which is precisely the arrangement
    // that would eat it.
    tester.pointer_move(ox + w as f64 * 0.2, y, false);
    tester.drain();
    tester.query(by_testid("cursor")).immediately()?;

    tester.pointer_down(ox + w as f64 * 0.2, y);
    tester.drain();
    for i in 1..=4 {
        tester.pointer_move(ox + w as f64 * (0.2 + 0.1 * i as f64), y - i as f64 * 3.0, true);
        tester.drain();
    }
    tester.pointer_up(ox + w as f64 * 0.6, y - 12.0);
    let _ = tester.pump().await;
    Ok(())
}

/// blitz-paint skips a widget whose box is zero, silently — the same
/// trap the roll's own `<object>` carries a comment about. A cursor
/// layer that mounts, positions itself and paints nothing looks exactly
/// like a cursor that works, right up until you run the app.
#[tokio::test]
async fn the_cursor_layer_has_a_box() -> dioxus_test::Result<()> {
    let tester = render(Surface).with_window_size(1000, 620).build();
    let roll = tester.query(by_testid("roll")).immediately()?;
    let (ox, oy) = roll.document_origin();
    let (w, h) = roll.size();

    tester.pointer_move(ox + w as f64 * 0.3, oy + h as f64 * 0.5, false);
    tester.drain();

    // Size, not markup: `outer_html` does not serialize a custom-widget
    // attribute at all (the roll's own object does not show one either),
    // so the box is the only evidence the widget actually arrived.
    let el = tester.query(by_testid("cursor")).immediately()?;
    let (cw, ch) = el.size();
    assert!(
        cw > 0.0 && ch > 0.0,
        "the cursor layer laid out with no area ({cw}x{ch}); blitz-paint skips it",
    );
    Ok(())
}

/// The glyph reaches actual pixels.
///
/// The box test above proves the layer is laid out; it cannot prove the
/// widget was ever asked to paint, and a scene that is built and never
/// replayed looks identical from the DOM. So this renders the surface
/// twice through the CPU rasterizer — once with the pointer away from
/// the roll, once with it over a note — and requires the two images to
/// differ.
#[tokio::test]
async fn the_glyph_reaches_pixels() -> dioxus_test::Result<()> {
    let dir = std::env::temp_dir().join("ee-cursor-pixels");
    std::fs::create_dir_all(&dir).ok();
    let away = dir.join("away.png");
    let over = dir.join("over.png");

    let tester = render(Surface).with_window_size(1000, 620).build();
    let roll = tester.query(by_testid("roll")).immediately()?;
    let (ox, oy) = roll.document_origin();
    let (w, h) = roll.size();
    tester.render_png(&away);

    tester.pointer_move(ox + w as f64 * 0.4, oy + h as f64 * 0.5, false);
    tester.drain();
    let _ = tester.pump().await;
    tester.render_png(&over);

    println!(
        "SceneWidget paints: {}",
        expression_editor_ui::roll_widget::SCENE_PAINTS
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    let a = std::fs::read(&away).expect("baseline png");
    let b = std::fs::read(&over).expect("hovered png");
    assert_ne!(
        a, b,
        "hovering the roll changed no pixels — the cursor layer never painted",
    );
    Ok(())
}
