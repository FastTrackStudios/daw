//! `expression-editor-ui` — the Dioxus surface for the expression
//! editor.
//!
//! One component renders in three places without changing: standalone,
//! as a VST3/CLAP editor through `nice-plug-dioxus` → Blitz, and in the
//! browser via the wasm build. That is why every style here is inline
//! and why the root component takes no props it cannot get from a
//! signal — a stylesheet reference or a desktop-only launcher would
//! break one of the three.
//!
//! State lives in [`expression_editor_core::Editor`], owned by the host
//! as a `Signal`. The component mutates view state (camera, tool,
//! selection) directly and the document only through `Editor::apply`,
//! so undo stays honest no matter which gesture produced the change.

use dioxus::prelude::*;
use dioxus_elements::input_data::MouseButton;
use keyboard_types::Modifiers;
use expression_editor_core::tools::Mods;
use expression_editor_core::{Editor, Lane, Viewport};

pub mod canvas;
pub mod interaction;
pub mod theme;
pub mod toolbar;

pub use expression_editor_core as core;
pub use interaction::Drag;

/// The editor: toolbar over canvas.
///
/// The host owns `editor` so it can read the document back out — to a
/// MIDI take, or to an offline render job — without the component
/// needing to know which domain it is serving.
#[component]
pub fn ExpressionEditor(editor: Signal<Editor>) -> Element {
    let drag = use_signal(Drag::default);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; width: 100%; height: 100%; \
                    min-height: 0; background: {theme::BG}; color: {theme::TEXT}; \
                    font-family: system-ui, sans-serif;",
            toolbar::Toolbar { editor, drag }
            Canvas { editor, drag }
            StatusBar { editor }
        }
    }
}

/// Pointer coordinates relative to the canvas element.
fn local(e: &PointerEvent) -> (f64, f64) {
    let c = e.data().element_coordinates();
    (c.x, c.y)
}

fn mods_of(m: Modifiers) -> Mods {
    Mods {
        // Cmd and Ctrl are the same gesture; normalizing here means no
        // handler downstream has to know what platform it is on.
        ctrl: m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META),
        shift: m.contains(Modifiers::SHIFT),
        alt: m.contains(Modifiers::ALT),
    }
}

#[component]
fn Canvas(editor: Signal<Editor>, drag: Signal<Drag>) -> Element {
    let mut editor = editor;
    let mut drag = drag;

    let ed = editor.read();
    let vp = ed.viewport;
    let rows = canvas::rows(&ed);
    let grid = canvas::grid_lines(&ed);
    let notes = canvas::note_rects(&ed);
    let curves = canvas::curve_paths(&ed);
    let boxes = canvas::lane_boxes(&ed);
    let guides = canvas::tuning_guides(&ed);
    let zone_guides = canvas::zone_guides(&ed);
    let microtonal = !ed.tuning.temperament.is_equal();
    let temperament_name = ed.tuning.temperament.name;
    let lane = ed.lane;
    drop(ed);

    let marquee = match &*drag.read() {
        Drag::Marquee {
            origin, current, ..
        } => Some((
            origin.0.min(current.0),
            origin.1.min(current.1),
            (current.0 - origin.0).abs(),
            (current.1 - origin.1).abs(),
        )),
        _ => None,
    };

    rsx! {
        div {
            style: "position: relative; flex: 1; min-height: 0; outline: none;",
            tabindex: "0",
            onkeydown: move |e: KeyboardEvent| {
                let key = e.key().to_string();
                let m = mods_of(e.modifiers());
                let d = drag.read().clone();
                if interaction::key_down(&mut editor.write(), &d, &key, m) {
                    e.prevent_default();
                }
            },
            svg {
                style: "width: 100%; height: 100%; display: block; \
                        touch-action: none; user-select: none; cursor: crosshair;",
                view_box: "0 0 {vp.w:.0} {vp.h:.0}",
                preserve_aspect_ratio: "none",
                onmounted: move |e| {
                    let data = e.data();
                    spawn(async move {
                        if let Ok(r) = data.get_client_rect().await {
                            editor.write().resize(Viewport::new(r.width(), r.height()));
                        }
                    });
                },
                onpointerdown: move |e: PointerEvent| {
                    let (x, y) = local(&e);
                    let m = mods_of(e.modifiers());
                    let button = if e.trigger_button()
                        == Some(MouseButton::Secondary)
                    {
                        2
                    } else {
                        0
                    };
                    let d = interaction::pointer_down(&mut editor.write(), x, y, m, button);
                    drag.set(d);
                },
                onpointermove: move |e: PointerEvent| {
                    if !drag.read().is_active() {
                        return;
                    }
                    let (x, y) = local(&e);
                    let m = mods_of(e.modifiers());
                    let mut d = drag.write();
                    interaction::pointer_move(&mut editor.write(), &mut d, x, y, m);
                },
                onpointerup: move |e: PointerEvent| {
                    let (x, y) = local(&e);
                    let m = mods_of(e.modifiers());
                    let d = drag.read().clone();
                    let next = interaction::pointer_up(&mut editor.write(), d, x, y, m);
                    drag.set(next);
                },
                onwheel: move |e: WheelEvent| {
                    let delta = e.delta().strip_units();
                    let m = mods_of(e.modifiers());
                    // A wheel event carries no pointer position, so
                    // zoom anchors on the canvas center until we track
                    // the last pointer move.
                    let (x, y) = (vp.w * 0.5, vp.h * 0.5);
                    interaction::wheel(&mut editor.write(), x, y, delta.x, delta.y, m);
                    e.prevent_default();
                },

                rect { x: "0", y: "0", width: "{vp.w:.0}", height: "{vp.h:.0}", fill: theme::BG }

                // Piano-roll rows.
                for r in rows.iter() {
                    rect {
                        key: "row{r.row}",
                        x: "0",
                        y: "{r.y:.1}",
                        width: "{vp.w:.0}",
                        height: "{r.h:.2}",
                        fill: r.fill,
                    }
                }
                for r in rows.iter().filter(|r| r.is_c) {
                    line {
                        key: "c{r.row}",
                        x1: "0",
                        y1: "{r.y:.1}",
                        x2: "{vp.w:.0}",
                        y2: "{r.y:.1}",
                        stroke: theme::OCTAVE_LINE,
                        stroke_width: "1",
                    }
                }

                // Local grid.
                for (i, gl) in grid.iter().enumerate() {
                    line {
                        key: "g{i}",
                        x1: "{gl.x:.1}",
                        y1: "0",
                        x2: "{gl.x:.1}",
                        y2: "{vp.h:.0}",
                        stroke: if gl.beat { theme::GRID_BEAT } else { theme::GRID_SUB },
                        stroke_width: "1",
                    }
                }

                // Microtonal centers.
                for (i, tg) in guides.iter().enumerate() {
                    g {
                        key: "tg{i}",
                        line {
                            x1: "0",
                            y1: "{tg.y:.1}",
                            x2: "{vp.w:.0}",
                            y2: "{tg.y:.1}",
                            stroke: theme::GOLD,
                            stroke_width: "1",
                            stroke_opacity: "0.45",
                            stroke_dasharray: "4 4",
                        }
                        text {
                            x: "4",
                            y: "{tg.y - 3.0:.1}",
                            fill: theme::GOLD,
                            font_size: "9",
                            opacity: "0.8",
                            "{tg.label}"
                        }
                    }
                }

                // Pressure/Timbre editing boxes.
                for (i, b) in boxes.iter().enumerate() {
                    rect {
                        key: "lb{i}",
                        x: "{b.x:.1}",
                        y: "{b.y:.1}",
                        width: "{b.w:.1}",
                        height: "{b.h:.1}",
                        fill: theme::lane_color(lane),
                        fill_opacity: "0.05",
                        stroke: theme::lane_color(lane),
                        stroke_opacity: "0.25",
                        stroke_width: "1",
                    }
                }

                // Notes.
                for n in notes.iter() {
                    g {
                        key: "n{n.id.0}",
                        rect {
                            x: "{n.x:.1}",
                            y: "{n.y:.1}",
                            width: "{n.w:.1}",
                            height: "{n.h:.1}",
                            rx: "{(n.h * 0.28).min(4.0):.1}",
                            fill: n.fill,
                            fill_opacity: "{n.opacity:.2}",
                            // Ambiguous ownership is called out in red:
                            // the writer refuses to guess which note
                            // owns shared-channel expression.
                            stroke: if n.ambiguous {
                                theme::ZONE
                            } else if n.selected {
                                theme::SELECTED
                            } else {
                                n.fill
                            },
                            stroke_width: if n.ambiguous || n.selected { "2" } else { "1" },
                        }
                        // Q-zone structure: red anchors, active zones
                        // tinted.
                        for (zi, z) in n.zones.iter().enumerate() {
                            g {
                                key: "z{zi}",
                                if z.2 && n.zones.len() > 1 {
                                    rect {
                                        x: "{z.0:.1}",
                                        y: "{n.y:.1}",
                                        width: "{(z.1 - z.0).max(1.0):.1}",
                                        height: "{n.h:.1}",
                                        fill: theme::SELECTED,
                                        fill_opacity: "0.12",
                                    }
                                }
                                if zi > 0 {
                                    line {
                                        x1: "{z.0:.1}",
                                        y1: "{n.y:.1}",
                                        x2: "{z.0:.1}",
                                        y2: "{n.y + n.h:.1}",
                                        stroke: theme::ZONE,
                                        stroke_width: "2",
                                    }
                                }
                            }
                        }
                        if let Some(cents) = n.cents {
                            text {
                                x: "{n.x + 3.0:.1}",
                                y: "{n.y - 2.0:.1}",
                                fill: theme::GOLD,
                                font_size: "9",
                                "{cents:+.0}¢"
                            }
                        }
                    }
                }

                // Effective-pitch guides, one per zone.
                for (i, z) in zone_guides.iter().enumerate() {
                    line {
                        key: "zg{i}",
                        x1: "{z.x0:.1}",
                        y1: "{z.y:.1}",
                        x2: "{z.x1:.1}",
                        y2: "{z.y:.1}",
                        stroke: theme::ZONE,
                        stroke_width: "1",
                        stroke_opacity: "0.7",
                        stroke_dasharray: "6 3",
                    }
                }

                // Expression curves — overlays first, active lane last.
                for (i, c) in curves.iter().enumerate() {
                    polyline {
                        key: "cv{i}",
                        points: "{c.points}",
                        fill: "none",
                        stroke: c.color,
                        stroke_width: if c.active { "2" } else { "1" },
                        stroke_opacity: if c.active {
                            if c.selected { "1" } else { "0.8" }
                        } else {
                            "0.3"
                        },
                        pointer_events: "none",
                    }
                }

                if let Some((x, y, w, h)) = marquee {
                    rect {
                        x: "{x:.1}",
                        y: "{y:.1}",
                        width: "{w:.1}",
                        height: "{h:.1}",
                        fill: theme::ACCENT,
                        fill_opacity: "0.12",
                        stroke: theme::ACCENT,
                        stroke_width: "1",
                    }
                }
            }

            // A non-equal tuning is always visibly flagged — silently
            // editing in a temperament you forgot about is how you ship
            // a detuned take.
            if microtonal {
                div {
                    style: "position: absolute; top: 6px; right: 8px; \
                            background: #422006; border: 1px solid {theme::GOLD}; \
                            border-radius: 4px; color: {theme::GOLD}; \
                            font-size: 10px; padding: 2px 7px; pointer-events: none;",
                    "{temperament_name}"
                }
            }
        }
    }
}

/// The bottom rail: selection readout plus the Melodyne drift/vibrato
/// controls for the selected note.
#[component]
fn StatusBar(editor: Signal<Editor>) -> Element {
    let mut editor = editor;
    let ed = editor.read();
    let selected = ed.selection.notes.first().copied();
    let info = selected.and_then(|id| ed.doc.note(id)).map(|n| {
        let units_per_second = ed.doc.time_base.units_per_second(ed.bpm);
        let d = expression_editor_core::blob::decompose(
            &n.pitch,
            n.start,
            n.end,
            64,
            units_per_second,
            0.0,
        );
        (
            expression_editor_core::tuning::note_name(n.row),
            n.channel,
            n.zone_count(),
            d.modulation_depth(),
            d.drift_extent(),
        )
    });
    let count = ed.selection.notes.len();
    let tool = ed.tool;
    drop(ed);

    rsx! {
        div {
            style: "display: flex; align-items: center; gap: 14px; padding: 4px 10px; \
                    background: {theme::PANEL}; border-top: 1px solid {theme::PANEL_BORDER}; \
                    color: {theme::TEXT_DIM}; font-size: 10px; \
                    font-family: ui-monospace, monospace;",
            span { "{tool.label()}" }
            span { "{count} selected" }
            if let Some((name, channel, zones, vibrato, drift)) = info {
                span { style: "color: {theme::TEXT};", "{name}" }
                if let Some(ch) = channel {
                    span { "ch {ch}" }
                }
                if zones > 1 {
                    span { style: "color: {theme::ZONE};", "{zones} zones" }
                }
                span { "vibrato {vibrato * 100.0:.0}¢" }
                span { "drift {drift * 100.0:+.0}¢" }

                // The two Melodyne sliders. They work on a hand-drawn
                // MPE bend as readily as on an analyzed vocal, because
                // the decomposition is derived, not stored.
                Blend { editor, label: "Drift".to_string(), drift: true }
                Blend { editor, label: "Vibrato".to_string(), drift: false }
                button {
                    style: theme::button_style(false),
                    title: "Flatten drift and vibrato",
                    onclick: move |_| {
                        let Some(id) = selected else { return };
                        let (t0, t1) = {
                            let ed = editor.read();
                            let Some(n) = ed.doc.note(id) else { return };
                            (n.start, n.end)
                        };
                        editor.write().apply(&expression_editor_core::Edit::ReblendPitch {
                            note: id,
                            t0,
                            t1,
                            drift_amount: 0.0,
                            modulation_amount: 0.0,
                        });
                    },
                    "Robot"
                }
            }
        }
    }
}

/// One of the drift/vibrato sliders.
///
/// Parked at 100% rather than tracking a stored amount: each change
/// re-decomposes the *current* curve, so the control is a relative
/// scale on what is there now and never fights an earlier edit.
#[component]
fn Blend(editor: Signal<Editor>, label: String, drift: bool) -> Element {
    let mut editor = editor;
    rsx! {
        label {
            style: "display: flex; align-items: center; gap: 4px;",
            "{label}"
            input {
                r#type: "range",
                min: "0",
                max: "150",
                value: "100",
                style: "width: 70px;",
                onchange: move |e: FormEvent| {
                    let Ok(v) = e.value().parse::<f64>() else { return };
                    let amount = v / 100.0;
                    let Some(id) = editor.read().selection.notes.first().copied() else {
                        return;
                    };
                    let (t0, t1) = {
                        let ed = editor.read();
                        let Some(n) = ed.doc.note(id) else { return };
                        (n.start, n.end)
                    };
                    editor.write().apply(&expression_editor_core::Edit::ReblendPitch {
                        note: id,
                        t0,
                        t1,
                        drift_amount: if drift { amount } else { 1.0 },
                        modulation_amount: if drift { 1.0 } else { amount },
                    });
                },
            }
        }
    }
}

/// Lanes shown by default in either domain.
///
/// Only Pitch: three overlaid curves on a first look is noise, and the
/// other two are one click away in the toolbar.
pub fn default_overlays() -> Vec<Lane> {
    vec![Lane::Pitch]
}
