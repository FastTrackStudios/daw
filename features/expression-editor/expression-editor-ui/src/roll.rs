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
use expression_editor_core::{Editor, Viewport};
use input::InputCommand;
use keyboard_types::Modifiers;

use crate::interaction::{self, Drag};
use crate::menu_ui::{self, ContextMenu};
use crate::multitool_ui::{self, MultiTool};
use crate::{canvas, drawer, drawer::ModDrawer, guitar, keys, scroll, theme};
use crate::{draw_hint_of, handle_mark, BendFlow};
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
    /// The box the roll has been given, computed by the editor root from
    /// the host's reported space less all of its chrome. `None` until a
    /// host has said how much room there is, which leaves the viewport
    /// the document was built with.
    ///
    /// A prop rather than something read from `AVAILABLE` here, because
    /// the terms that decide it — the inspector's width, the lane
    /// strip's height — are owned up there. Subtracting a constant from
    /// the window down here is what made the roll an inspector too wide.
    #[props(default)]
    want: Option<Viewport>,
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

    // What can follow the keys typed so far. Empty means no sequence is
    // live, which is the overlay's cue to stay hidden.
    let mut which_key = use_signal(Vec::<keys::Continuation>::new);

    // Take the box the root worked out for us.
    //
    // An effect, not a poll and not an element measurement: it runs when
    // `want` changes and never touches the document, so there is no
    // relayout per tick and nothing to re-enter mid-dispatch. See
    // `crate::sizing` for why measuring directly is not on the table.
    use_effect(move || {
        let Some(want) = want else { return };
        if let Ok(mut ed) = editor.try_write()
            && ((ed.viewport.w - want.w).abs() >= 1.0 || (ed.viewport.h - want.h).abs() >= 1.0)
        {
            ed.resize(want);
        }
    });

    // While the drawer is open its target is locked: editing gestures
    // are blocked, but every navigation path stays live so the preview
    // can be auditioned in context.
    let locked = drawer.read().open;

    let ed = editor.read();
    let vp = ed.viewport;
    let rows = canvas::rows(&ed);
    let grid = canvas::grid_lines(&ed);
    let notes = canvas::note_rects(&ed);
    let curves = canvas::curve_paths(&ed);
    let boxes = canvas::lane_boxes(&ed);
    let guides = canvas::tuning_guides(&ed);
    let zone_guides = canvas::zone_guides(&ed);
    let keys = canvas::keyboard(&ed);
    let key_groups = canvas::key_groups(&ed, &keys);
    let ticks = canvas::ruler(&ed);
    let marker_flags = canvas::markers(&ed);
    let playhead = ed.playhead.map(|t| ed.camera.x(t));
    let razors = canvas::razor_rects(&ed);
    let refs = canvas::reference_rects(&ed);
    let handle_sets = canvas::note_handles(&ed);
    let take_wave = canvas::take_waveform(&ed);
    let sibilants = canvas::sibilant_bands(&ed);
    let sibilant_scope = ed.sibilant_scope;
    let draft_view = draft.read().as_ref().map(|d| canvas::draft_view(&ed, d));
    let sep_lines = canvas::separators(&ed);
    // Prototype (#161): the string roll's bend flow.
    let flow_mode = try_consume_context::<BendFlow>().unwrap_or_default();
    let flow = if flow_mode.on_row() {
        guitar::flow_paths(&ed)
    } else {
        Default::default()
    };
    let joins = guitar::joins(&ed);
    let bend_lane = flow_mode.draws_in_lane().then(|| guitar::bend_lane(&ed)).flatten();
    let midi_ref = canvas::midi_reference_rects(&ed);
    let midi_ref_front = ed.reference_to_front;
    // `R` brings references forward, the way `M` does for the MIDI
    // reference — with several parts on screen the quiet default is
    // sometimes too quiet to read against.
    let ref_opacity = if ed.refs_to_front { 0.95 } else { 0.45 };
    let cc_paths = canvas::cc_paths(&ed);
    // Notes recede while a controller is being edited: the roll is that
    // dimension's editing surface for the moment, and full-strength notes
    // would compete with the curve for the same pixels.
    let note_opacity = if ed.cc_editing() {
        ed.cc_display.note_dim
    } else {
        1.0
    };
    let cc_editing = ed.cc_edit;
    let microtonal = !ed.tuning.temperament.is_equal();
    let temperament_name = ed.tuning.temperament.name;
    let dimension = ed.dimension;
    let empty = ed.doc.notes.is_empty();
    // The empty-roll hint names the *actual* draw binding, read from
    // the live mouse map — which the host may have overlaid with the
    // user's own REAPER mouse modifiers. A hardcoded "press D" hint
    // goes stale the moment the map differs.
    let draw_hint = draw_hint_of(&ed);
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
                // `R` is momentary: references drop back the instant it
                // is released, so it can never be left on by accident.
                match e.key().to_string().as_str() {
                    "r" => editor.write().refs_to_front = false,
                    "m" => editor.write().reference_to_front = false,
                    _ => {}
                }
            },
            svg {
                // The roll is the surface every gesture lands on, so it
                // carries an id a test can aim a pointer at (#167).
                "data-testid": "roll",
                // **The declared size and the element box are the same
                // number, and that number is `vp`.** Both halves are
                // load-bearing, for one reason that took four failed
                // attempts to find:
                //
                // Blitz paints an inline `<svg>` as a *replaced element*
                // with a hardcoded `object-fit: contain`
                // (`blitz-paint/src/render.rs`, `draw_svg`). Everything
                // drawn is scaled by
                //
                //     element content box / usvg tree size
                //
                // So a mismatch between the two does not clip and does
                // not resize — it silently *scales*, which is why every
                // earlier combination traded one bug for another:
                //
                // - declared nothing: usvg takes the tree size from the
                //   content bounding box, which moves as the roll
                //   scrolls. The scale therefore changes as you scroll,
                //   and the roll grows or shrinks inside a frame that
                //   never moves. This is the bug `tests/geometry.rs`
                //   pins.
                // - declared `100%`: same, because a percentage is
                //   resolved against layout, not against the tree.
                // - `viewBox`: sets the tree size to the viewBox, so the
                //   scale is `layout / viewBox` — constant, but only 1
                //   when those agree, which nothing enforced.
                //
                // Setting the CSS box *and* the svg attributes from `vp`
                // makes the two sides of that ratio the same number by
                // construction. The scale is exactly 1, always, and no
                // longer depends on what is drawn. With no viewBox an
                // svg user unit is then a CSS pixel, so
                // `element_coordinates()` is a document coordinate
                // exactly — the pointer cannot drift.
                //
                // `vp` out of step with the *cell* is now the only
                // failure left, and it clips or leaves background rather
                // than scaling: visible and harmless, where a scaled
                // pointer is invisible and wrong. `available_space`
                // keeps the two in step.
                width: "{vp.w + canvas::GUTTER_W:.0}",
                height: "{vp.h + canvas::RULER_H:.0}",
                // `position: absolute` is what keeps the declared size
                // *used*. An in-flow svg is a replaced element, so the
                // cell shrink-fits it to preserve its aspect ratio —
                // laid out at 963x441 where it declared 1154x528, which
                // is a 0.83 scale on everything drawn and on every
                // pointer position. Out of flow with both axes given,
                // the used size is exactly the declared size, and a `vp`
                // larger than the cell is clipped by the cell's
                // `overflow: hidden` instead of silently rescaling.
                style: "position: absolute; left: 0; top: 0; display: block; \
                        touch-action: none; user-select: none; cursor: crosshair; \
                        width: {vp.w + canvas::GUTTER_W:.0}px; \
                        height: {vp.h + canvas::RULER_H:.0}px;",
                // Measured by `onresize`, not by a spawned
                // `get_client_rect().await`.
                //
                // The old code spawned a task to measure; that future
                // resolves inside a later event dispatch and re-enters
                // dioxus's document, panicking with "RefCell already
                // borrowed" at `native-dom/src/events.rs:164` — taking
                // the canvas down on the first click under Blitz, the
                // renderer REAPER uses (#167). dioxus documents the
                // constraint itself: background tasks must not touch
                // the document during event handling, and
                // `get_client_rect` has no `try_` form.
                //
                // `onresize` carries its size in the event, so there is
                // no task and nothing to re-enter — and unlike a
                // mount-time measurement it fires again when a REAPER
                // dock is dragged, which is the behaviour the spawned
                // version was reaching for in the first place.
                onresize: move |e: Event<ResizeData>| {
                    if let Ok(size) = e.data().get_content_box_size() {
                        let want = Viewport::new(
                            size.width - canvas::GUTTER_W,
                            size.height - canvas::RULER_H,
                        );
                        // Guarded: a no-op resize would invalidate the
                        // view for nothing, and `try_write` keeps a
                        // contended frame from panicking rather than
                        // relying on dispatch order.
                        if let Ok(mut ed) = editor.try_write()
                            && ed.viewport != want {
                                ed.resize(want);
                            }
                    }
                },
                // (the old onmounted measurement lived here)
                //
                // It used to `spawn` a `get_client_rect().await` and
                // resize from the result. That future resolves inside a
                // later event dispatch and re-enters dioxus's document,
                // which panics with "RefCell already borrowed" at
                // `native-dom/src/events.rs:164` — taking the canvas
                // down on the first click under Blitz, the renderer
                // REAPER uses (#167). dioxus documents the constraint
                // itself: background tasks must not touch the document
                // during event handling, and `get_client_rect` has no
                // `try_` form.
                //
                // The viewport comes from the host instead, through
                // `Editor::resize` — which the standalone runner and
                // the REAPER panel both already call. One source for
                // the size, and no background task racing the pointer.
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
                onwheel: move |e: WheelEvent| {
                    let delta = e.delta().strip_units();
                    let m = mods_of(e.modifiers());
                    // A wheel event carries no pointer position, so
                    // zoom anchors on the roll centre until we track the
                    // last pointer move.
                    let (x, y) = (vp.w * 0.5, vp.h * 0.5);
                    interaction::wheel(&mut editor.write(), x, y, delta.x, delta.y, m);
                    e.prevent_default();
                },

                // The roll is clipped to its own box so a pitch curve
                // that travels off-screen cannot paint over the ruler or
                // the keyboard.
                defs {
                    clipPath { id: "roll-clip",
                        rect { x: "0", y: "0", width: "{vp.w:.0}", height: "{vp.h:.0}" }
                    }
                }
                rect {
                    x: "0", y: "0",
                    width: "{vp.w + canvas::GUTTER_W:.0}",
                    height: "{vp.h + canvas::RULER_H:.0}",
                    fill: theme::BG,
                }

                g {
                    transform: "translate({canvas::GUTTER_W:.0} {canvas::RULER_H:.0})",
                    clip_path: "url(#roll-clip)",

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
                // One divider per family, so a kit reads as groups
                // rather than thirty-nine identical lanes.
                for r in rows.iter().filter(|r| r.starts_group) {
                    line {
                        key: "grp{r.row}",
                        x1: "0",
                        y1: "{r.y:.1}",
                        x2: "{vp.w:.0}",
                        y2: "{r.y:.1}",
                        stroke: theme::OCTAVE_LINE,
                        stroke_width: "1",
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

                // Pinned controller lanes, behind the notes.
                for (i, c) in cc_paths.iter().enumerate() {
                    g {
                        key: "cc{i}",
                        polygon {
                            points: "{c.fill}",
                            fill: c.color,
                            fill_opacity: "{c.opacity * 0.35:.3}",
                            pointer_events: "none",
                        }
                        polyline {
                            points: "{c.points}",
                            fill: "none",
                            stroke: c.color,
                            stroke_width: if c.active { "2.5" } else { "1.5" },
                            stroke_opacity: "{c.opacity:.2}",
                            pointer_events: "none",
                        }
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
                        fill: theme::lane_color(dimension),
                        fill_opacity: "0.05",
                        stroke: theme::lane_color(dimension),
                        stroke_opacity: "0.25",
                        stroke_width: "1",
                    }
                }

                // The take's own waveform, behind everything. Faint on
                // purpose: it is context, and it covers the full height
                // of the roll, so at any real strength it would fight
                // every note on screen.
                if let Some(wave) = take_wave.as_ref() {
                    polygon {
                        points: "{wave}",
                        fill: theme::REFERENCE,
                        fill_opacity: "0.13",
                        pointer_events: "none",
                    }
                }

                // Sibilants, shaded while their scope is armed — the
                // manual's "dark areas in the waveform", and the only
                // way to see what an amplitude drag is about to hit.
                for (i, (sx, ex)) in sibilants.iter().enumerate() {
                    {
                        let sw = (ex - sx).max(1.0);
                        rsx! {
                            // Darkened *and* edged. A dark band over an
                            // already-dark backdrop is nearly invisible,
                            // and the whole point is knowing exactly
                            // which spans an amplitude drag will hit.
                            rect {
                                key: "sib{i}",
                                x: "{sx:.1}", y: "0",
                                width: "{sw:.1}", height: "{vp.h:.0}",
                                fill: theme::SURFACE_DEEP,
                                fill_opacity: "0.72",
                                stroke: theme::BORDER_STRONG,
                                stroke_opacity: "0.85",
                                stroke_width: "1",
                                pointer_events: "none",
                            }
                        }
                    }
                }

                // The MIDI reference, behind the sung notes: outlines
                // rather than fills, because it is a target to agree
                // with and must never be mistaken for something the
                // pointer can grab.
                for (i, r) in midi_ref.iter().enumerate() {
                    rect {
                        key: "mref{i}",
                        x: "{r.x:.1}", y: "{r.y:.1}",
                        width: "{r.w:.1}", height: "{r.h:.1}",
                        rx: "2",
                        fill: theme::SURFACE_DEEP,
                        fill_opacity: if midi_ref_front { "0.85" } else { "0.5" },
                        stroke: theme::TEXT_BRIGHT,
                        stroke_opacity: if midi_ref_front { "0.95" } else { "0.45" },
                        stroke_width: "1",
                        pointer_events: "none",
                    }
                }

                // Reference tracks, behind the notes and never on top of
                // them: a reference you cannot edit must never be the
                // thing your pointer lands on first.
                for r in refs.iter() {
                    g {
                        key: "ref{r.track}-{r.id.0}",
                        opacity: "{ref_opacity:.2}",
                        rect {
                            x: "{r.x:.1}",
                            y: "{r.y:.1}",
                            width: "{r.w:.1}",
                            height: "{r.h:.1}",
                            rx: "2",
                            fill: r.fill.clone().unwrap_or_else(|| "none".into()),
                            fill_opacity: "0.20",
                            stroke: "{r.stroke}",
                            stroke_opacity: "0.75",
                            stroke_width: "1",
                        }
                    }
                }

                // Notes.
                for n in notes.iter() {
                    g {
                        key: "n{n.id.0}",
                        opacity: "{note_opacity:.2}",
                        if let Some(blob) = n.blob.as_ref() {
                            // A sung note: the body follows the
                            // amplitude envelope and rides the pitch
                            // contour, so the shape *is* the reading.
                            polygon {
                                points: "{blob}",
                                fill: n.fill,
                                fill_opacity: "{n.opacity + 0.25:.2}",
                                stroke: if n.selected { theme::SELECTED } else { n.fill },
                                stroke_width: if n.selected { "1.5" } else { "0.75" },
                            }
                            // The note's own pitch, as a hairline
                            // through the body. Without it there is
                            // nothing to read the wandering pitch track
                            // *against*, and the whole surface is about
                            // that difference.
                            if let Some(cy) = n.blob_center {
                                line {
                                    x1: "{n.x:.1}", y1: "{cy:.1}",
                                    x2: "{n.x + n.w:.1}", y2: "{cy:.1}",
                                    stroke: theme::TEXT_FAINT,
                                    stroke_opacity: "0.7",
                                    stroke_width: "0.75",
                                }
                            }
                        } else if let Some(head) = n.head.as_ref() {
                            polygon {
                                points: "{head}",
                                fill: n.fill,
                                fill_opacity: "{n.opacity + 0.15:.2}",
                                stroke: if n.selected { theme::SELECTED } else { n.fill },
                                stroke_width: if n.selected { "2" } else { "1" },
                            }
                        } else {
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
                        if n.ambiguous {
                            rect {
                                x: "{n.x:.1}",
                                y: "{n.y:.1}",
                                width: "{n.w:.1}",
                                height: "{n.h:.1}",
                                rx: "{(n.h * 0.28).min(4.0):.1}",
                                fill: theme::ZONE,
                                fill_opacity: "0.42",
                                pointer_events: "none",
                            }
                            text {
                                x: "{n.x + 4.0:.1}",
                                y: "{n.y + n.h * 0.5 + 4.0:.1}",
                                fill: theme::SELECTED,
                                font_size: "12",
                                pointer_events: "none",
                                "⚠"
                            }
                        }
                        if let Some(ribbon) = n.ribbon.as_ref() {
                            polygon {
                                points: "{ribbon}",
                                fill: theme::SELECTED,
                                fill_opacity: "0.18",
                                pointer_events: "none",
                            }
                        }
                        if let Some(label) = n.label.as_ref() {
                            text {
                                x: "{n.x + 5.0:.1}",
                                y: "{n.y + n.h * 0.5 + 3.5:.1}",
                                // Dark on the body: these labels sit on
                                // a saturated fill, and light text on a
                                // yellow string is unreadable.
                                fill: theme::SURFACE_DEEP,
                                fill_opacity: "0.85",
                                font_size: "10",
                                font_weight: "600",
                                pointer_events: "none",
                                "{label}"
                            }
                        }
                        if let Some(badge) = n.badge {
                            text {
                                x: "{n.x + 2.0:.1}",
                                y: "{n.y - 3.0:.1}",
                                fill: theme::ACCENT,
                                font_size: "9",
                                font_weight: "600",
                                pointer_events: "none",
                                "{badge}"
                            }
                        }
                        if n.legato {
                            // A tie arc off the right edge: legato is a
                            // relationship to the NEXT note, so it has
                            // to be drawn leaving the note.
                            path {
                                d: "M {n.x + n.w - 2.0:.1} {n.y + 1.0:.1} \
                                    q {n.h * 0.5:.1} {-n.h * 0.45:.1} {n.h:.1} 0",
                                fill: "none",
                                stroke: theme::ACCENT,
                                stroke_width: "1.5",
                                pointer_events: "none",
                            }
                        }
                        if let Some(cents) = n.cents {
                            text {
                                x: "{n.x + 3.0:.1}",
                                y: "{n.y - 3.0:.1}",
                                fill: theme::GOLD,
                                font_size: "9",
                                "{cents:+.0}¢"
                            }
                        }
                    }
                }

                // ── #161 prototype: bend flow ───────────────────────
                //
                // The string's own line, lifted off its row by the
                // bend. Drawn thick and in the string's colour so it
                // reads as "the string moved", not as an overlay.
                for (i, f) in flow.iter().enumerate() {
                    g {
                        key: "flow{i}",
                        polyline {
                            points: "{f.points}",
                            fill: "none",
                            stroke: f.color,
                            stroke_width: if f.selected { "3.5" } else { "2.5" },
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            stroke_opacity: "0.95",
                            pointer_events: "none",
                        }
                        // The height of the bend, where it peaks. A
                        // guitarist reads bends as "full" and "half",
                        // not as a shape — the number is the datum and
                        // the curve is how it got there.
                        if let Some(label) = f.peak_label.as_ref() {
                            text {
                                x: "{f.peak_at.0 + 3.0:.1}",
                                y: "{f.peak_at.1 - 4.0:.1}",
                                fill: theme::ACCENT,
                                font_size: "9",
                                font_weight: "600",
                                pointer_events: "none",
                                "{label}"
                            }
                        }
                    }
                }

                // Joins between two notes on one string. A hammer-on
                // gets an arc and a letter; a slide gets a straight
                // connector — deliberately two different marks, so the
                // pictures can be compared.
                for (i, j) in joins.iter().enumerate() {
                    g {
                        key: "join{i}",
                        path {
                            d: "{j.d}",
                            fill: "none",
                            stroke: j.color,
                            stroke_width: "1.5",
                            pointer_events: "none",
                        }
                        if let guitar::JoinKind::Hopo(letter) = j.kind {
                            text {
                                x: "{j.label_at.0 - 3.0:.1}",
                                y: "{j.label_at.1:.1}",
                                fill: theme::ACCENT,
                                font_size: "9",
                                font_weight: "700",
                                pointer_events: "none",
                                "{letter}"
                            }
                        }
                    }
                }

                // The dimension variant: the same motion on an absolute
                // semitone axis, under the roll.
                if let Some(dimension) = bend_lane.as_ref() {
                    g {
                        rect {
                            x: "0", y: "{dimension.y:.1}",
                            width: "{vp.w:.0}", height: "{dimension.h:.1}",
                            fill: theme::SURFACE_DEEP,
                            fill_opacity: "0.9",
                            stroke: theme::BORDER_STRONG,
                            stroke_width: "1",
                            pointer_events: "none",
                        }
                        for (gi, (gy, label)) in dimension.guides.iter().enumerate() {
                            g {
                                key: "bg{gi}",
                                line {
                                    x1: "0", y1: "{gy:.1}",
                                    x2: "{vp.w:.0}", y2: "{gy:.1}",
                                    stroke: theme::BORDER_STRONG,
                                    stroke_width: "1",
                                    stroke_dasharray: "4 4",
                                    pointer_events: "none",
                                }
                                text {
                                    x: "3", y: "{gy - 2.0:.1}",
                                    fill: theme::TEXT_FAINT,
                                    font_size: "8",
                                    pointer_events: "none",
                                    "{label}"
                                }
                            }
                        }
                        for (pi, f) in dimension.paths.iter().enumerate() {
                            polyline {
                                key: "bp{pi}",
                                points: "{f.points}",
                                fill: "none",
                                stroke: f.color,
                                stroke_width: "2",
                                stroke_linecap: "round",
                                pointer_events: "none",
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

                // Expression curves — overlays first, active dimension last.
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

                // Razor areas sit above the notes: they are a
                // statement about a region, and the region has to be
                // legible even where it is dense with material.
                // Note handles, in front of the notes they belong to.
                // Pointer events stay off: `pointer_down` hit-tests them
                // from the same layout, so letting the SVG intercept
                // would give two competing answers.
                for set in handle_sets.iter() {
                    g {
                        key: "hs{set.id.0}",
                        pointer_events: "none",
                        // The temporary note, when one is open here.
                        if let Some((sx, ex)) = set.scope {
                            {
                                let body = set.rects.first();
                                let (ty, th) = body.map(|b| (b.y, b.h)).unwrap_or((0.0, 0.0));
                                let sw = (ex - sx).abs().max(1.0);
                                let sxx = sx.min(ex);
                                rsx! {
                                    rect {
                                        x: "{sxx:.1}", y: "{ty:.1}",
                                        width: "{sw:.1}", height: "{th:.1}",
                                        fill: theme::ACCENT, fill_opacity: "0.28",
                                        stroke: theme::ACCENT, stroke_width: "1",
                                    }
                                }
                            }
                        }
                        for hr in set.rects.iter() {
                            {
                                // The body handle is the note itself and
                                // needs no chrome; only the strips draw.
                                let is_body = hr.handle
                                    == expression_editor_core::Handle::Pitch;
                                let (cx, cy) = hr.center();
                                let half = (hr.w * 0.28).min(9.0);
                                rsx! {
                                    if !is_body {
                                        g {
                                            key: "h{set.id.0}-{hr.handle:?}",
                                            rect {
                                                x: "{hr.x:.1}", y: "{hr.y:.1}",
                                                width: "{hr.w:.1}", height: "{hr.h:.1}",
                                                rx: "2",
                                                fill: theme::CONTROL,
                                                fill_opacity: "0.92",
                                                stroke: theme::BORDER_STRONG,
                                                stroke_opacity: "0.9",
                                                stroke_width: "1",
                                            }
                                            // A glyph rather than a
                                            // label: at this size text
                                            // is unreadable, and the
                                            // mark says which axis the
                                            // handle moves.
                                            {
                                                let hollow = hr.handle
                                                    == expression_editor_core::Handle::Amplitude
                                                    && sibilant_scope;
                                                let mark =
                                                    handle_mark(hr.handle, cx, cy, half, hollow);
                                                // Filled unless it is
                                                // the hollow amplitude
                                                // circle, whose whole
                                                // job is to be empty.
                                                let fill = if hr.handle
                                                    == expression_editor_core::Handle::Amplitude
                                                    && !hollow
                                                {
                                                    theme::HANDLE
                                                } else {
                                                    "none"
                                                };
                                                rsx! {
                                                    path {
                                                        d: "{mark}",
                                                        stroke: theme::HANDLE,
                                                        stroke_width: "1.6",
                                                        fill,
                                                        stroke_linecap: "round",
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // The pitch drawing, over everything it edits.
                if let Some(dv) = draft_view.as_ref() {
                    g {
                        pointer_events: "none",
                        // What was there before, thin and underneath, so
                        // the change is visible rather than remembered.
                        if !dv.original.is_empty() {
                            polyline {
                                points: "{dv.original}",
                                fill: "none",
                                stroke: theme::TEXT_FAINT,
                                stroke_width: "1",
                                stroke_opacity: "0.8",
                            }
                        }
                        if !dv.line.is_empty() {
                            polyline {
                                points: "{dv.line}",
                                fill: "none",
                                stroke: theme::ACCENT,
                                stroke_width: "2",
                            }
                        }
                        for (i, (ax, ay)) in dv.anchors.iter().enumerate() {
                            circle {
                                key: "anch{i}",
                                cx: "{ax:.1}", cy: "{ay:.1}", r: "4",
                                fill: theme::BG,
                                stroke: theme::ACCENT,
                                stroke_width: "2",
                            }
                        }
                    }
                }

                // Timing separators, over everything: in timing mode
                // the boundary is what the pointer is addressing.
                for (i, s) in sep_lines.iter().enumerate() {
                    g {
                        key: "sep{i}",
                        pointer_events: "none",
                        line {
                            x1: "{s.x:.1}", y1: "0",
                            x2: "{s.x:.1}", y2: "{vp.h:.0}",
                            stroke: s.color,
                            stroke_width: "2",
                        }
                        // The tick that splits the two drag laws. Above
                        // it the left side stretches and the rest
                        // slides; below it both sides stretch.
                        line {
                            x1: "{s.x - 6.0:.1}", y1: "{s.tick_y:.1}",
                            x2: "{s.x + 6.0:.1}", y2: "{s.tick_y:.1}",
                            stroke: theme::SELECTED,
                            stroke_width: "2",
                        }
                    }
                }

                for (i, r) in razors.iter().enumerate() {
                    g {
                        key: "rz{i}",
                        rect {
                            x: "{r.x:.1}",
                            y: "{r.y:.1}",
                            width: "{r.w:.1}",
                            height: "{r.h:.1}",
                            fill: theme::RAZOR,
                            fill_opacity: "0.20",
                            stroke: theme::RAZOR,
                            stroke_width: "1",
                            pointer_events: "none",
                        }
                        // Hard edges — this is where notes get sliced.
                        line {
                            x1: "{r.x:.1}", y1: "{r.y:.1}",
                            x2: "{r.x:.1}", y2: "{r.y + r.h:.1}",
                            stroke: theme::RAZOR, stroke_width: "2",
                            pointer_events: "none",
                        }
                        line {
                            x1: "{r.x + r.w:.1}", y1: "{r.y:.1}",
                            x2: "{r.x + r.w:.1}", y2: "{r.y + r.h:.1}",
                            stroke: theme::RAZOR, stroke_width: "2",
                            pointer_events: "none",
                        }
                    }
                }

                if let Some(t) = playhead {
                    line {
                        x1: "{t:.1}", y1: "0",
                        x2: "{t:.1}", y2: "{vp.h:.0}",
                        stroke: theme::PLAYHEAD,
                        stroke_width: "1",
                        stroke_opacity: "0.75",
                    }
                }

                } // end roll group

                // ── timeline ruler ───────────────────────────────────
                g {
                    transform: "translate({canvas::GUTTER_W:.0} 0)",
                    rect {
                        x: "0", y: "0",
                        width: "{vp.w:.0}", height: "{canvas::RULER_H:.0}",
                        fill: theme::PANEL,
                    }
                    for (i, tk) in ticks.iter().enumerate() {
                        line {
                            key: "tk{i}",
                            x1: "{tk.x:.1}",
                            y1: if tk.bar { "0" } else { "{canvas::RULER_H * 0.55:.0}" },
                            x2: "{tk.x:.1}",
                            y2: "{canvas::RULER_H:.0}",
                            stroke: if tk.bar { theme::TEXT_DIM } else { theme::PANEL_BORDER },
                            stroke_width: "1",
                        }
                    }
                    for (i, tk) in ticks.iter().enumerate() {
                        if let Some(label) = tk.label.as_ref() {
                            text {
                                key: "tl{i}",
                                x: "{tk.x + 3.0:.1}",
                                y: "11",
                                fill: theme::TEXT_DIM,
                                font_size: "9",
                                "{label}"
                            }
                        }
                    }
                    for (i, mk) in marker_flags.iter().enumerate() {
                        g {
                            key: "mk{i}",
                            line {
                                x1: "{mk.x:.1}", y1: "0",
                                x2: "{mk.x:.1}", y2: "{canvas::RULER_H:.0}",
                                stroke: theme::ACCENT, stroke_width: "1",
                            }
                            text {
                                x: "{mk.x + 3.0:.1}",
                                y: "{canvas::RULER_H - 6.0:.0}",
                                fill: theme::ACCENT,
                                font_size: "9",
                                "{mk.label}"
                            }
                        }
                    }
                    if let Some(t) = playhead {
                        // A downward triangle, so the transport head
                        // reads at a glance against the tick marks.
                        polygon {
                            points: "{t - 5.0:.1},0 {t + 5.0:.1},0 {t:.1},{canvas::RULER_H:.0}",
                            fill: theme::PLAYHEAD,
                        }
                    }
                    line {
                        x1: "0", y1: "{canvas::RULER_H:.0}",
                        x2: "{vp.w:.0}", y2: "{canvas::RULER_H:.0}",
                        stroke: theme::PANEL_BORDER, stroke_width: "1",
                    }
                }

                // ── piano-key gutter ─────────────────────────────────
                g {
                    transform: "translate(0 {canvas::RULER_H:.0})",
                    rect {
                        x: "0", y: "0",
                        width: "{canvas::GUTTER_W:.0}", height: "{vp.h:.0}",
                        fill: theme::GUTTER_BG,
                    }
                    for k in keys.iter() {
                        rect {
                            key: "k{k.row}",
                            x: "0",
                            y: "{k.y:.1}",
                            // A hairline gap so adjacent white keys stay
                            // distinguishable at small row heights.
                            width: if k.black { "{canvas::GUTTER_W * 0.62:.0}" } else { "{canvas::GUTTER_W:.0}" },
                            height: "{(k.h - 1.0).max(1.0):.2}",
                            fill: if k.black { theme::KEY_BLACK } else { theme::KEY_WHITE },
                        }
                    }
                    for k in keys.iter() {
                        if let Some(label) = k.label.as_ref() {
                            text {
                                key: "kl{k.row}",
                                x: "{canvas::GUTTER_W - 4.0:.0}",
                                y: "{k.y + k.h * 0.5 + 3.0:.1}",
                                text_anchor: "end",
                                fill: if k.black { theme::TEXT_DIM } else { theme::KEY_LABEL },
                                font_size: "9",
                                "{label}"
                            }
                        }
                    }
                    // The piece name, braced over its hands. Drawn after
                    // the hand labels so the brace sits on top of the
                    // key fills rather than under them.
                    for g in key_groups.iter() {
                        path {
                            key: "gb{g.label}",
                            d: "M 26 {g.y + 1.5:.1} L 22 {g.y + 1.5:.1} L 22 {g.y + g.h - 1.5:.1} L 26 {g.y + g.h - 1.5:.1}",
                            fill: "none",
                            stroke: theme::TEXT_DIM,
                            stroke_width: "1",
                        }
                        // The label is centred on the span, which is
                        // exactly where the seam between the two keys
                        // falls — so it gets its own chip to sit on.
                        rect {
                            x: "1",
                            y: "{g.y + g.h * 0.5 - 6.0:.1}",
                            width: "20",
                            height: "12",
                            fill: theme::KEY_WHITE,
                        }
                        // Horizontal, not rotated: Blitz does not apply
                        // an SVG transform to text, so a rotated label
                        // renders as nothing at all.
                        text {
                            x: "4",
                            y: "{g.y + g.h * 0.5 + 3.0:.1}",
                            text_anchor: "start",
                            fill: theme::KEY_LABEL,
                            font_size: "9",
                            "{g.label}"
                        }
                    }
                    line {
                        x1: "{canvas::GUTTER_W:.0}", y1: "0",
                        x2: "{canvas::GUTTER_W:.0}", y2: "{vp.h:.0}",
                        stroke: theme::PANEL_BORDER, stroke_width: "1",
                    }
                }

                // The corner where ruler and gutter meet.
                rect {
                    x: "0", y: "0",
                    width: "{canvas::GUTTER_W:.0}", height: "{canvas::RULER_H:.0}",
                    fill: theme::PANEL,
                }
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
