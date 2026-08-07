//! The right-hand inspector.
//!
//! Everything about *the current selection* that does not need to be
//! one click away. The selection row above the roll keeps only the
//! chord and the two blend controls — the things you reach for while
//! shaping a phrase — and the rest lives here, where there is room to
//! label it.
//!
//! Also the home for the controller lanes, because pinning a CC and
//! choosing which one to edit is a per-session decision that wants a
//! list, not a cycling button.

use dioxus::prelude::*;
use expression_editor_core::cc::{standard_name, CC_COLORS};
use expression_editor_core::doc::NoteId;
use expression_editor_core::rows::Articulation;
use expression_editor_core::{blob, chord, tuning, Edit, Editor, RowSpace};

use crate::theme;
use crate::widgets::{CenterSlider, Slider};

fn section(title: &str) -> Element {
    rsx! {
        div {
            style: "font-size: 9px; letter-spacing: 0.09em; text-transform: uppercase; \
                    color: {theme::TEXT_DIM}; padding: 10px 10px 4px;",
            "{title}"
        }
    }
}

fn row(label: &str, value: String) -> Element {
    rsx! {
        div {
            style: "display: flex; justify-content: space-between; align-items: center; \
                    padding: 2px 10px; font-size: 10px; color: {theme::TEXT_DIM};",
            span { "{label}" }
            span {
                style: "font-family: ui-monospace, monospace; color: {theme::TEXT};",
                "{value}"
            }
        }
    }
}

#[component]
pub fn Inspector(editor: Signal<Editor>, open: Signal<bool>) -> Element {
    let mut editor = editor;
    let mut open = open;

    if !open() {
        // A tab, not a hidden panel: the way back has to be visible.
        return rsx! {
            div {
                style: "flex: 0 0 auto; width: 18px; display: flex; align-items: center; \
                        justify-content: center; background: {theme::PANEL}; \
                        border-left: 1px solid {theme::PANEL_BORDER}; cursor: pointer; \
                        color: {theme::TEXT_DIM}; font-size: 10px;",
                onclick: move |_| open.set(true),
                "‹"
            }
        };
    }

    let ed = editor.read();
    let sel: Vec<NoteId> = ed.selection.notes.clone();
    let single = sel.first().copied();
    let note = single.and_then(|id| ed.doc.note(id)).cloned();
    let space = ed.row_space.clone();
    let ups = ed.doc.time_base.units_per_second(ed.bpm);
    let chord_name = ed.current_chord().map(|c| chord::name(&c));
    let lanes = ed.doc.cc.lanes.clone();
    let cc_edit = ed.cc_edit;
    let is_strings = matches!(space, RowSpace::Strings(_));
    drop(ed);

    let decomposition = note
        .as_ref()
        .map(|n| blob::decompose(&n.pitch, n.start, n.end, 64, ups, 0.0));

    rsx! {
        div {
            style: "flex: 0 0 auto; width: 236px; display: flex; flex-direction: column; \
                    background: {theme::PANEL}; overflow-y: auto; \
                    border-left: 1px solid {theme::PANEL_BORDER}; \
                    color: {theme::TEXT}; font-family: system-ui, sans-serif;",

            div {
                style: "display: flex; align-items: center; justify-content: space-between; \
                        padding: 7px 10px; border-bottom: 1px solid {theme::PANEL_BORDER};",
                span {
                    style: "font-size: 10px; letter-spacing: 0.08em; \
                            text-transform: uppercase; color: {theme::TEXT_DIM};",
                    "Inspector"
                }
                button {
                    style: "background: none; border: none; color: {theme::TEXT_DIM}; \
                            cursor: pointer; font-size: 11px;",
                    onclick: move |_| open.set(false),
                    "›"
                }
            }

            // ── selection ────────────────────────────────────────────
            if let Some(n) = note.clone() {
                {section("Note")}
                {row("Pitch", space.row_label(n.row))}
                if let Some(label) = space.note_label(&n) {
                    {row("Label", label)}
                }
                {row("Sounding", tuning::note_name(space.pitch_of(&n)))}
                if let Some(ch) = n.channel {
                    {row("Channel", ch.to_string())}
                }
                if sel.len() > 1 {
                    {row("Selected", format!("{} notes", sel.len()))}
                }
                if let Some(name) = chord_name.clone() {
                    {row("Chord", name)}
                }

                div { style: "padding: 4px 10px 8px;",
                    Slider {
                        label: "Vel".to_string(),
                        value: n.velocity,
                        min: 0.0,
                        max: 1.0,
                        width: 130.0,
                        readout: format!("{:.0}", n.velocity * 127.0),
                        on_change: move |v: f64| {
                            let notes = editor.read().selection.notes.clone();
                            editor.write().apply(&Edit::SetVelocity { notes, velocity: v });
                        },
                    }
                    Slider {
                        label: "Rel".to_string(),
                        value: n.off_velocity,
                        min: 0.0,
                        max: 1.0,
                        width: 130.0,
                        readout: format!("{:.0}", n.off_velocity * 127.0),
                        on_change: move |v: f64| {
                            let notes = editor.read().selection.notes.clone();
                            editor.write().apply(&Edit::SetOffVelocity { notes, velocity: v });
                        },
                    }
                }

                // ── pitch shape ──────────────────────────────────────
                {section("Pitch shape")}
                if let Some(d) = decomposition {
                    {row("Vibrato", format!("{:.0}¢", d.modulation_depth() * 100.0))}
                    {row("Drift", format!("{:+.0}¢", d.drift_extent() * 100.0))}
                }
                div { style: "padding: 4px 10px 8px;",
                    Blend { editor, label: "Drift".to_string(), drift: true }
                    Blend { editor, label: "Vibrato".to_string(), drift: false }
                    button {
                        style: "margin-top: 6px; width: 100%; height: 22px; \
                                background: {theme::CONTROL}; border: 1px solid {theme::PANEL_BORDER}; \
                                border-radius: 5px; color: {theme::TEXT}; font-size: 10px; \
                                cursor: pointer;",
                        title: "Flatten drift and vibrato",
                        onclick: move |_| {
                            let Some(id) = single else { return };
                            let (t0, t1) = {
                                let ed = editor.read();
                                let Some(n) = ed.doc.note(id) else { return };
                                (n.start, n.end)
                            };
                            editor.write().apply(&Edit::ReblendPitch {
                                note: id,
                                t0,
                                t1,
                                drift_amount: 0.0,
                                modulation_amount: 0.0,
                            });
                        },
                        "Robot — flatten both"
                    }
                }

                // ── technique ────────────────────────────────────────
                if is_strings {
                    {section("Technique")}
                    {row("Fret", n.fret.map(|f| f.to_string()).unwrap_or_else(|| "—".into()))}
                    {row("Legato", if n.legato { "yes".into() } else { "no".into() })}
                    div {
                        style: "display: flex; flex-wrap: wrap; gap: 3px; padding: 4px 10px 8px;",
                        for a in Articulation::ALL {
                            button {
                                key: "art{a:?}",
                                style: format!(
                                    "height: 20px; padding: 0 6px; font-size: 9px; \
                                     border-radius: 4px; cursor: pointer; \
                                     border: 1px solid {}; background: {}; color: {};",
                                    if n.articulation == Some(a) { theme::ACCENT } else { theme::PANEL_BORDER },
                                    if n.articulation == Some(a) { theme::CONTROL_ACTIVE } else { theme::SURFACE_INSET },
                                    theme::TEXT,
                                ),
                                title: "{a.label()}",
                                onclick: move |_| {
                                    let notes = editor.read().selection.notes.clone();
                                    let cur = editor
                                        .read()
                                        .doc
                                        .note(notes[0])
                                        .and_then(|n| n.articulation);
                                    // Clicking the active technique
                                    // clears it — otherwise there is no
                                    // way back to plain sustain.
                                    let next = if cur == Some(a) { None } else { Some(a) };
                                    editor.write().apply(&Edit::SetArticulation {
                                        notes,
                                        articulation: next,
                                    });
                                },
                                "{a.label()}"
                            }
                        }
                    }
                }

                // ── zones ────────────────────────────────────────────
                if n.zone_count() > 1 {
                    {section("Q zones")}
                    {row("Count", n.zone_count().to_string())}
                    {row(
                        "Target",
                        match n.target {
                            expression_editor_core::Target::WholeNote => "whole note".into(),
                            expression_editor_core::Target::Zone(i) => format!("zone {}", i + 1),
                        },
                    )}
                }
            } else {
                div {
                    style: "padding: 16px 10px; font-size: 11px; color: {theme::TEXT_DIM}; \
                            text-align: center; line-height: 1.7;",
                    "Nothing selected"
                    br {}
                    span { style: "font-size: 10px;", "Click a note to inspect it" }
                }
            }

            // ── controller lanes ─────────────────────────────────────
            {section("Controller lanes")}
            div {
                style: "display: flex; flex-direction: column; gap: 3px; padding: 0 10px 8px;",
                for lane in lanes.iter() {
                    div {
                        key: "cc{lane.number}",
                        style: format!(
                            "display: flex; align-items: center; gap: 6px; \
                             border: 1px solid {}; border-radius: 5px; padding: 4px 6px; \
                             background: {};",
                            if cc_edit == Some(lane.number) { theme::ACCENT } else { theme::PANEL_BORDER },
                            if cc_edit == Some(lane.number) { theme::CONTROL_SELECTED } else { theme::SURFACE_INSET },
                        ),
                        // The colour swatch is how a background curve is
                        // matched to its lane, so it has to be here.
                        div {
                            style: format!(
                                "width: 10px; height: 10px; border-radius: 2px; \
                                 background: {}; flex: 0 0 auto;",
                                CC_COLORS[lane.color % CC_COLORS.len()],
                            ),
                        }
                        span {
                            style: "flex: 1; font-size: 10px;",
                            "CC{lane.number} {lane.name}"
                        }
                        button {
                            style: format!(
                                "background: none; border: none; cursor: pointer; \
                                 font-size: 11px; color: {};",
                                if lane.pinned { theme::ACCENT } else { theme::BORDER_STRONG },
                            ),
                            title: "Pin behind the roll",
                            onclick: {
                                let number = lane.number;
                                move |_| { editor.write().doc.cc.toggle_pin(number); }
                            },
                            if lane.pinned { "◉" } else { "○" }
                        }
                        button {
                            style: format!(
                                "background: none; border: none; cursor: pointer; \
                                 font-size: 10px; color: {};",
                                if cc_edit == Some(lane.number) { theme::ACCENT } else { theme::TEXT_DIM },
                            ),
                            title: "Edit this controller on the roll",
                            onclick: {
                                let number = lane.number;
                                move |_| {
                                    let mut e = editor;
                                    if e.read().cc_edit == Some(number) {
                                        e.write().exit_cc_edit();
                                    } else {
                                        e.write().edit_cc(number);
                                    }
                                }
                            },
                            "✎"
                        }
                    }
                }
            }
            // The controllers an orchestral part actually rides.
            div {
                style: "display: flex; flex-wrap: wrap; gap: 3px; padding: 0 10px 12px;",
                for number in [1u8, 11, 2, 7, 10, 64] {
                    button {
                        key: "add{number}",
                        style: "height: 20px; padding: 0 6px; font-size: 9px; \
                                border: 1px solid {theme::PANEL_BORDER}; border-radius: 4px; \
                                background: {theme::SURFACE_INSET}; color: {theme::TEXT_DIM}; cursor: pointer;",
                        title: "Add CC{number} ({standard_name(number)})",
                        onclick: move |_| { editor.write().doc.cc.ensure(number); },
                        "+CC{number}"
                    }
                }
            }
        }
    }
}

/// One of the drift/vibrato sliders.
///
/// Parked at 1.0 rather than tracking a stored amount: each change
/// re-decomposes the *current* curve, so the control is a relative
/// scale on what is there now and never fights an earlier edit.
#[component]
fn Blend(editor: Signal<Editor>, label: String, drift: bool) -> Element {
    let mut editor = editor;
    let mut amount = use_signal(|| 1.0f64);
    rsx! {
        CenterSlider {
            label: label.clone(),
            value: amount(),
            min: 0.0,
            max: 1.5,
            center: 1.0,
            width: 130.0,
            readout: format!("{:.0}%", amount() * 100.0),
            on_change: move |v: f64| {
                amount.set(v);
                let Some(id) = editor.read().selection.notes.first().copied() else {
                    return;
                };
                let (t0, t1) = {
                    let ed = editor.read();
                    let Some(n) = ed.doc.note(id) else { return };
                    (n.start, n.end)
                };
                editor.write().apply(&Edit::ReblendPitch {
                    note: id,
                    t0,
                    t1,
                    drift_amount: if drift { v } else { 1.0 },
                    modulation_amount: if drift { 1.0 } else { v },
                });
            },
        }
    }
}
