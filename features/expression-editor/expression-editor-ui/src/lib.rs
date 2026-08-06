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
pub mod demo;
pub mod drawer;
pub mod interaction;
pub mod theme;
pub mod toolbar;
pub mod widgets;

pub use expression_editor_core as core;
pub use drawer::ModDrawer;
pub use interaction::Drag;

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
    #[props(default)] initial_drawer: Option<ModDrawer>,
) -> Element {
    let drag = use_signal(Drag::default);
    let drawer = use_signal(|| initial_drawer.clone().unwrap_or_default());

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
            toolbar::ChordBox { editor }
            Canvas { editor, drag, drawer }
            LaneStrip { editor }
            toolbar::StatusBar { editor }
        }
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
        Chrome::Key(
            ed.camera
                .pitch_at(y - canvas::RULER_H, ed.viewport)
                .round() as i32,
        )
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
fn Canvas(editor: Signal<Editor>, drag: Signal<Drag>, drawer: Signal<ModDrawer>) -> Element {
    let mut editor = editor;
    let mut drag = drag;
    let mut drawer = drawer;
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
    let ticks = canvas::ruler(&ed);
    let marker_flags = canvas::markers(&ed);
    let playhead = ed.playhead.map(|t| ed.camera.x(t));
    let razors = canvas::razor_rects(&ed);
    let microtonal = !ed.tuning.temperament.is_equal();
    let temperament_name = ed.tuning.temperament.name;
    let lane = ed.lane;
    let empty = ed.doc.notes.is_empty();
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
            tabindex: "0",
            onkeydown: move |e: KeyboardEvent| {
                let key = e.key().to_string();
                let m = mods_of(e.modifiers());
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
            svg {
                style: "display: block; width: 100%; height: 100%; \
                        touch-action: none; user-select: none; cursor: crosshair;",
                view_box: "0 0 {vp.w + canvas::GUTTER_W:.0} {vp.h + canvas::RULER_H:.0}",
                preserve_aspect_ratio: "none",
                onmounted: move |e| {
                    let data = e.data();
                    spawn(async move {
                        if let Ok(r) = data.get_client_rect().await {
                            editor.write().resize(Viewport::new(
                                r.width() - canvas::GUTTER_W,
                                r.height() - canvas::RULER_H,
                            ));
                        }
                    });
                },
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
                                fill: "#fff",
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
                                fill: "#0b0b10",
                                fill_opacity: "0.75",
                                font_size: "10",
                                pointer_events: "none",
                                "{label}"
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

                // Razor areas sit above the notes: they are a
                // statement about a region, and the region has to be
                // legible even where it is dense with material.
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
                                fill: if k.black { theme::TEXT_DIM } else { "#33333f" },
                                font_size: "9",
                                "{label}"
                            }
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
                        div { "Press D for Note Draw, then click the grid" }
                        div { "V resets the view \u{b7} B opens modulation" }
                    }
                }
            }

            drawer::ModulationDrawer { editor, drawer }

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
                    background: #101017; border-top: 1px solid {theme::PANEL_BORDER};",
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
pub fn default_overlays() -> Vec<Lane> {
    vec![Lane::Pitch]
}
