//! Which surface you are working on, down the right-hand side.
//!
//! This was a full-width row across the top — seven buttons that a user
//! presses a handful of times a session, sitting above the music for the
//! whole of it. It cost thirty pixels of every window, and thirty pixels
//! of height is worth far more here than thirty pixels of width: the
//! roll is a *pitch* axis, and the thing it never has enough of is
//! vertical room.
//!
//! Down the side it costs width we already had, reads as a list rather
//! than a strip of icons, and has room to say which family each mode
//! belongs to — which the top row could only imply with a divider.
//!
//! ## Why it lives in the inspector
//!
//! Rather than a rail of its own beside it. A second right-hand column
//! would take another hundred pixels from the roll to show seven items,
//! and the inspector is already the panel for "what am I looking at".
//! Modes go at the top of it because they are the widest-scoped thing in
//! there: everything below is about the selection, and this is about the
//! whole surface.

use dioxus::prelude::*;
use expression_editor_core::{Editor, Mode, ModeFamily};

use crate::theme;

/// The mode list, grouped by family.
#[component]
pub fn ModePicker(editor: Signal<Editor>) -> Element {
    let mut editor = editor;
    let current = editor.read().mode;

    rsx! {
        div {
            "data-testid": "mode-picker",
            style: "display: flex; flex-direction: column; \
                    border-bottom: 1px solid {theme::PANEL_BORDER};",

            for family in ModeFamily::ALL {
                // The family is a caption rather than a divider. The top
                // row could only separate these with a rule, which said
                // "these are different" without ever saying how — and
                // the difference is exactly the one that decides what an
                // edit writes back: events on one side, stretch markers
                // and envelope points on the other.
                div {
                    style: "font-size: 9px; letter-spacing: 0.08em; \
                            text-transform: uppercase; color: {theme::TEXT_DIM}; \
                            padding: 8px 10px 3px;",
                    "{family.label()}"
                }
                for m in family.modes().iter().copied() {
                    ModeRow { key: "{m:?}", mode: m, active: current == m,
                        onpick: move |_| editor.write().set_mode(m) }
                }
            }
        }
    }
}

/// One mode. A row, not a chip: it is a choice from a list.
#[component]
fn ModeRow(mode: Mode, active: bool, onpick: EventHandler<()>) -> Element {
    let (fg, bg) = if active {
        (theme::SELECTED, theme::CONTROL_ACTIVE)
    } else {
        (theme::TEXT, "transparent")
    };
    rsx! {
        button {
            "data-testid": "mode-{mode:?}",
            title: "{mode.label()}",
            style: "display: flex; align-items: center; gap: 8px; \
                    width: 100%; border: none; text-align: left; \
                    padding: 5px 10px; cursor: pointer; \
                    background: {bg}; color: {fg}; \
                    font-size: 11px; font-family: system-ui, sans-serif;",
            onclick: move |_| onpick.call(()),
            svg {
                view_box: "0 0 16 16",
                style: "width: 13px; height: 13px; flex: 0 0 auto;",
                path {
                    d: theme::mode_icon(mode),
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "1.3",
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                }
            }
            span { "{mode.label()}" }
        }
    }
}
