//! The compact toolbar: TOOLS, EXPRESSION, CURVE SHAPE, TUNING.

use dioxus::prelude::*;
use expression_editor_core::doc::Lane;
use expression_editor_core::{Editor, Shape, Tool};

use crate::interaction::{self, Drag};
use crate::theme;

/// Glyphs rather than words: the toolbar has to stay narrow enough to
/// sit above a plugin-sized canvas.
fn tool_glyph(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "⬚",
        Tool::Pen => "✎",
        Tool::Curve => "∿",
        Tool::Eraser => "⌫",
        Tool::NoteDraw => "▤",
        Tool::NoteErase => "✕",
    }
}

fn shape_glyph(shape: Shape) -> &'static str {
    match shape {
        Shape::Linear => "╱",
        Shape::EaseIn => "◟",
        Shape::EaseOut => "◜",
        Shape::EaseInOut => "∫",
        Shape::Exponential => "⌐",
        Shape::SCurve => "∽",
    }
}

#[component]
pub fn Toolbar(editor: Signal<Editor>, drag: Signal<Drag>) -> Element {
    let ed = editor.read();
    let tool = ed.tool;
    let lane = ed.lane;
    let overlays = ed.overlays.clone();
    let shape = ed.shape;
    let grid_label = ed.grid.label();
    let grid_on = ed.grid.enabled;
    let temperament = ed.tuning.temperament.name;
    let key_pc = ed.tuning.key_pc;
    let snap_12tet = ed.tuning.snap_12tet;
    let bend_range = ed.doc.bend_range;
    let can_undo = ed.can_undo();
    let can_redo = ed.can_redo();
    drop(ed);

    rsx! {
        div {
            style: "display: flex; align-items: center; flex-wrap: wrap; gap: 2px; \
                    padding: 5px 4px; background: {theme::PANEL}; \
                    border-bottom: 1px solid {theme::PANEL_BORDER}; \
                    font-family: system-ui, sans-serif;",

            // ── TOOLS ────────────────────────────────────────────────
            div {
                style: theme::group_style(),
                span { style: theme::group_label_style(), "Tools" }
                for t in Tool::ALL {
                    button {
                        key: "{t:?}",
                        style: theme::button_style(tool == t),
                        title: "{t.label()}",
                        onclick: move |_| editor.write().tool = t,
                        "{tool_glyph(t)}"
                    }
                }
            }

            // ── EXPRESSION ───────────────────────────────────────────
            // A joined lane-and-eye control: the lane name selects it
            // for editing, the eye toggles it as an overlay behind the
            // active lane.
            div {
                style: theme::group_style(),
                span { style: theme::group_label_style(), "Expression" }
                for l in Lane::ALL {
                    div {
                        key: "{l:?}",
                        style: "display: flex; align-items: center; gap: 0;",
                        button {
                            style: format!(
                                "{} border-top-right-radius: 0; border-bottom-right-radius: 0; \
                                 border-right: none; color: {};",
                                theme::button_style(lane == l),
                                theme::lane_color(l),
                            ),
                            onclick: move |_| editor.write().lane = l,
                            "{theme::lane_label(l)}"
                        }
                        button {
                            style: format!(
                                "{} border-top-left-radius: 0; border-bottom-left-radius: 0; \
                                 min-width: 22px; padding: 0 4px;",
                                theme::button_style(overlays.contains(&l)),
                            ),
                            title: "Show as overlay",
                            onclick: move |_| {
                                let mut ed = editor.write();
                                match ed.overlays.iter().position(|&x| x == l) {
                                    Some(i) => { ed.overlays.remove(i); }
                                    None => ed.overlays.push(l),
                                }
                            },
                            if overlays.contains(&l) { "◉" } else { "○" }
                        }
                    }
                }
            }

            // ── CURVE SHAPE ──────────────────────────────────────────
            div {
                style: theme::group_style(),
                span { style: theme::group_label_style(), "Curve" }
                for s in Shape::ALL {
                    button {
                        key: "{s:?}",
                        style: theme::button_style(shape == s),
                        title: "{s.label()}",
                        onclick: move |_| {
                            let d = drag.read().clone();
                            interaction::apply_shape(&mut editor.write(), &d, s);
                        },
                        "{shape_glyph(s)}"
                    }
                }
            }

            // ── GRID ─────────────────────────────────────────────────
            div {
                style: theme::group_style(),
                span { style: theme::group_label_style(), "Grid" }
                button {
                    style: theme::button_style(grid_on),
                    title: "Snap to the editor's own grid",
                    onclick: move |_| {
                        let on = editor.read().grid.enabled;
                        editor.write().grid.enabled = !on;
                    },
                    "⊞"
                }
                button {
                    style: theme::button_style(false),
                    title: "Coarser (1)",
                    onclick: move |_| editor.write().grid.coarser(),
                    "−"
                }
                // Fixed-width so the readout does not jitter as the
                // division changes.
                span {
                    style: "min-width: 44px; text-align: center; color: {theme::TEXT}; \
                            font-size: 11px; font-family: ui-monospace, monospace;",
                    "{grid_label}"
                }
                button {
                    style: theme::button_style(false),
                    title: "Finer (2)",
                    onclick: move |_| editor.write().grid.finer(),
                    "+"
                }
            }

            // ── TUNING ───────────────────────────────────────────────
            div {
                style: theme::group_style(),
                span { style: theme::group_label_style(), "Tuning" }
                select {
                    style: theme::select_style(),
                    value: "{temperament}",
                    onchange: move |e| {
                        let name = e.value();
                        if let Some(t) = expression_editor_core::tuning::PRESETS
                            .iter()
                            .find(|t| t.name == name)
                        {
                            editor.write().tuning.temperament = (*t).clone();
                        }
                    },
                    for t in expression_editor_core::tuning::PRESETS {
                        option { key: "{t.name}", value: "{t.name}", "{t.name}" }
                    }
                }
                select {
                    style: theme::select_style(),
                    value: "{key_pc}",
                    onchange: move |e| {
                        if let Ok(pc) = e.value().parse::<i32>() {
                            editor.write().tuning.key_pc = pc;
                        }
                    },
                    for pc in 0..12 {
                        option {
                            key: "{pc}",
                            value: "{pc}",
                            "{expression_editor_core::tuning::pitch_class_name(pc)}"
                        }
                    }
                }
                button {
                    style: theme::button_style(snap_12tet),
                    title: "Also offer ordinary semitone centers",
                    onclick: move |_| {
                        let on = editor.read().tuning.snap_12tet;
                        editor.write().tuning.snap_12tet = !on;
                    },
                    "Snap 12TET"
                }
                label {
                    style: "display: flex; align-items: center; gap: 4px; \
                            color: {theme::TEXT_DIM}; font-size: 10px;",
                    "Bend"
                    input {
                        r#type: "number",
                        min: "1",
                        max: "96",
                        value: "{bend_range}",
                        style: "{theme::select_style()} width: 52px;",
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<f64>() {
                                editor.write().doc.bend_range = v.max(1.0);
                            }
                        },
                    }
                }
            }

            // ── HISTORY / VIEW ───────────────────────────────────────
            div {
                style: "display: flex; align-items: center; gap: 4px; padding: 0 8px;",
                button {
                    style: theme::button_style(false),
                    disabled: !can_undo,
                    title: "Undo",
                    onclick: move |_| { editor.write().undo(); },
                    "↶"
                }
                button {
                    style: theme::button_style(false),
                    disabled: !can_redo,
                    title: "Redo",
                    onclick: move |_| { editor.write().redo(); },
                    "↷"
                }
                button {
                    style: theme::button_style(false),
                    title: "Reset View (V)",
                    onclick: move |_| editor.write().reset_view(),
                    "Reset View"
                }
            }
        }
    }
}
