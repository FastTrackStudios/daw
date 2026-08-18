//! The piano roll — the surface every gesture lands on.
//!
//! One component, and the only place that turns a pointer position into
//! a document coordinate. Split out of `lib.rs`, where it was 1500 of
//! 2000 lines and the reason the sizing model was hard to reason about.
//!
//! ## Roll space
//!
//! Every handler here works in *roll space*: element coordinates less
//! the keyboard gutter down the left and the ruler across the top, which
//! is what [`local`] does and the only place that subtraction happens.
//! The camera therefore never has to know the chrome exists.
//!
//! That mapping is exact rather than approximate, and deliberately so —
//! see [`crate::sizing`] for why the svg carries no `viewBox` and takes
//! its box from layout.
//!
//! ## What decides a gesture
//!
//! Not the armed tool by itself. `interaction::pointer_down` resolves
//! `Editor::mouse` first, giving the armed tool first refusal on the
//! plain gestures it claims, and falls through to the tool-driven path
//! only where the map says nothing.

use dioxus::prelude::*;
use dioxus_elements::input_data::MouseButton;
use expression_editor_core::memagic;
use expression_editor_core::tools::Mods;
use expression_editor_core::Editor;
use input::InputCommand;
use keyboard_types::Modifiers;

use crate::interaction::{self, Drag};
use crate::menu_ui::{self, ContextMenu};
use crate::multitool_ui::{self, MultiTool};
use crate::{
    canvas, cursor, drawer, drawer::ModDrawer, keys, paint, roll_widget, scroll, text, theme,
};
use crate::{draw_hint_of, BendFlow};
/// Pointer coordinates in **roll** space — element coordinates minus
/// the keyboard gutter and timeline ruler.
///
/// Every interaction handler works in roll space, so the camera never
/// has to know the chrome exists.
fn local(e: &PointerEvent) -> (f64, f64) {
    let c = e.data().element_coordinates();
    (c.x - canvas::GUTTER_W, c.y - canvas::RULER_H)
}

/// Where in the chrome a press landed, if it did.
enum Chrome {
    Roll,
    /// The ruler: clicking it moves the playhead.
    Ruler(f64),
    /// A piano key: clicking it selects every note on that row.
    Key(i32),
}

/// Which region the pointer is in, and what to anchor on.
///
/// The anchor follows MeMagic's priority chain: the playhead while the
/// transport is running, else the pointer, else the middle of the view.
/// Zooming during playback should frame the music, not wherever the
/// mouse happened to be left.
///
/// With no pointer at all — a toolbar press, a key with the mouse
/// elsewhere — the region is `Elsewhere`, which fits the whole item
/// rather than guessing at a position.
fn memagic_at(ed: &Editor, hover: Option<(f64, f64)>) -> (memagic::Region, memagic::Anchor) {
    let Some((x, y)) = hover else {
        return (
            memagic::Region::Elsewhere,
            memagic::Anchor {
                t: ed.playhead.unwrap_or_else(|| ed.camera.t_at(ed.viewport.w * 0.5)),
                row: None,
            },
        );
    };
    let region = match chrome_at(ed, x, y) {
        Chrome::Ruler(_) => memagic::Region::Ruler,
        Chrome::Key(_) => memagic::Region::Piano,
        Chrome::Roll => memagic::Region::NoteArea,
    };
    let pointer_t = ed.camera.t_at(x - canvas::GUTTER_W);
    (
        region,
        memagic::Anchor {
            t: ed.playhead.unwrap_or(pointer_t),
            row: Some(ed.camera.pitch_at(y - canvas::RULER_H, ed.viewport)),
        },
    )
}

fn chrome_at(ed: &Editor, x: f64, y: f64) -> Chrome {
    if y < canvas::RULER_H {
        Chrome::Ruler(ed.camera.t_at(x - canvas::GUTTER_W))
    } else if x < canvas::GUTTER_W {
        Chrome::Key(ed.camera.pitch_at(y - canvas::RULER_H, ed.viewport).round() as i32)
    } else {
        Chrome::Roll
    }
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
pub fn Canvas(
    editor: Signal<Editor>,
    drag: Signal<Drag>,
    drawer: Signal<ModDrawer>,
    multi: Signal<MultiTool>,
    menu_state: Signal<ContextMenu>,
    pending: Signal<Option<menu_ui::Pending>>,
    draft: Signal<Option<expression_editor_core::PitchDraft>>,
) -> Element {
    let mut multi = multi;
    let mut editor = editor;
    let mut drag = drag;
    let mut drawer = drawer;
    let mut menu_state = menu_state;
    let mut draft = draft;

    // Where the pointer last was, in element coordinates.
    //
    // Needed because a keypress carries no position and MeMagic is
    // anchored on one — "zoom to what I am pointing at" has to know
    // where that is. `None` until the pointer has been over the canvas.
    let mut hover = use_signal(|| None::<(f64, f64)>);

    // Modifiers as of the last pointer or key event.
    //
    // Tracked separately from `hover` because holding Alt has to change
    // the painted cursor *without* the pointer moving — the whole point
    // of the glyph is that it previews the gesture, and a preview that
    // waits for a wiggle before admitting Alt is bound to Copy is not
    // one.
    let mut hover_mods = use_signal(Mods::default);

    // What can follow the keys typed so far. Empty means no sequence is
    // live, which is the overlay's cue to stay hidden.
    let mut which_key = use_signal(Vec::<keys::Continuation>::new);

    // The viewport is followed by `ExpressionEditor`, which is the only
    // component that knows the whole chrome. See the effect there.

    // Where each render leaves the drawing for the renderer to pick up.
    let slot = use_hook(roll_widget::SceneSlot::new);

    // The shaper, kept across renders because that is the whole point of
    // it: the same twenty pitch names are drawn every frame and must be
    // shaped once, not once per frame.
    let labels = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(text::Labeller::new())));

    // The widget, created once. `CustomWidgetAttr` is write-once — the
    // DOM takes the widget out of it on the first mutation — so building
    // a new one per render would hand the second render an empty
    // attribute and the roll would go blank.
    let frames = use_context::<roll_widget::Frames>();
    let widget = use_hook(|| {
        dioxus_native_dom::CustomWidgetAttr::new(roll_widget::RollWidget::new(
            slot.clone(),
            frames.clone(),
        ))
    });

    // While the drawer is open its target is locked: editing gestures
    // are blocked, but every navigation path stays live so the preview
    // can be auditioned in context.
    let locked = drawer.read().open;

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

    let ed = editor.read();
    let vp = ed.viewport;
    let cc_editing = ed.cc_edit;
    let microtonal = !ed.tuning.temperament.is_equal();
    let temperament_name = ed.tuning.temperament.name;
    let empty = ed.doc.notes.is_empty();
    // The empty-roll hint names the *actual* draw binding, read from
    // the live mouse map — which the host may have overlaid with the
    // user's own REAPER mouse modifiers. A hardcoded "press D" hint
    // goes stale the moment the map differs.
    let draw_hint = draw_hint_of(&ed);

    // ── the drawing ──────────────────────────────────────────────────
    //
    // Built here, in render, where reading a signal is ordinary. The
    // widget below only replays what this leaves in the slot — see
    // `crate::roll_widget` for why the renderer must not reach back into
    // dioxus to fetch it.
    //
    // Rebuilding on every render is the same cadence the svg markup was
    // rebuilt at, and far cheaper: a `Scene` is a vector of draw
    // commands, where the markup had to be re-parsed into a usvg tree
    // before anything reached the screen.
    frames.built();
    slot.put(paint::roll_scene(
        &ed,
        vp.w + canvas::GUTTER_W,
        vp.h + canvas::RULER_H,
        &paint::Overlay {
            marquee,
            draft: draft.read().as_ref().map(|d| canvas::draft_view(&ed, d)),
            // Prototype (#161): read deep in the drawing and nothing
            // between here and there cares, so it arrives by context.
            flow: try_consume_context::<BendFlow>().unwrap_or_default(),
        },
        &mut labels.borrow_mut(),
    ));
    drop(ed);

    rsx! {
        div {
            // `flex: 1 1 0` + a floor. The zero basis is load-bearing:
            // with `auto`, the basis comes from the content, and the
            // content is an svg whose intrinsic size is the `viewBox` —
            // which is `Editor::viewport`, which the measure below sets
            // *from this cell*. That is a loop: cell sizes svg, svg
            // sizes cell, and the measure drives it round every tick.
            // It showed up as `wgpu error: Out of Memory`, because each
            // pass is a full re-render.
            //
            // The floor is what the old `auto` basis was really for: a
            // parent whose height does not resolve (a plugin window
            // before its first resize, a headless mount) would otherwise
            // collapse the canvas to nothing, and the svg no longer
            // volunteers any height of its own.
            style: "position: relative; flex: 1 1 0; min-height: 360px; \
                    overflow: hidden; outline: none;",
            // The cell is what gets measured to drive `Editor::resize`.
            // `onresize` cannot do it: dioxus-native's
            // `convert_resize_data` is `unimplemented!()`, so an element
            // resize event never arrives in the renderer the plugin, the
            // REAPER panel and the desktop runner all use. The measure
            // below polls this element instead, which is what the REAPER
            // panel already did from outside.
            "data-testid": "canvas-cell",
            tabindex: "0",
            onkeydown: move |e: KeyboardEvent| {
                let key = e.key().to_string();
                let m = mods_of(e.modifiers());
                // Pressing a modifier is itself a keydown, so this is
                // what makes the painted cursor answer to Alt and Ctrl
                // with the pointer sitting still.
                hover_mods.set(m);

                // The shared keymap gets first refusal, so which-key
                // sequences resolve before any hardcoded binding. `z`
                // used to be hardcoded here, which ate the first key of
                // every zoom sequence the REAPER profile defines.
                if key == "Escape" && keys::is_pending() {
                    keys::cancel();
                    which_key.set(Vec::new());
                    e.prevent_default();
                    return;
                }
                let commands = keys::resolve(&key, m);
                let mut ran = false;
                for cmd in &commands {
                    let action = match cmd {
                        InputCommand::Action(a) => Some(a.0.as_str()),
                        InputCommand::ActionWithArgs { action, .. } => Some(action.0.as_str()),
                        _ => None,
                    };
                    if let Some(action) = action {
                        let (region, anchor) = memagic_at(&editor.read(), hover());
                        ran |= keys::dispatch(&mut editor.write(), action, region, anchor);
                    }
                }
                // A half-typed sequence owns the key too: `z` on its way
                // to `z i` must not also fire the tool shortcut.
                which_key.set(keys::continuations());
                if ran || keys::is_pending() {
                    e.prevent_default();
                    return;
                }

                // A pitch drawing is modal, so it takes its keys first
                // and Ctrl+Z means *its* undo — the document's history
                // has nothing in it to undo yet, by design.
                if draft.read().is_some() {
                    let mut handled = true;
                    match (key.as_str(), m.ctrl) {
                        ("Enter", _) => {
                            let d = draft.read().clone();
                            if let Some(d) = d {
                                editor.write().apply_draft(&d);
                            }
                            draft.set(None);
                        }
                        ("Escape", _) => {
                            let d = draft.read().clone();
                            if let Some(d) = d {
                                editor.write().dismiss_draft(&d);
                            }
                            draft.set(None);
                        }
                        ("z", true) => {
                            let mut dw = draft.write();
                            if let Some(dr) = dw.as_mut() {
                                if m.shift { dr.redo(); } else { dr.undo(); }
                                let preview = dr.clone();
                                drop(dw);
                                editor.write().preview_draft(&mut { preview });
                            }
                        }
                        _ => handled = false,
                    }
                    if handled {
                        e.prevent_default();
                        return;
                    }
                }

                // Open a drawing on the selected note.
                if key == "3" && !m.ctrl {
                    let opened = {
                        let ed = editor.read();
                        ed.selection
                            .notes
                            .first()
                            .and_then(|id| expression_editor_core::PitchDraft::open(&ed.doc, *id))
                    };
                    if opened.is_some() {
                        draft.set(opened);
                        e.prevent_default();
                        return;
                    }
                }
                // B opens the drawer; Escape closes it. Both work
                // whether or not it is already open.
                if key == "b" && !m.ctrl {
                    let mut dw = drawer.write();
                    if dw.open {
                        dw.cancel(&mut editor.write());
                    } else if dw.open_on(&editor.read()) {
                        dw.preview(&mut editor.write());
                    }
                    e.prevent_default();
                    return;
                }
                // The Multi Tool arms over the selection and takes its
                // own gestures while armed.
                if key == "a" && !m.ctrl {
                    let armed = multi.read().armed;
                    if armed {
                        multi.write().disarm();
                    } else {
                        let ok = multi.write().arm(&editor.read());
                        if !ok {
                            multi.write().disarm();
                        }
                    }
                    e.prevent_default();
                    return;
                }
                // Drum-mode keys, before the general ones: `f` means
                // flam here and nothing elsewhere.
                if key == "f" && !m.ctrl && editor.read().mode == expression_editor_core::Mode::Drums {
                    let made = editor.write().flam_selection();
                    if made > 0 {
                        e.prevent_default();
                        return;
                    }
                }

                if multi.read().armed {
                    match key.as_str() {
                        "m" => {
                            let mut t = multi.write();
                            t.toggle_bend(&mut editor.write());
                            e.prevent_default();
                            return;
                        }
                        "s" => {
                            let mut t = multi.write();
                            t.toggle_symmetric(&mut editor.write());
                            e.prevent_default();
                            return;
                        }
                        "Escape" => {
                            multi.write().disarm();
                            e.prevent_default();
                            return;
                        }
                        _ => {}
                    }
                }
                if key == "Escape" && drawer.read().open {
                    drawer.write().cancel(&mut editor.write());
                    e.prevent_default();
                    return;
                }
                if locked {
                    return;
                }
                let d = drag.read().clone();
                if interaction::key_down(&mut editor.write(), &d, &key, m) {
                    e.prevent_default();
                }
            },
            onkeyup: move |e: KeyboardEvent| {
                hover_mods.set(mods_of(e.modifiers()));
                // `R` is momentary: references drop back the instant it
                // is released, so it can never be left on by accident.
                match e.key().to_string().as_str() {
                    "r" => editor.write().refs_to_front = false,
                    "m" => editor.write().reference_to_front = false,
                    _ => {}
                }
            },

            // ── the roll ─────────────────────────────────────────────
            //
            // A custom widget, painted straight into the renderer's
            // scene by `crate::paint`. It replaced an inline `<svg>`
            // subtree, for two reasons that were both structural:
            //
            // - Blitz paints an inline svg as a replaced element with a
            //   hardcoded `object-fit: contain`, so the drawing was
            //   scaled by (element box / declared size) — and an svg
            //   that declares no size takes it from its own content, so
            //   the roll rescaled as it scrolled. A widget is handed its
            //   box; there is no ratio to get wrong.
            // - Every camera move rebuilt the roll's markup, which Blitz
            //   re-parsed into a usvg tree before drawing anything. The
            //   scene is a recording: rebuilt when the state changes,
            //   replayed by the renderer every frame.
            //
            // The events stay on the element. `<object>` is an ordinary
            // DOM node, so `element_coordinates()` still arrives here —
            // and with nothing scaling, those *are* document
            // coordinates. `interaction.rs` did not have to change.
            object {
                "data-testid": "roll",
                "data": widget,
                // The box the scene was built for, as an attribute
                // rather than a readout in the status bar.
                //
                // It is the only way to ask the *mounted* surface what it
                // is drawing for, which is what pins the scene and the
                // element to one box — but a size in the corner of the
                // window is a debug aid, and debug aids do not belong in
                // a shipped chrome. `tests/geometry.rs` reads it here.
                "data-viewport": "{vp.w:.0}x{vp.h:.0}",
                // Explicit, and the same box the scene was built for.
                // A widget reports no intrinsic size, so without this it
                // has none — and blitz-paint skips a widget whose box is
                // zero, silently.
                // `cursor: none` because `crate::cursor` paints the real
                // one: CSS has no `[`, no `]`, no pencil and no razor,
                // and Blitz supports no `cursor: url(…)` to supply them.
                style: "position: absolute; left: 0; top: 0; display: block; \
                        width: {vp.w + canvas::GUTTER_W:.0}px; \
                        height: {vp.h + canvas::RULER_H:.0}px; \
                        touch-action: none; user-select: none; cursor: none;",
                onpointerdown: move |e: PointerEvent| {
                    let raw = e.data().element_coordinates();
                    // Resolve against a snapshot: the read guard must
                    // be dropped before any arm can write.
                    let where_ = chrome_at(&editor.read(), raw.x, raw.y);
                    match where_ {
                        Chrome::Ruler(t) => {
                            editor.write().playhead = Some(t);
                            return;
                        }
                        Chrome::Key(row) => {
                            let ids: Vec<_> = editor
                                .read()
                                .doc
                                .notes
                                .iter()
                                .filter(|n| n.row == row)
                                .map(|n| n.id)
                                .collect();
                            editor.write().selection.notes = ids;
                            return;
                        }
                        Chrome::Roll => {}
                    }
                    if locked {
                        return;
                    }
                    let (x, y) = local(&e);
                    let m = mods_of(e.modifiers());
                    let button = match e.trigger_button() {
                        Some(MouseButton::Secondary) => 2,
                        // Middle is a real gesture (hand-scroll pan);
                        // collapsing it onto left made a middle-drag
                        // edit notes.
                        Some(MouseButton::Auxiliary) => 1,
                        _ => 0,
                    };
                    // A pitch drawing owns the surface while it is up:
                    // a click is an anchor and nothing else.
                    if draft.read().is_some() {
                        let mut dw = draft.write();
                        let Some(dr) = dw.as_mut() else { return };
                        let next = interaction::draft_press(&editor.read(), dr, x, y);
                        let preview = dr.clone();
                        drop(dw);
                        editor.write().preview_draft(&mut { preview });
                        drag.set(next);
                        return;
                    }
                    let d = interaction::pointer_down(&mut editor.write(), x, y, m, button);
                    // A right-click resolves to a menu request rather
                    // than a drag. Opening it here — not in `interaction`
                    // — keeps the core pointer path free of UI state.
                    if let Drag::ContextMenu { x, y, under, t } = d {
                        menu_state.write().show(x, y, under, t);
                        return;
                    }
                    menu_state.write().close();
                    drag.set(d);
                },
                onpointermove: move |e: PointerEvent| {
                    // Recorded before the drag guard: the anchor has to
                    // follow the pointer whether or not a drag is live.
                    hover.set(Some(local(&e)));
                    hover_mods.set(mods_of(e.modifiers()));
                    if !drag.read().is_active() {
                        return;
                    }
                    let (x, y) = local(&e);
                    let m = mods_of(e.modifiers());
                    // Read the drag out before touching it: the guard
                    // would otherwise still be alive when `drag.set`
                    // wants the mutable borrow.
                    let current = drag.read().clone();
                    if let Drag::DraftAnchor { index } = current {
                        let mut dw = draft.write();
                        if let Some(dr) = dw.as_mut() {
                            let moved = interaction::draft_move(&editor.read(), dr, index, x, y);
                            let preview = dr.clone();
                            drop(dw);
                            editor.write().preview_draft(&mut { preview });
                            if let Some(i) = moved {
                                drag.set(Drag::DraftAnchor { index: i });
                            }
                        }
                        return;
                    }
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
                onpointerleave: move |_| {
                    // The painted cursor is only correct while the
                    // pointer is over the roll. Left set, it would be
                    // stranded at the last position the pointer had
                    // *and* the OS cursor would be visible next to it.
                    hover.set(None);
                },
                onwheel: move |e: WheelEvent| {
                    // Normalised to notches: a touchpad reports pixels and a
                    // mouse reports lines, and the gain constants
                    // downstream are tuned for lines. See `scroll::notches`.
                    let (dx, dy) = scroll::notches(&e.delta());
                    let m = mods_of(e.modifiers());
                    // Anchored on the last pointer position when there
                    // is one: a wheel event carries none of its own, and
                    // zooming about the middle of the view when the
                    // mouse is somewhere else is the wrong place.
                    let (x, y) = hover().unwrap_or((vp.w * 0.5, vp.h * 0.5));
                    interaction::wheel(&mut editor.write(), x, y, dx, dy, m);
                    e.prevent_default();
                },
            }

            if empty {
                div {
                    style: "position: absolute; inset: 0; display: flex; \
                            align-items: center; justify-content: center; \
                            pointer-events: none; color: {theme::TEXT_DIM}; \
                            font-size: 12px; text-align: center; line-height: 1.7;",
                    div {
                        div { style: "color: {theme::TEXT}; font-size: 14px;", "No notes" }
                        div { "{draw_hint}" }
                        // Read from the bindings, not restated here:
                        // this line still said "Scroll zooms ·
                        // Alt+scroll pans" after the shared config made
                        // that wrong, which is what a hardcoded hint is
                        // for.
                        div { "{scroll::hint()}" }
                    }
                }
            }

            // The painted pointer. Above the roll and below the modal
            // overlays, which have their own (real) cursors — and its
            // own component, so a pointer move repaints one 48px box
            // instead of rebuilding the whole roll scene. See
            // `crate::cursor`.
            cursor::CursorLayer {
                editor,
                hover,
                mods: hover_mods,
                drag,
                locked,
            }

            multitool_ui::MultiToolOverlay { editor, tool: multi }

            menu_ui::ContextMenuOverlay { editor, menu_state, pending }

            drawer::ModulationDrawer { editor, drawer }

            // A non-equal tuning is always visibly flagged — silently
            // editing in a temperament you forgot about is how you ship
            // a detuned take.
            if let Some(number) = cc_editing {
                div {
                    style: "position: absolute; top: 6px; left: 60px; display: flex; \
                            align-items: center; gap: 8px; background: {theme::BANNER_INFO}; \
                            border: 1px solid {theme::ACCENT}; border-radius: 4px; \
                            color: {theme::ACCENT}; font-size: 10px; padding: 3px 9px;",
                    span { "CC edit — CC{number}" }
                    button {
                        style: "background: none; border: none; color: {theme::ACCENT}; \
                                cursor: pointer; font-size: 11px; padding: 0;",
                        onclick: move |_| editor.write().exit_cc_edit(),
                        "✕"
                    }
                }
            }

            if microtonal {
                div {
                    style: "position: absolute; top: 6px; right: 8px; \
                            background: {theme::BANNER_WARN}; border: 1px solid {theme::GOLD}; \
                            border-radius: 4px; color: {theme::GOLD}; \
                            font-size: 10px; padding: 2px 7px; pointer-events: none;",
                    "{temperament_name}"
                }
            }
            // ── which-key ────────────────────────────────────────
            // Shown while a key sequence is half-typed: what can follow,
            // and what each one does. Bottom-left so it never covers the
            // pointer, which is what the pending gesture is anchored on.
            if !which_key().is_empty() {
                div {
                    "data-testid": "which-key",
                    style: format!(
                        "position: absolute; left: 10px; bottom: 10px; z-index: 40; \
                         min-width: 220px; max-height: 60%; overflow-y: auto; \
                         padding: 6px 0; border-radius: 6px; \
                         border: 1px solid {}; background: {}; color: {}; \
                         font-size: 11px; box-shadow: 0 6px 24px rgba(0,0,0,0.45);",
                        theme::PANEL_BORDER, theme::SURFACE_INSET, theme::TEXT,
                    ),
                    for c in which_key().into_iter() {
                        div {
                            key: "{c.key}",
                            style: "display: flex; align-items: baseline; gap: 8px; \
                                    padding: 2px 10px;",
                            span {
                                style: format!(
                                    "min-width: 34px; font-weight: 600; color: {};",
                                    theme::ACCENT,
                                ),
                                "{c.key}"
                            }
                            span {
                                style: format!(
                                    "color: {};",
                                    if c.is_group { theme::TEXT_DIM } else { theme::TEXT },
                                ),
                                if c.is_group { "+{c.label}" } else { "{c.label}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
