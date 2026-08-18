//! `expression-editor-ui` — the Dioxus surface for the expression
//! editor.
//!
//! One component renders in two places without changing: as a desktop
//! window through `dioxus-native` → Blitz, and in the browser via the
//! wasm build. That is why every style here is inline and why the root
//! component takes no props it cannot get from a signal — a stylesheet
//! reference or a desktop-only launcher would break one of the two.
//!
//! It is an application, not a plugin. There is no VST3/CLAP editor and
//! no `nice-plug-dioxus`: that opens a plugin window through baseview,
//! and carrying a plugin framework to open an app window cost a second
//! windowing path and a `native` feature that existed only to pull it
//! in.
//!
//! State lives in [`expression_editor_core::Editor`], owned by the host
//! as a `Signal`. The component mutates view state (camera, tool,
//! selection) directly and the document only through `Editor::apply`,
//! so undo stays honest no matter which gesture produced the change.

use dioxus::prelude::*;
use expression_editor_core::{Dimension, Editor};

pub mod canvas;
pub mod demo;
pub mod drawer;
pub mod arp_panel;
pub mod cursor;
pub mod curve_editor;
pub mod drag;
pub mod envelopes;
pub mod velocity_panel;
pub mod guitar;
pub mod inspector;
pub mod keys;
pub mod interaction;
pub mod lane_strip;
pub mod menu_ui;
pub mod mode_picker;
pub mod multitool_ui;
pub mod paint;
pub mod quantize_panel;
pub mod roll;
/// The renderer seam. Native only — everything above it is portable.
#[cfg(not(target_arch = "wasm32"))]
pub mod roll_widget;
pub mod scroll;
pub mod sizing;
pub mod stack;
pub mod switcher;
pub mod text;
pub mod theme;
pub mod toolbar;
pub mod widgets;
pub mod workflow;

pub use drawer::ModDrawer;
pub use guitar::BendFlow;
pub use expression_editor_core as core;
pub use interaction::Drag;
pub use lane_strip::LaneStrip;
pub use menu_ui::ContextMenu;
pub use multitool_ui::MultiTool;
pub use roll::Canvas;
// Re-exported from `sizing` rather than moved, because these are the
// crate's public contract with a host: `available_space` is how a window
// or a REAPER dock tells the editor how much room it has.
pub use sizing::{available_space, current_viewport, viewport_in, AVAILABLE, CHROME_HEIGHT};

/// The empty-roll hint line, derived from the live mouse map: which
/// modifier+drag on open canvas inserts or paints a note.
pub(crate) fn draw_hint_of(ed: &Editor) -> String {
    use expression_editor_core::mouse::{Action, Context, Gesture};
    let draw = ed.mouse.bindings().iter().find(|b| {
        b.context == Context::PianoRoll
            && b.gesture == Gesture::Drag
            && matches!(
                b.action,
                Action::InsertNoteDragToExtend
                    | Action::InsertNoteDragToExtendNoSnap
                    | Action::InsertNoteDragToMove
                    | Action::InsertNote
                    | Action::InsertNoteNoSnap
                    | Action::InsertNoteDragToEditVelocity
                    | Action::PaintNotes
                    | Action::PaintNotesNoSnap
                    | Action::PaintRowOfNotes
            )
    });
    match draw {
        Some(b) => {
            let bits = b.mods.bits();
            let mut parts = Vec::new();
            if bits & 2 != 0 {
                parts.push("Ctrl");
            }
            if bits & 1 != 0 {
                parts.push("Shift");
            }
            if bits & 4 != 0 {
                parts.push("Alt");
            }
            parts.push("drag");
            format!("{} draws a note", parts.join("+"))
        }
        None => "The active tool draws with a drag".to_string(),
    }
}

/// The editor: toolbar over canvas.
///
/// The host owns `editor` so it can read the document back out — to a
/// MIDI take, or to an offline render job — without the component
/// needing to know which domain it is serving.
#[component]
pub fn ExpressionEditor(
    editor: Signal<Editor>,
    /// Open the modulation drawer on mount. Hosts normally leave this
    /// alone — it exists so a caller can restore a session, and so the
    /// screenshot harness can shoot the drawer through the real path.
    #[props(default)]
    initial_drawer: Option<ModDrawer>,
    /// Arm the Multi Tool on mount. Same purpose as `initial_drawer`:
    /// restore a session, and let the screenshot harness reach a state
    /// that is otherwise only produced by a key press.
    #[props(default)]
    initial_multi: Option<MultiTool>,
    /// Open a pitch drawing on mount. Same purpose again: restore a
    /// session, and let the screenshot harness reach a modal state that
    /// is otherwise only produced by a key press and several clicks.
    #[props(default)]
    initial_draft: Option<expression_editor_core::PitchDraft>,
    /// **Prototype (#161).** Where a string roll draws its bend flow.
    /// Ignored outside `RowSpace::Strings`.
    #[props(default)]
    bend_flow: Option<BendFlow>,
) -> Element {
    // Context rather than a prop chain: the flow variant is read deep
    // inside the canvas and nothing between here and there cares.
    use_context_provider(|| bend_flow.unwrap_or_default());
    // Painted frames, shared between the roll that counts them and the
    // status bar that reports them. Provided here because those two are
    // siblings — the counter belongs to neither.
    use_context_provider(roll_widget::Frames::new);
    let drag = use_signal(Drag::default);
    let drawer = use_signal(|| initial_drawer.clone().unwrap_or_default());
    let mut inspector_open = use_signal(|| true);
    let multi = use_signal(|| initial_multi.clone().unwrap_or_default());
    let menu_state = use_signal(ContextMenu::default);
    let draft = use_signal(|| initial_draft.clone());
    let mut pending = use_signal(|| None::<menu_ui::Pending>);

    // Follow the space the host reports.
    //
    // Here rather than in the canvas because this is the only component
    // that knows the whole chrome: the inspector's state is a signal it
    // owns, and the lane strip's height is a document property.
    //
    // An effect that does its own reading, *not* one closing over a
    // value computed in render. `use_effect` re-runs when a signal read
    // inside it changes; a plain value captured from the enclosing
    // render is not a signal, so an effect built that way runs once at
    // mount and is frozen for the life of the component. That is exactly
    // what happened here — the roll kept the viewport its document was
    // built with however large the window got, because the effect was
    // still holding the first render's answer.
    //
    // Writing `editor` from an effect that also reads it terminates:
    // the write only happens when the two differ by a pixel or more, so
    // the re-run it provokes finds them equal and stops.
    use_effect(move || {
        let Some((w, h)) = sizing::AVAILABLE() else {
            return;
        };
        // Decided under a *read*, and the guard dropped before writing.
        //
        // Taking a write guard notifies this signal's subscribers whether
        // or not anything is mutated, and this effect is one of them —
        // so `if let Ok(mut ed) = editor.try_write() && ed.viewport != want`
        // re-triggers itself forever, because the guard is taken before
        // the condition is ever tested. It hung every test that mounts
        // the editor. Read first, decide, then write only if there is
        // something to write.
        let want = {
            let ed = editor.read();
            sizing::viewport_within(w, h, sizing::chrome_of(&ed, inspector_open()))
        };
        let stale = {
            let ed = editor.read();
            (ed.viewport.w - want.w).abs() >= 1.0 || (ed.viewport.h - want.h).abs() >= 1.0
        };
        if stale
            && let Ok(mut ed) = editor.try_write()
        {
            ed.resize(want);
        }
    });

    // Menu items the core could not finish become UI: Properties simply
    // opens the inspector, which is where the note's fields already
    // live. The rest are picked up by the panels that own them.
    if matches!(*pending.read(), Some(menu_ui::Pending::Properties)) {
        inspector_open.set(true);
        pending.set(None);
    }

    rsx! {
        div {
            // The canvas is the only flexible child. Blitz sizes an
            // inline <svg> as a replaced element with an intrinsic
            // aspect ratio, so without an explicit `flex: 0 0 auto` on
            // the chrome it will happily eat the toolbar and status bar.
            style: "display: flex; flex-direction: column; width: 100%; height: 100%; \
                    min-height: 0; overflow: hidden; background: {theme::BG}; \
                    color: {theme::TEXT}; font-family: system-ui, sans-serif;",
            toolbar::Toolbar { editor, drag, drawer }
            switcher::TrackSwitcher { editor }
            div {
                style: "display: flex; flex: 1 1 auto; min-height: 0;",
                div {
                    style: "display: flex; flex-direction: column; flex: 1 1 auto; \
                            min-width: 0; min-height: 0;",
                    if editor.read().stacked {
                        stack::StackView { editor }
                    } else {
                        Canvas { editor, drag, drawer, multi, menu_state, pending, draft }
                        LaneStrip { editor }
                    }
                }
                inspector::Inspector { editor, open: inspector_open }
            }
            toolbar::StatusBar { editor }
        }
    }
}

/// The glyph drawn on a handle, as an SVG path.
///
/// Each mark says what the handle *does* rather than naming it: the
/// slopes are the slope they apply, fine pitch is a tick, formant a
/// bar, amplitude a dot, vibrato a wave. At fourteen pixels there is no
/// room for a word, and a shape is faster to read than one anyway.
/// `hollow` draws the amplitude handle as an empty circle, which is how
/// the manual signals that a drag will hit only the sibilants rather
/// than the whole note.
pub(crate) fn handle_mark(
    handle: expression_editor_core::Handle,
    cx: f64,
    cy: f64,
    r: f64,
    hollow: bool,
) -> String {
    use expression_editor_core::Handle as H;
    match handle {
        H::LeftSlope => format!(
            "M {:.1} {:.1} L {:.1} {:.1}",
            cx - r,
            cy + r * 0.5,
            cx + r,
            cy - r * 0.5
        ),
        H::RightSlope => format!(
            "M {:.1} {:.1} L {:.1} {:.1}",
            cx - r,
            cy - r * 0.5,
            cx + r,
            cy + r * 0.5
        ),
        H::FinePitch => format!(
            "M {:.1} {:.1} L {:.1} {:.1}",
            cx - r * 0.6,
            cy,
            cx + r * 0.6,
            cy
        ),
        H::Formant => format!(
            "M {:.1} {:.1} L {:.1} {:.1}",
            cx,
            cy - r * 0.7,
            cx,
            cy + r * 0.7
        ),
        H::Amplitude => {
            // A small circle, drawn as two arcs so it stays one path.
            // Larger when hollow, since an outline reads smaller than a
            // filled dot at this size.
            let d = if hollow { r * 0.58 } else { r * 0.42 };
            format!(
                "M {:.1} {:.1} a {:.1} {:.1} 0 1 0 {:.1} 0 a {:.1} {:.1} 0 1 0 {:.1} 0",
                cx - d,
                cy,
                d,
                d,
                d * 2.0,
                d,
                d,
                -d * 2.0
            )
        }
        H::Vibrato => format!(
            "M {:.1} {:.1} q {:.1} {:.1} {:.1} 0 q {:.1} {:.1} {:.1} 0",
            cx - r,
            cy,
            r * 0.25,
            -r * 0.9,
            r,
            r * 0.25,
            r * 0.9,
            r
        ),
        H::Pitch => String::new(),
    }
}


/// Lanes shown by default in either domain.
///
/// Only Pitch: three overlaid curves on a first look is noise, and the
/// other two are one click away in the toolbar.
pub fn default_overlays() -> Vec<Dimension> {
    vec![Dimension::Pitch]
}

/// The raw widgets, for tests that need to drive one in isolation.
///
/// Not part of the panel API — exported so `tests/slider_drag.rs` can
/// mount a single control without the whole panel around it. Absorbed
/// with the panels from `midi-tools-ui` (#153).
pub mod test_support {
    pub use crate::drag::{BarEditor, RangeSlider, Slider};
}

pub use arp_panel::{ArpPanel, ArpSinkHandle};
pub use velocity_panel::{SinkHandle, VelocityPanel};
