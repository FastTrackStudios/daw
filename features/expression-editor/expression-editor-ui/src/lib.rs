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
use expression_editor_core::tools::Mods;
use expression_editor_core::{Editor, Dimension, Viewport};
use keyboard_types::Modifiers;

pub mod canvas;
pub mod demo;
pub mod drawer;
pub mod arp_panel;
pub mod curve_editor;
pub mod drag;
pub mod envelopes;
pub mod velocity_panel;
pub mod guitar;
pub mod inspector;
pub mod interaction;
pub mod menu_ui;
pub mod multitool_ui;
pub mod quantize_panel;
pub mod stack;
pub mod switcher;
pub mod theme;
pub mod toolbar;
pub mod widgets;

pub use drawer::ModDrawer;
pub use guitar::BendFlow;
pub use expression_editor_core as core;
pub use interaction::Drag;
pub use menu_ui::ContextMenu;
pub use multitool_ui::MultiTool;

/// The empty-roll hint line, derived from the live mouse map: which
/// modifier+drag on open canvas inserts or paints a note.
fn draw_hint_of(ed: &Editor) -> String {
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
    let drag = use_signal(Drag::default);
    let drawer = use_signal(|| initial_drawer.clone().unwrap_or_default());
    let mut inspector_open = use_signal(|| true);
    let multi = use_signal(|| initial_multi.clone().unwrap_or_default());
    let menu_state = use_signal(ContextMenu::default);
    let draft = use_signal(|| initial_draft.clone());
    let mut pending = use_signal(|| None::<menu_ui::Pending>);

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
            toolbar::ChordBox { editor }
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
fn handle_mark(
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
fn Canvas(
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
            // `flex: 1 1 auto` + a floor, not `flex: 1 1 0`: when the
            // parent's height does not resolve (a plugin window before
            // its first resize, a headless mount), a zero basis would
            // collapse the canvas to nothing and the svg is the only
            // child that could have given it height back.
            style: "position: relative; flex: 1 1 auto; min-height: 360px; \
                    overflow: hidden; outline: none;",
            // The cell is what a host measures to drive `Editor::resize`
            // (element resize events don't exist under dioxus-native, so
            // the REAPER panel polls this element's layout size): the
            // svg itself is fixed to the viewport and would only ever
            // report the size it was last given.
            "data-testid": "canvas-cell",
            tabindex: "0",
            onkeydown: move |e: KeyboardEvent| {
                let key = e.key().to_string();
                let m = mods_of(e.modifiers());

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
                // Sized in px to exactly the viewBox, never stretched
                // (`100%` + `preserveAspectRatio: none` scaled the
                // coordinate space whenever the element's size drifted
                // from `vp` — and element_coordinates are element px, so
                // every gesture landed offset by the stretch factor).
                // With a 1:1 mapping the mouse is exact even while `vp`
                // is stale; a stale `vp` only costs clipped or
                // letterboxed rendering until the host resizes the
                // editor, and the parent's overflow:hidden absorbs that.
                style: "display: block; \
                        width: {vp.w + canvas::GUTTER_W:.0}px; \
                        height: {vp.h + canvas::RULER_H:.0}px; \
                        touch-action: none; user-select: none; cursor: crosshair;",
                view_box: "0 0 {vp.w + canvas::GUTTER_W:.0} {vp.h + canvas::RULER_H:.0}",
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
                        div { "Scroll zooms \u{b7} Alt+scroll pans \u{b7} Ctrl+Alt+scroll scrolls pitch" }
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
        }
    }
}

/// Write a strip value from a pointer position.
///
/// A free function over `Signal` (which is `Copy`) rather than a shared
/// closure — otherwise both pointer handlers fight over one `FnMut`.
fn strip_write(mut editor: Signal<Editor>, h: f64, x: f64, y: f64) {
    let v = (1.0 - y / h).clamp(0.0, 1.0);
    let hit: Vec<_> = {
        let ed = editor.read();
        let rx = x - canvas::GUTTER_W;
        let t = ed.camera.t_at(rx);
        ed.doc
            .notes
            .iter()
            // A generous grab window around the onset: the stem is only
            // a few pixels wide, and this is a value edit, not a
            // precision selection.
            .filter(|n| {
                let dx = (ed.camera.x(n.start) - rx).abs();
                dx <= 8.0 || (n.start <= t && n.end > t && dx <= 40.0)
            })
            .map(|n| n.id)
            .collect()
    };
    if hit.is_empty() {
        return;
    }
    let edit = match editor.read().strip_lane {
        expression_editor_core::StripLane::OffVelocity => {
            expression_editor_core::Edit::SetOffVelocity {
                notes: hit,
                velocity: v,
            }
        }
        _ => expression_editor_core::Edit::SetVelocity {
            notes: hit,
            velocity: v,
        },
    };
    editor.write().apply_live(&edit);
}

/// The velocity / CC lane strip below the roll.
///
/// Shares the roll's horizontal camera exactly — a stem must sit under
/// its note — but has its own vertical scale, because the value being
/// edited has nothing to do with pitch.
#[component]
fn LaneStrip(editor: Signal<Editor>) -> Element {
    let mut editor = editor;
    let mut drag = use_signal(|| None::<(f64, f64)>);

    let ed = editor.read();
    let h = ed.lane_strip_h;
    if h <= 0.0 {
        return rsx! {};
    }
    let vp = ed.viewport;
    let stems = canvas::stems(&ed, h);
    let curves = canvas::strip_curves(&ed, h);
    let guides = canvas::strip_guides(h);
    let label = ed.strip_lane.label();
    let per_note = ed.strip_lane.is_per_note();
    drop(ed);

    rsx! {
        div {
            style: "position: relative; flex: 0 0 auto; height: {h}px; \
                    background: {theme::SURFACE_BAR}; border-top: 1px solid {theme::PANEL_BORDER};",
            svg {
                style: "display: block; width: 100%; height: 100%; \
                        touch-action: none; user-select: none; cursor: ns-resize;",
                view_box: "0 0 {vp.w + canvas::GUTTER_W:.0} {h:.0}",
                preserve_aspect_ratio: "none",
                onpointerdown: move |e: PointerEvent| {
                    let c = e.data().element_coordinates();
                    if !per_note {
                        return;
                    }
                    editor.write().begin_gesture();
                    drag.set(Some((c.x, c.y)));
                    strip_write(editor, h, c.x, c.y);
                },
                onpointermove: move |e: PointerEvent| {
                    if drag.read().is_none() {
                        return;
                    }
                    let c = e.data().element_coordinates();
                    strip_write(editor, h, c.x, c.y);
                },
                onpointerup: move |_| drag.set(None),
                onpointerleave: move |_| drag.set(None),

                // The gutter column, so the strip lines up with the roll.
                rect {
                    x: "0", y: "0",
                    width: "{canvas::GUTTER_W:.0}", height: "{h:.0}",
                    fill: theme::GUTTER_BG,
                }
                text {
                    x: "6", y: "14",
                    fill: theme::TEXT_DIM, font_size: "9",
                    "{label}"
                }

                g {
                    transform: "translate({canvas::GUTTER_W:.0} 0)",
                    for (i, (y, major)) in guides.iter().enumerate() {
                        line {
                            key: "sg{i}",
                            x1: "0", y1: "{y:.1}",
                            x2: "{vp.w:.0}", y2: "{y:.1}",
                            stroke: if *major { theme::GRID_BEAT } else { theme::GRID_SUB },
                            stroke_width: "1",
                        }
                    }
                    for (i, s) in stems.iter().enumerate() {
                        g {
                            key: "st{i}",
                            rect {
                                x: "{s.x:.1}",
                                y: "{s.y:.1}",
                                width: "{s.w:.1}",
                                height: "{s.h.max(1.0):.1}",
                                fill: s.color,
                                fill_opacity: if s.muted { "0.2" } else { "0.85" },
                            }
                            // A cap on the selected stems, so the ones a
                            // drag will actually move are obvious.
                            if s.selected {
                                rect {
                                    x: "{s.x - 1.0:.1}",
                                    y: "{s.y - 2.0:.1}",
                                    width: "{s.w + 2.0:.1}",
                                    height: "3",
                                    fill: theme::SELECTED,
                                }
                            }
                        }
                    }
                    for (i, c) in curves.iter().enumerate() {
                        polyline {
                            key: "sc{i}",
                            points: "{c.points}",
                            fill: "none",
                            stroke: c.color,
                            stroke_width: "1.5",
                            stroke_opacity: if c.selected { "1" } else { "0.6" },
                        }
                    }
                }
            }
        }
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
