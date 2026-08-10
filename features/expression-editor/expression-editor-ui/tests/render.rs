//! SSR structure tests — the surface renders, and what it draws
//! follows the document.
//!
//! Plain `VirtualDom` + `dioxus-ssr`, no GPU and no browser, so this
//! runs anywhere. Event dispatch is covered by the core's own suite;
//! this asserts the view actually reaches the screen.

use dioxus::prelude::*;
use expression_editor_core::doc::{ExpressionDoc, Lane, Note, NoteId, TimeBase};
use expression_editor_core::{Editor, Viewport};
use expression_editor_ui::ExpressionEditor;

const PPQ: f64 = 960.0;

fn demo_editor(microtonal: bool, with_zones: bool) -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    for i in 0..4 {
        let mut n = Note::new(
            NoteId(i + 1),
            PPQ * i as f64,
            PPQ * (i as f64 + 0.9),
            60 + (i as i32) * 2,
        );
        n.channel = Some(2 + i as u8);
        // A scoop into the note plus a little vibrato.
        for k in 0..24 {
            let f = k as f64 / 23.0;
            let t = n.start + (n.end - n.start) * f;
            let scoop = -1.5 * (1.0 - f).powi(3);
            let vib = 0.15 * (f * 18.0).sin() * f;
            n.pitch.set(t, scoop + vib);
        }
        if with_zones && i == 1 {
            n.add_split(n.start + (n.end - n.start) * 0.5);
        }
        doc.push(n);
    }
    doc.mark_ambiguity();

    let mut ed = Editor::new(doc, Viewport::new(900.0, 480.0));
    // MPE: the mode that has every control on screen, so the layout
    // tests see the full bar.
    ed.set_mode(expression_editor_core::Mode::Mpe);
    ed.selection.set_single(NoteId(2));
    if microtonal {
        ed.tuning.temperament = expression_editor_core::tuning::RAST.clone();
    }
    ed
}

fn render(ed: Editor) -> String {
    #[component]
    fn Harness(seed: Editor) -> Element {
        let editor = use_signal(|| seed.clone());
        rsx! { ExpressionEditor { editor } }
    }
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { seed: ed });
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

#[test]
fn the_editor_renders_its_toolbar_canvas_and_status_bar() {
    let html = render(demo_editor(false, false));
    assert!(html.contains("<svg"), "the canvas must render");
    // The top bar carries modes; the status bar carries settings.
    assert!(html.contains("Undo"), "history controls");
    assert!(html.contains("Reset view (V)"), "view controls");
    assert!(html.contains("12TET"), "tuning moved to the status bar");
    assert!(html.contains("Chord"), "the chord box renders");
    // Lane controls.
    for lane in Lane::ALL {
        let label = expression_editor_ui::theme::lane_label(lane);
        assert!(html.contains(label), "missing lane control: {label}");
    }
    assert!(
        html.contains("1/16"),
        "the grid readout, now in the status bar"
    );
}

#[test]
fn the_top_bar_follows_the_mode() {
    use expression_editor_core::{Mode, ModeFamily};

    let mut mpe = demo_editor(false, false);
    mpe.set_mode(Mode::Mpe);
    let mpe_html = render(mpe);
    assert!(mpe_html.contains("Spread ch"), "MPE shows channel controls");
    assert!(mpe_html.contains("Pressure"), "and the expression lanes");

    let mut midi = demo_editor(false, false);
    midi.set_mode(Mode::Midi);
    let midi_html = render(midi);
    // Plain MIDI cannot carry per-note pressure, so offering the
    // control would promise an edit the format drops.
    assert!(
        !midi_html.contains("Spread ch"),
        "plain MIDI must not show MPE channel controls"
    );
    assert!(
        !midi_html.contains(">Pressure<"),
        "nor the per-note expression lanes"
    );
    // The mode switcher itself is always present.
    for mode in Mode::ALL {
        assert!(
            midi_html.contains(mode.label()),
            "missing mode: {}",
            mode.label()
        );
    }
    // And it is grouped by family rather than one flat run. The check
    // is positional because that is the whole point of the grouping:
    // every MIDI-family label must come before every audio-family one,
    // so the bar reads "which kind of material" left to right.
    let at = |label: &str| {
        midi_html
            .find(label)
            .unwrap_or_else(|| panic!("missing mode: {label}"))
    };
    let last_midi = ModeFamily::Midi
        .modes()
        .iter()
        .map(|m| at(m.label()))
        .max()
        .unwrap();
    let first_audio = ModeFamily::Audio
        .modes()
        .iter()
        .map(|m| at(m.label()))
        .min()
        .unwrap();
    assert!(
        last_midi < first_audio,
        "the two families must render as separate runs, MIDI first"
    );
}

#[test]
fn every_note_reaches_the_canvas() {
    let ed = demo_editor(false, false);
    let expected = ed.doc.notes.len();
    let html = render(ed);
    // Each note draws one body rect with a pitch-class fill.
    let drawn = expression_editor_ui::theme::PITCH_CLASS
        .iter()
        .map(|c| html.matches(&format!("fill=\"{c}\"")).count())
        .sum::<usize>();
    assert!(
        drawn >= expected,
        "expected at least {expected} note bodies, found {drawn}"
    );
    assert!(html.contains("<polyline"), "pitch curves must render");
}

#[test]
fn a_non_equal_tuning_is_visibly_flagged() {
    // The badge, not the toolbar's preset list — every preset name
    // appears in the dropdown either way.
    const BADGE: &str = "background: #422006";
    let plain = render(demo_editor(false, false));
    assert!(!plain.contains(BADGE), "no badge in 12-TET");

    let tuned = render(demo_editor(true, false));
    assert!(
        tuned.contains(BADGE),
        "a non-equal tuning must always be called out"
    );
    assert!(
        tuned.contains(expression_editor_ui::theme::GOLD),
        "microtonal centers draw in gold"
    );
}

#[test]
fn zone_structure_draws_in_red() {
    let plain = render(demo_editor(false, false));
    let zoned = render(demo_editor(false, true));
    let count = |h: &str| h.matches(expression_editor_ui::theme::ZONE).count();
    assert!(
        count(&zoned) > count(&plain),
        "a Q split must add visible red structure"
    );
}

#[test]
fn the_status_bar_reports_the_selected_notes_analysis() {
    let html = render(demo_editor(false, false));
    assert!(html.contains("1 selected"));
    assert!(html.contains("vib"), "the vibrato readout");
    assert!(html.contains("drift"), "the drift readout");
    assert!(html.contains("Robot"), "the flatten button");
    assert!(html.contains("ch 3"), "the MPE member channel");
}

#[test]
fn an_empty_document_still_renders() {
    let doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 4.0);
    let html = render(Editor::new(doc, Viewport::new(900.0, 480.0)));
    assert!(html.contains("<svg"), "no notes must not mean no canvas");
    assert!(html.contains("0 selected"));
}
