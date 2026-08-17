//! The top bar, the chord box, and the status bar.
//!
//! The top bar carries only what changes *what a gesture does* — tool,
//! dimension, curve shape, history, view. Everything that is a setting
//! rather than a mode (grid, key, tuning, bend range, lane strip) moved
//! to the status bar, because those get set once a session and were
//! crowding the controls that get touched constantly.
//!
//! One consequence worth keeping: the top bar now fits on a single row
//! at plugin width, so the canvas no longer loses a wrapped second row.

use dioxus::prelude::*;
use expression_editor_core::doc::Dimension;
use expression_editor_core::{chord, Editor, ModeFamily, Shape, StripLane, Tool};

use crate::drawer::ModDrawer;
use crate::interaction::{self, Drag};
use crate::theme;

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

/// A run of buttons that reads as one control.
///
/// Segmenting is what makes a dense bar scannable: the eye finds a
/// group before it finds a button, so related choices share one outline
/// instead of floating as separate pills.
#[component]
fn Segment(children: Element) -> Element {
    rsx! {
        div {
            // `flex: 0 0 auto` is load-bearing. A segment allowed to
            // shrink does not drop cells or ellipsize — it compresses
            // each one until the icon overlaps its own label, which
            // reads as a rendering bug rather than as a full toolbar.
            style: "display: flex; flex: 0 0 auto; align-items: stretch; \
                    border: 1px solid {theme::PANEL_BORDER}; border-radius: 6px; \
                    overflow: hidden; background: {theme::SURFACE_BAR};",
            {children}
        }
    }
}

/// One cell inside a [`Segment`].
#[component]
fn Seg(
    active: bool,
    title: String,
    #[props(default = false)] accent: bool,
    #[props(default)] color: Option<String>,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let fg = color.unwrap_or_else(|| {
        if active {
            theme::SELECTED.to_string()
        } else {
            theme::TEXT.to_string()
        }
    });
    let bg = if active {
        if accent {
            theme::CONTROL_ACTIVE
        } else {
            theme::CONTROL_HOVER
        }
    } else {
        "transparent"
    };
    rsx! {
        button {
            style: "display: flex; align-items: center; justify-content: center; \
                    min-width: 24px; height: 24px; padding: 0 7px; border: none; \
                    border-right: 1px solid {theme::PANEL_BORDER}; \
                    background: {bg}; color: {fg}; font-size: 11px; \
                    line-height: 1; cursor: pointer; user-select: none; \
                    white-space: nowrap;",
            title: "{title}",
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}

fn divider() -> Element {
    rsx! {
        div {
            style: "width: 1px; height: 18px; background: {theme::PANEL_BORDER}; \
                    margin: 0 3px; flex: 0 0 auto;"
        }
    }
}

// ── top bar ──────────────────────────────────────────────────────────

#[component]
pub fn Toolbar(editor: Signal<Editor>, drag: Signal<Drag>, drawer: Signal<ModDrawer>) -> Element {
    let mut editor = editor;
    let mut drawer = drawer;

    let ed = editor.read();
    let tool = ed.tool;
    let dimension = ed.dimension;
    let overlays = ed.overlays.clone();
    let shape = ed.shape;
    let can_undo = ed.can_undo();
    let can_redo = ed.can_redo();
    let mod_open = drawer.read().open;
    let mode = ed.mode;
    let stacked = ed.stacked;
    let bend = ed.doc.bend_range;
    drop(ed);

    rsx! {
        div {
            "data-testid": "toolbar",
            // A fixed height that scrolls, not a wrapping one that grows.
            //
            // Wrapping made the bar's height a function of the window
            // *width*, and the roll is sized by subtracting this bar
            // from the window — so a narrow window silently stole a row
            // from the roll and the two disagreed about where the roll
            // began. `overflow-x: auto` keeps everything reachable at
            // any width without the height ever moving.
            // `border-box`, so the constant is the row's *total* height.
            // With the default `content-box` the padding and border
            // stack on top, and the roll was sized against a number 11px
            // smaller than the row it was subtracting.
            style: "display: flex; flex: 0 0 auto; flex-wrap: nowrap; \
                    box-sizing: border-box; height: {crate::sizing::TOOLBAR_H}px; \
                    align-items: center; gap: 5px; overflow-x: auto; \
                    padding: 5px 8px; background: {theme::PANEL}; \
                    border-bottom: 1px solid {theme::PANEL_BORDER}; \
                    font-family: system-ui, sans-serif;",

            // The mode leads: everything after it is conditional on
            // what the editor currently is.
            //
            // One segment per family, so the switcher reads as "which
            // kind of material is this" before "which surface" — the
            // MIDI-shaped modes on the left, the two analysed-audio
            // ones on the right.
            for (i, family) in ModeFamily::ALL.into_iter().enumerate() {
                // Separator *between* groups, not after each: a
                // trailing divider would collide with the fixed one
                // below and read as a double rule.
                if i > 0 {
                    {divider()}
                }
                Segment {
                    for m in family.modes().iter().copied() {
                        Seg {
                            key: "m{m:?}",
                            active: mode == m,
                            accent: true,
                            title: format!("{} mode ({} family)", m.label(), family.label()),
                            onclick: move |_| editor.write().set_mode(m),
                            span {
                                style: "display: flex; align-items: center; gap: 5px;",
                                svg {
                                    view_box: "0 0 16 16",
                                    style: "width: 13px; height: 13px; flex: 0 0 auto;",
                                    path {
                                        d: theme::mode_icon(m),
                                        fill: "none",
                                        stroke: "currentColor",
                                        stroke_width: "1.3",
                                        stroke_linecap: "round",
                                        stroke_linejoin: "round",
                                    }
                                }
                                "{m.label()}"
                            }
                        }
                    }
                }
            }

            {divider()}

            // Stack toggle. Next to the mode buttons because it answers
            // the same question from the other side: those pick how one
            // track is drawn, this shows every track drawn its own way.
            // Hidden with one track, where a stack of one is just the
            // roll with less room.
            if editor.read().tracks.len() > 1 {
                Segment {
                    Seg {
                        active: stacked,
                        accent: true,
                        title: "Show every track on one timeline".to_string(),
                        onclick: move |_| {
                            let now = editor.read().stacked;
                            editor.write().stacked = !now;
                        },
                        // Icon only. The toolbar is already full at the
                        // width a plugin window gets, and this is a view
                        // toggle rather than something you hunt for by
                        // name.
                        svg {
                            view_box: "0 0 16 16",
                            style: "width: 13px; height: 13px; flex: 0 0 auto;",
                            path {
                                // Three stacked lanes.
                                d: "M2 4h12 M2 8h12 M2 12h12",
                                fill: "none",
                                stroke: "currentColor",
                                stroke_width: "1.3",
                                stroke_linecap: "round",
                            }
                        }
                    }
                }

                {divider()}
            }

            Segment {
                for t in Tool::ALL {
                    Seg {
                        key: "{t:?}",
                        active: tool == t,
                        accent: true,
                        title: t.label().to_string(),
                        onclick: move |_| editor.write().tool = t,
                        "{tool_glyph(t)}"
                    }
                }
            }

            {divider()}

            // Dimension selection and overlay visibility, joined: the name
            // picks the dimension to edit, the dot toggles it as an overlay.
            // Hidden outside MPE and Audio — plain MIDI cannot carry
            // per-note pressure, so offering the control would promise
            // an edit the format will drop.
            if mode.has_expression_lanes() {
            Segment {
                for l in Dimension::ALL {
                    // One wrapper per dimension so the key lands on a single
                    // node; Dioxus only honours a key on the first node
                    // in a block, and this loop emits a pair.
                    div {
                        key: "dimension{l:?}",
                        style: "display: flex; align-items: stretch;",
                    Seg {
                        active: dimension == l,
                        color: theme::lane_color(l).to_string(),
                        title: format!("Edit {}", theme::lane_label(l)),
                        onclick: move |_| editor.write().dimension = l,
                        "{theme::lane_label(l)}"
                    }
                    Seg {
                        active: overlays.contains(&l),
                        title: format!("Show {} as overlay", theme::lane_label(l)),
                        onclick: move |_| {
                            let mut ed = editor.write();
                            match ed.overlays.iter().position(|&x| x == l) {
                                Some(i) => { ed.overlays.remove(i); }
                                None => ed.overlays.push(l),
                            }
                        },
                        if overlays.contains(&l) { "●" } else { "○" }
                    }
                    }
                }
            }
            }

            // MPE channel management, only where channels are the
            // mechanism that makes per-note expression possible.
            if mode.has_mpe_channels() {
                Segment {
                    Seg {
                        active: false,
                        title: "Spread selected notes across member channels (R)".to_string(),
                        onclick: move |_| {
                            let notes = editor.read().selection.notes.clone();
                            if !notes.is_empty() {
                                editor.write().apply(
                                    &expression_editor_core::Edit::AssignChannels {
                                        notes,
                                        seed: 0x5EED,
                                    },
                                );
                            }
                        },
                        "Spread ch"
                    }
                    Seg {
                        active: false,
                        title: "Channel down".to_string(),
                        onclick: move |_| {
                            let notes = editor.read().selection.notes.clone();
                            editor.write().apply(
                                &expression_editor_core::Edit::NudgeChannel { notes, delta: -1 },
                            );
                        },
                        "ch−"
                    }
                    Seg {
                        active: false,
                        title: "Channel up".to_string(),
                        onclick: move |_| {
                            let notes = editor.read().selection.notes.clone();
                            editor.write().apply(
                                &expression_editor_core::Edit::NudgeChannel { notes, delta: 1 },
                            );
                        },
                        "ch+"
                    }
                    Seg {
                        active: false,
                        title: "Pitch-bend range — must match the instrument".to_string(),
                        onclick: move |_| {
                            // The values instruments actually use.
                            const RANGES: [f64; 4] = [2.0, 12.0, 24.0, 48.0];
                            let mut ed = editor.write();
                            let cur = ed.doc.bend_range;
                            let i = RANGES.iter().position(|r| *r == cur).unwrap_or(3);
                            ed.doc.bend_range = RANGES[(i + 1) % RANGES.len()];
                        },
                        span { style: "min-width: 42px;", "±{bend:.0}" }
                    }
                }

                {divider()}
            }

            {divider()}

            Segment {
                for s in Shape::ALL {
                    Seg {
                        key: "{s:?}",
                        active: shape == s,
                        title: s.label().to_string(),
                        onclick: move |_| {
                            let d = drag.read().clone();
                            interaction::apply_shape(&mut editor.write(), &d, s);
                        },
                        "{shape_glyph(s)}"
                    }
                }
            }

            // Pushed right: history, and things that act on the view
            // rather than the material.
            div {
                style: "margin-left: auto; display: flex; align-items: center; gap: 5px;",
                Segment {
                    Seg {
                        active: false,
                        title: "Undo".to_string(),
                        onclick: move |_| { editor.write().undo(); },
                        span { style: if can_undo { "" } else { "opacity: 0.3;" }, "↶" }
                    }
                    Seg {
                        active: false,
                        title: "Redo".to_string(),
                        onclick: move |_| { editor.write().redo(); },
                        span { style: if can_redo { "" } else { "opacity: 0.3;" }, "↷" }
                    }
                }
                Segment {
                    Seg {
                        active: mod_open,
                        accent: true,
                        title: "Modulation (B)".to_string(),
                        onclick: move |_| {
                            let mut dw = drawer.write();
                            if dw.open {
                                dw.cancel(&mut editor.write());
                            } else if dw.open_on(&editor.read()) {
                                dw.preview(&mut editor.write());
                            }
                        },
                        "Mod"
                    }
                }
                Segment {
                    Seg {
                        active: false,
                        title: "Zoom to the passage under the cursor (F)".to_string(),
                        onclick: move |_| {
                            let (t, row) = {
                                let ed = editor.read();
                                (
                                    ed.playhead
                                        .unwrap_or_else(|| ed.camera.t_at(ed.viewport.w * 0.5)),
                                    ed.camera.vertical.center,
                                )
                            };
                            editor.write().smart_zoom(
                                expression_editor_core::ZoomModes::NOTE_AREA,
                                t,
                                row,
                            );
                        },
                        "Zoom"
                    }
                    Seg {
                        active: false,
                        title: "Reset view (V)".to_string(),
                        onclick: move |_| editor.write().reset_view(),
                        "Fit"
                    }
                }
                Fps { editor }
            }
        }
    }
}

// ── chord box ────────────────────────────────────────────────────────

/// The selection row: what is selected, what chord it makes, and the
/// two Melodyne blend controls for it.
///
/// Reads the selection, or the notes under the playhead when nothing is
/// selected — so it says something useful while you navigate rather
/// than going blank the moment you deselect.
///
/// The blend sliders live here rather than in the status bar because
/// they act on *the selection*, which is exactly what this row is
/// about; the status bar is for session-wide settings.
#[component]
pub fn ChordBox(editor: Signal<Editor>) -> Element {
    let ed = editor.read();
    let single = ed.selection.notes.first().copied();
    let analysis = single.and_then(|id| ed.doc.note(id)).map(|n| {
        let ups = ed.doc.time_base.units_per_second(ed.bpm);
        let d = expression_editor_core::blob::decompose(&n.pitch, n.start, n.end, 64, ups, 0.0);
        (
            expression_editor_core::tuning::note_name(n.row),
            n.channel,
            n.zone_count(),
            d.modulation_depth(),
            d.drift_extent(),
        )
    });
    let pitches = ed.chord_pitches();
    let name = ed.current_chord().map(|c| chord::name(&c));
    let note_names: Vec<String> = pitches
        .iter()
        .map(|&p| expression_editor_core::tuning::note_name(p))
        .collect();
    let from_selection = !ed.selection.notes.is_empty();
    drop(ed);

    rsx! {
        div {
            "data-testid": "chord-box",
            style: "display: flex; flex: 0 0 auto; align-items: center; gap: 10px; \
                    box-sizing: border-box; height: {crate::sizing::CHORD_H}px; \
                    padding: 0 10px; \
                    background: {theme::SURFACE_BAR}; \
                    border-bottom: 1px solid {theme::PANEL_BORDER}; \
                    font-family: system-ui, sans-serif; overflow: hidden;",

            span {
                style: "font-size: 9px; letter-spacing: 0.08em; text-transform: uppercase; \
                        color: {theme::TEXT_DIM}; flex: 0 0 auto;",
                "Chord"
            }
            if let Some(name) = name {
                span {
                    style: "font-size: 15px; font-weight: 600; color: {theme::GOLD}; \
                            min-width: 88px; flex: 0 0 auto;",
                    "{name}"
                }
            } else {
                span {
                    style: "font-size: 12px; color: {theme::TEXT_FAINT}; min-width: 88px; flex: 0 0 auto;",
                    if pitches.len() == 1 { "single note" } else { "—" }
                }
            }
            // The constituent notes, so a surprising name can be checked
            // against what is actually sounding.
            div {
                style: "display: flex; gap: 4px; align-items: center; overflow: hidden;",
                for (i, n) in note_names.iter().enumerate() {
                    span {
                        key: "cn{i}",
                        style: "font-size: 10px; font-family: ui-monospace, monospace; \
                                color: {theme::TEXT}; background: {theme::CONTROL}; \
                                border: 1px solid {theme::PANEL_BORDER}; \
                                border-radius: 3px; padding: 1px 5px; flex: 0 0 auto;",
                        "{n}"
                    }
                }
            }
            div {
                style: "margin-left: auto; display: flex; align-items: center; gap: 10px; \
                        flex: 0 0 auto; font-family: ui-monospace, monospace; \
                        font-size: 10px; color: {theme::TEXT_DIM};",

                if let Some((name, channel, zones, vibrato, drift)) = analysis {
                    span { style: "color: {theme::TEXT};", "{name}" }
                    if let Some(ch) = channel {
                        span { "ch {ch}" }
                    }
                    if zones > 1 {
                        span { style: "color: {theme::ZONE};", "{zones} zones" }
                    }
                    span { "vib {vibrato * 100.0:.0}¢" }
                    span { "drift {drift * 100.0:+.0}¢" }
                    // The blend sliders and technique controls live
                    // in the inspector; this row keeps the readouts.
                } else {
                    span { style: "font-size: 9px;",
                        if from_selection { "from selection" } else { "at playhead" }
                    }
                }
            }
        }
    }
}

// ── status bar ───────────────────────────────────────────────────────

/// Settings, not modes: grid, key, tuning, the lane strip, and readouts.
#[component]
pub fn StatusBar(editor: Signal<Editor>) -> Element {
    let mut editor = editor;
    let ed = editor.read();
    let grid_label = ed.grid.label();
    let grid_on = ed.grid.enabled;
    let triplet = ed.grid.triplet;
    let temperament = ed.tuning.temperament.name;
    let key_pc = ed.tuning.key_pc;
    let snap_12tet = ed.tuning.snap_12tet;
    let bend = ed.doc.bend_range;
    let strip = ed.strip_lane;
    let strip_on = ed.lane_strip_h > 0.0;
    let count = ed.selection.notes.len();
    let ambiguous = ed.doc.notes.iter().filter(|n| n.ambiguous).count();
    let razors = ed.razor.areas.len();
    let mouse_preset = ed.mouse.name;
    drop(ed);

    rsx! {
        div {
            "data-testid": "status-bar",
            style: "display: flex; flex: 0 0 auto; align-items: center; gap: 5px; \
                    box-sizing: border-box; height: {crate::sizing::STATUS_H}px; \
                    padding: 0 8px; background: {theme::PANEL}; \
                    border-top: 1px solid {theme::PANEL_BORDER}; \
                    color: {theme::TEXT_DIM}; font-size: 10px; \
                    font-family: system-ui, sans-serif; overflow: hidden;",

            Segment {
                Seg {
                    active: grid_on,
                    accent: true,
                    title: "Snap to the editor's own grid".to_string(),
                    onclick: move |_| {
                        let on = editor.read().grid.enabled;
                        editor.write().grid.enabled = !on;
                    },
                    "⊞"
                }
                Seg {
                    active: false,
                    title: "Coarser (1)".to_string(),
                    onclick: move |_| editor.write().grid.coarser(),
                    "−"
                }
                Seg {
                    active: false,
                    title: "Grid division".to_string(),
                    onclick: move |_| {},
                    span {
                        style: "min-width: 38px; font-family: ui-monospace, monospace; \
                                color: {theme::TEXT};",
                        "{grid_label}"
                    }
                }
                Seg {
                    active: false,
                    title: "Finer (2)".to_string(),
                    onclick: move |_| editor.write().grid.finer(),
                    "+"
                }
                Seg {
                    active: triplet,
                    title: "Triplet grid (T)".to_string(),
                    onclick: move |_| {
                        let on = editor.read().grid.triplet;
                        editor.write().grid.triplet = !on;
                    },
                    "T"
                }
            }

            Segment {
                Seg {
                    active: false,
                    title: "Key — click to cycle".to_string(),
                    onclick: move |_| {
                        let mut ed = editor.write();
                        ed.tuning.key_pc = (ed.tuning.key_pc + 1).rem_euclid(12);
                    },
                    span {
                        style: "min-width: 20px; color: {theme::TEXT}; font-weight: 600;",
                        "{expression_editor_core::tuning::pitch_class_name(key_pc)}"
                    }
                }
                Seg {
                    active: false,
                    title: "Temperament — click to cycle".to_string(),
                    onclick: move |_| {
                        let presets = expression_editor_core::tuning::PRESETS;
                        let mut ed = editor.write();
                        let i = presets
                            .iter()
                            .position(|t| t.name == ed.tuning.temperament.name)
                            .unwrap_or(0);
                        ed.tuning.temperament = presets[(i + 1) % presets.len()].clone();
                    },
                    span { style: "min-width: 92px;", "{temperament}" }
                }
                Seg {
                    active: snap_12tet,
                    title: "Also offer ordinary semitone centres".to_string(),
                    onclick: move |_| {
                        let on = editor.read().tuning.snap_12tet;
                        editor.write().tuning.snap_12tet = !on;
                    },
                    "12TET"
                }
            }

            Segment {
                Seg {
                    active: strip_on,
                    accent: true,
                    title: "Show the velocity / CC dimension".to_string(),
                    onclick: move |_| {
                        let h = editor.read().lane_strip_h;
                        editor.write().lane_strip_h = if h > 0.0 { 0.0 } else { 96.0 };
                    },
                    "▁"
                }
                Seg {
                    active: false,
                    title: "Which dimension the strip shows".to_string(),
                    onclick: move |_| {
                        let cur = editor.read().strip_lane;
                        let all = StripLane::ALL;
                        let i = all.iter().position(|s| *s == cur).unwrap_or(0);
                        editor.write().strip_lane = all[(i + 1) % all.len()];
                    },
                    span { style: "min-width: 54px; color: {theme::TEXT};", "{strip.label()}" }
                }
            }

            span { "Bend {bend:.0}" }
            span { "{mouse_preset}" }

            div {
                style: "margin-left: auto; display: flex; align-items: center; gap: 12px; \
                        font-family: ui-monospace, monospace; flex: 0 0 auto;",
                if razors > 0 {
                    span { style: "color: {theme::RAZOR};", "razor {razors}" }
                }
                if ambiguous > 0 {
                    span {
                        style: "color: {theme::ZONE};",
                        "⚠ {ambiguous} share a channel — writes blocked"
                    }
                }
                span { "{count} selected" }
            }
        }
    }
}

/// The update rate, top right.
///
/// Measured from the interval between renders of *this* component. It
/// took a signal to make that mean anything: `Fps {}` has no props, and
/// dioxus memoizes a component whose props have not changed, so it
/// rendered exactly once at mount and then never again. The average was
/// therefore taken over a single sample forever, which is why the
/// readout sat at `— fps` no matter how hard the surface was working.
///
/// Reading `editor` here is what fixes it: a subscription re-renders the
/// component whether or not its props changed, so this now ticks once
/// per editor update — which during a drag is the interaction rate, the
/// number worth watching.
///
/// It is deliberately **not** the renderer's frame rate. Blitz paints on
/// its own schedule and dioxus-native exposes no per-frame hook, so
/// there is nothing here that could honestly count painted frames; the
/// label says "ups" rather than "fps" so it cannot be mistaken for one.
/// A true frame counter comes with the custom-widget roll, whose `paint`
/// is called once per frame by the renderer itself.
///
/// Averaged over a short window rather than shown per-frame. A raw
/// reciprocal jitters far too much to read, and a number nobody can read
/// is not a diagnostic.
#[component]
fn Fps(editor: Signal<Editor>) -> Element {
    // The subscription. Reading any field is enough — the component is
    // then re-run on every write to the editor, which is the event being
    // counted. Cheap on purpose: a clone of the document here would make
    // the meter itself the slow thing it is measuring.
    let _tick = editor.read().doc.notes.len();
    // Deliberately **not** a signal.
    //
    // A signal written during render schedules another render, which
    // writes it again: the surface spins at 100% forever and never
    // settles. It is not a subtle failure — it hung the headless test
    // suite outright, every gesture test timing out at thirty seconds
    // while this component re-rendered underneath them.
    //
    // The measurement is not view state anyway. It describes how often
    // the view is being drawn, so it must be a passive observer of that
    // and never a cause of it. A plain `RefCell` behind `use_hook` gives
    // per-component storage that reading and writing cannot invalidate.
    let state = use_hook(|| {
        std::rc::Rc::new(std::cell::RefCell::new((
            Vec::<f64>::new(),
            None::<std::time::Instant>,
        )))
    });

    let now = std::time::Instant::now();
    let s = {
        let (samples, last) = &mut *state.borrow_mut();
        if let Some(previous) = last.replace(now) {
            let ms = now.duration_since(previous).as_secs_f64() * 1000.0;
            // Drop the idle gaps: a frame after a second of nothing is
            // not a 1 fps frame, it is the first frame of a new burst.
            if ms < 500.0 {
                samples.push(ms);
                if samples.len() > 30 {
                    samples.remove(0);
                }
            }
        }
        samples.clone()
    };

    let fps = if s.is_empty() {
        None
    } else {
        let mean = s.iter().sum::<f64>() / s.len() as f64;
        (mean > 0.0).then(|| 1000.0 / mean)
    };

    // Quiet while it is healthy, and only asks for attention when it is
    // not: dim above a screen refresh, gold where a drag starts to feel
    // it, and the razor colour below the rate at which motion reads as
    // motion. A readout that is loud when nothing is wrong gets ignored
    // when something is.
    let color = match fps {
        Some(f) if f >= 50.0 => theme::TEXT_DIM,
        Some(f) if f >= 24.0 => theme::GOLD,
        Some(_) => theme::RAZOR,
        None => theme::TEXT_DIM,
    };
    rsx! {
        div {
            "data-testid": "fps",
            title: "Editor updates per second, averaged over the last 30",
            style: format!(
                "min-width: 46px; text-align: right; font-size: 10px; \
                 font-variant-numeric: tabular-nums; color: {color};",
            ),
            match fps {
                Some(f) => format!("{f:.0} ups"),
                None => "— ups".to_string(),
            }
        }
    }
}
