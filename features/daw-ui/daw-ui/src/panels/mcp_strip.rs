//! The WALTER-themeable MCP strip.
//!
//! Replaces the flex-stacked `ChannelStrip` for the mixer console: every
//! element is positioned by the active [`McpLayout`]'s 8-value anchor coords
//! (`theme.mcp`), colours resolve through the per-element overrides
//! ([`McpColors`]) with token fallbacks, and `define_parameter`-style knobs
//! (`mcp_show_pan`, `mcp_strip_width`) bend the layout — so a REAPER
//! `Layout … EndLayout` block can drive this strip 1:1.
//!
//! Element ids rendered (REAPER vocabulary): `mcp.trackidx mcp.label
//! mcp.volume mcp.volume.label mcp.pan mcp.meter mcp.mute mcp.solo mcp.recarm
//! mcp.phase mcp.fx mcp.fxbyp mcp.io mcp.env mcp.folder`. Hidden elements are
//! zero-sized coords, the WALTER idiom.

use crate::core::sensitivity::{DragSensitivity, ModifierKeys};
use crate::panels::model::TrackView;
use crate::prelude::*;
use crate::theming::{Color, FaderMode, McpLayout, ThemeState, ToggleKind, use_theme};
use crate::widgets::mixer::RoutingButton;

/// One mixer strip, laid out by the theme's MCP context. With `tcp: true`
/// the strip renders the theme's **TCP context** instead (same element
/// vocabulary, laid out as a track-control row) and fills its parent box —
/// the row owns the size so rows align with the arrange lanes.
#[component]
pub fn McpStrip(
    track: TrackView,
    /// Named layout to use (`None` = the theme's first/default layout).
    #[props(default)]
    layout: Option<String>,
    #[props(default)] disabled: bool,
    /// Render the theme's `tcp.*` context (track-control row).
    #[props(default)]
    tcp: bool,
    /// The actual px box this strip fills, when the parent knows it (TCP
    /// rows: sidebar width × track height). With an imported REAPER theme
    /// this re-runs WALTER at that exact size — REAPER's resize model — so
    /// flow themes (Reapertips) rewrap/shrink/cull elements per the real
    /// width instead of springing the natural-size bake.
    #[props(default)]
    size: Option<(f32, f32)>,
    /// Horizontal-zoom multiplier on the strip width (the mixer's per-strip
    /// "track width"). `1.0` = natural. With an imported theme + `size`, the
    /// engine re-flows at the scaled width; otherwise it just widens the box
    /// (WALTER-anchored elements spread per the layout).
    #[props(default = 1.0)]
    width_scale: f32,
    /// When set (mixer mode), render a right-edge drag handle; the callback
    /// gets `(client_x, base_width_px)` on mousedown so the mixer can begin a
    /// per-strip width resize.
    #[props(default)]
    on_resize_start: Option<EventHandler<(f64, u32)>>,
) -> Element {
    let theme = use_theme().theme;
    let ctx_name = if tcp { "tcp" } else { "mcp" };
    let mcp = if tcp {
        theme.tcp.clone()
    } else {
        theme.mcp.clone()
    };
    let armed = (track.record_arm)();
    let selected = (track.selected)();
    // Inline rename (REAPER: double-click the name field edits it in place).
    let mut editing_name = use_signal(|| false);
    let mut name_sig = track.name;
    // Layout resolution: with a runtime engine + a known px box, re-evaluate
    // the theme at this exact size (state scalars included — selection
    // switches the theme's selected chrome). Otherwise fall back to the
    // baked layouts (`@armed` variant when the theme has one).
    let runtime = match (&theme.engine, size) {
        (Some(engine), Some((w, h))) if w > 0.0 && h > 0.0 => {
            // Engine layouts are named without the `@state` suffix.
            let base = mcp.layout(layout.as_deref()).name.clone();
            let base = base.split('@').next().unwrap_or(&base).to_string();
            engine.layout_at(
                ctx_name,
                &base,
                w,
                h,
                crate::theming::StripState { armed, selected },
            )
        }
        _ => None,
    };
    let l = runtime.unwrap_or_else(|| {
        let base = mcp.layout(layout.as_deref()).clone();
        if armed {
            let name = format!("{}@armed", base.name);
            mcp.layouts
                .iter()
                .find(|x| x.name == name)
                .cloned()
                .unwrap_or(base)
        } else {
            base
        }
    });
    let nat = l.size;

    let st = ThemeState::new().track(track.color.as_deref().and_then(Color::hex));
    let strip = theme.strip(&st);
    let accent = strip.accent;

    // `mcp_strip_width` param overrides the layout's natural width (0 = off),
    // then a per-track width override (drag-resize) and the horizontal-zoom
    // multiplier apply (the mixer's "track width").
    let natural_w = match mcp.param("mcp_strip_width") {
        Some(w) if w > 0.0 => w,
        _ => l.size.0,
    };
    let base_w = (track.strip_width)().map(|w| w as f32).unwrap_or(natural_w);
    let strip_w = base_w * width_scale.max(0.1);
    let show_pan = mcp.param("mcp_show_pan") != Some(0.0);

    let opacity = if disabled { "0.5" } else { "1.0" };

    // ── element styles ──
    // Colour resolution: per-layout WALTER colours → theme-level overrides →
    // token fallbacks. Track-colour sentinels resolve to the live accent.
    let label_fg = l
        .colors
        .label
        .map(|c| c.fg)
        .or(mcp.colors.label.map(|c| c.fg))
        .unwrap_or(accent.on())
        .resolve_track(accent);
    // With theme-drawn chrome (`mcp.custom.*` name-bar boxes) behind it, the
    // label itself stays transparent; bare layouts get the accent bar.
    let label_bg = l
        .colors
        .label
        .and_then(|c| c.bg)
        .or(mcp.colors.label.and_then(|c| c.bg))
        .unwrap_or(if l.customs.is_empty() {
            accent
        } else {
            Color::rgba(0, 0, 0, 0)
        })
        .resolve_track(accent);
    let vol_label_fg = l
        .colors
        .volume_label
        .map(|c| c.fg)
        .or(mcp.colors.volume_label.map(|c| c.fg))
        .unwrap_or(strip.value_text)
        .resolve_track(accent);
    let idx_fg = l
        .colors
        .trackidx
        .map(|c| c.fg)
        .or(mcp.colors.trackidx.map(|c| c.fg))
        .unwrap_or(theme.tokens.text_faint)
        .resolve_track(accent);

    let fader_accent = mcp.colors.volume.unwrap_or(accent);
    let pan_color = mcp.colors.pan; // Knob themes itself; reserved for importer use.
    let _ = pan_color;

    // Image skin (imported REAPER themes): per-element image overrides.
    let skin = mcp.skin.clone().unwrap_or_default();

    let display_pct = format!("{:.0}%", (track.fader)().clamp(0.0, 1.0) * 100.0);

    // Strip body: with theme-drawn chrome the panel base is the plain mixer
    // surface (REAPER's customs paint the strip; tinting comes from the
    // theme's own colour boxes). Bare layouts keep the FTS track tint.
    let (body, border) = if l.customs.is_empty() {
        (strip.body, strip.border)
    } else {
        (theme.tokens.surface, theme.tokens.border)
    };

    // MCP strips size themselves (the mixer is a horizontal stack of natural-
    // width strips); TCP rows fill the parent row box, which owns the height
    // so rows stay aligned with the arrange lanes.
    let container = if tcp {
        format!(
            "position:relative; width:100%; height:100%; overflow:hidden; \
             opacity:{opacity}; user-select:none; \
             background:{body}; border-bottom:1px solid {border};",
            body = body.css(),
            border = border.css(),
        )
    } else {
        format!(
            "position:relative; flex:0 0 auto; width:{strip_w}px; height:100%; \
             min-width:{minw}px; min-height:{minh}px; overflow:hidden; \
             margin-right:-1px; opacity:{opacity}; user-select:none; \
             background:{body}; border:1px solid {border};",
            minw = l.min_size.0,
            minh = l.min_size.1,
            body = body.css(),
            border = border.css(),
        )
    };

    rsx! {
        div {
            style: container,

            // Right-edge width handle (mixer only): drag to resize this strip.
            // `fts-rz` brightens it on hover (see arrange_view::RESIZE_CSS,
            // rendered alongside in the workspace).
            if let Some(cb) = on_resize_start {
                div {
                    class: "fts-rz",
                    style: "position:absolute; top:0; bottom:0; right:0; width:9px; \
                            cursor:ew-resize; z-index:6; background:rgba(255,255,255,0.05);",
                    onmousedown: move |evt: MouseEvent| {
                        if evt.trigger_button()
                            == Some(dioxus_elements::input_data::MouseButton::Primary)
                        {
                            evt.stop_propagation();
                            cb.call((evt.client_coordinates().x, base_w.round() as u32));
                        }
                    },
                }
            }

            // Theme-drawn chrome: `mcp.custom.*` boxes, painted first in
            // WALTER z-order. Only the `.color` *background* (second four
            // components) fills the box — the first four are the custom's
            // text colour. Image-backed customs render their sliced art.
            // Track-colour sentinels resolve to the accent.
            for c in l.customs.iter() {
                if c.image.is_some() || c.bg.is_some() {
                    div {
                        key: "{c.name}",
                        style: format!(
                            "{pos}{fill} pointer-events:none;",
                            pos = c.coord.css_position(nat),
                            fill = c
                                .bg
                                .map(|b| format!(" background:{};", b.resolve_track(accent).css()))
                                .unwrap_or_default(),
                        ),
                        if let Some(img) = &c.image {
                            {skin_fill(img, c.coord.w, c.coord.h)}
                        }
                    }
                }
            }

            // mcp.trackidx
            if !l.trackidx.is_hidden() {
                div {
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:{justify}; \
                         color:{fg}; font-size:{fs}px; font-weight:{fw}; pointer-events:none;",
                        pos = l.trackidx.css_position(nat),
                        justify = flex_justify(&l.trackidx_margin),
                        fg = idx_fg.css(),
                        fs = l.trackidx_font.size,
                        fw = l.trackidx_font.weight,
                    ),
                    "{track.id + 1}"
                }
            }

            // mcp.recinput / mcp.recmode / mcp.recmon / mcp.fxin — the record
            // row. Inert until the view-model carries record state; themed +
            // positioned by the layout.
            if !l.recinput.is_hidden() {
                div {
                    title: "Record input",
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:{justify}; \
                         padding:{pad}; color:{fg}; font-size:10px; \
                         white-space:nowrap; overflow:hidden;",
                        pos = l.recinput.css_position(nat),
                        justify = flex_justify(&l.recinput_margin),
                        pad = l.recinput_margin.css_padding(),
                        fg = theme.tokens.text_dim.css(),
                    ),
                    // 9-slice bg: the dropdown art's pink run marks its
                    // stretch zone — only the text area grows, the arrow
                    // stays at native size.
                    if let Some(bg) = skin.recinput_bg.as_ref() {
                        {skin_fill(bg, l.recinput.w, l.recinput.h)}
                    }
                    span { style: "position:relative;", "Input 1" }
                }
            }
            if !l.recmode.is_hidden() {
                McpFlag { pos: l.recmode.css_position(nat), box_w: l.recmode.w, box_h: l.recmode.h, glyph: "REC", title: "Record mode", skin: skin.recmode.as_ref().map(|b| b.on.normal.clone()) }
            }
            // mcp.recmon — no image in most themes (REAPER draws it
            // built-in): a speaker icon on the standard chip.
            if !l.recmon.is_hidden() {
                div {
                    title: "Record monitoring",
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:center; \
                         border-radius:3px; background:{bg}; border:1px solid {bd};",
                        pos = l.recmon.css_position(nat),
                        bg = theme.tokens.surface_sunken.css(),
                        bd = theme.tokens.border.css(),
                    ),
                    svg {
                        width: "12",
                        height: "12",
                        view_box: "0 0 24 24",
                        polygon {
                            points: "3,9 8,9 14,4 14,20 8,15 3,15",
                            fill: theme.tokens.text_dim.css(),
                        }
                        path {
                            d: "M17 8 Q20 12 17 16",
                            stroke: theme.tokens.text_dim.css(),
                            stroke_width: "2",
                            fill: "none",
                            stroke_linecap: "round",
                        }
                    }
                }
            }
            if !l.fxin.is_hidden() {
                McpFlag { pos: l.fxin.css_position(nat), box_w: l.fxin.w, box_h: l.fxin.h, glyph: "FX", title: "Input FX", skin: skin.fxin.as_ref().map(|b| b.off.normal.clone()) }
            }

            // mcp.pan — horizontal slider per `mcp.pan.fadermode` (REAPER's
            // MCP default), knob otherwise.
            if !l.pan.is_hidden() && show_pan {
                if l.pan_fadermode == FaderMode::Horizontal {
                    McpFader {
                        pos: l.pan.css_position(nat),
                        value: track.pan,
                        mode: FaderMode::Horizontal,
                        accent: mcp.colors.pan.unwrap_or(accent),
                        skin_bg: skin.panbg.clone(),
                        skin_thumb: skin.panthumb.clone(),
                        disabled,
                    }
                } else {
                    McpKnob {
                        pos: l.pan.css_position(nat),
                        box_w: l.pan.w,
                        box_h: l.pan.h,
                        value: track.pan,
                        skin: skin.pan_knob.clone(),
                        accent: mcp.colors.pan.unwrap_or(accent).resolve_track(accent),
                        disabled,
                    }
                }
            }

            // mcp.solo / mcp.mute / mcp.recarm
            if !l.solo.is_hidden() {
                McpToggle { pos: l.solo.css_position(nat), box_w: l.solo.w, box_h: l.solo.h, active: track.solo, kind: ToggleKind::Solo, skin: skin.solo.clone(), disabled }
            }
            if !l.mute.is_hidden() {
                McpToggle { pos: l.mute.css_position(nat), box_w: l.mute.w, box_h: l.mute.h, active: track.mute, kind: ToggleKind::Mute, skin: skin.mute.clone(), disabled }
            }
            if !l.recarm.is_hidden() {
                McpToggle { pos: l.recarm.css_position(nat), box_w: l.recarm.w, box_h: l.recarm.h, active: track.record_arm, kind: ToggleKind::RecArm, skin: skin.recarm.clone(), disabled }
            }

            // mcp.volume — knob when `.fadermode` forces one (the REAPER 7
            // default TCP volume), fader otherwise.
            if !l.volume.is_hidden() {
                if l.volume_fadermode == FaderMode::Knob {
                    McpKnob {
                        pos: l.volume.css_position(nat),
                        box_w: l.volume.w,
                        box_h: l.volume.h,
                        value: track.fader,
                        skin: skin.vol_knob.clone(),
                        accent: fader_accent.resolve_track(accent),
                        disabled,
                    }
                } else {
                    McpFader {
                        pos: l.volume.css_position(nat),
                        value: track.fader,
                        mode: l.volume_fadermode,
                        accent: fader_accent,
                        skin_bg: skin.volbg.clone(),
                        skin_thumb: skin.volthumb.clone(),
                        disabled,
                    }
                }
            }

            // mcp.meter
            if !l.meter.is_hidden() {
                McpMeter {
                    pos: l.meter.css_position(nat),
                    level: track.level,
                    level_right: track.stereo.then_some(track.level_right),
                    peak: track.peak,
                    skin_strip: skin.meter_strip.clone(),
                    skin_bg: skin.meter_bg.clone(),
                }
            }

            // mcp.volume.label — fader readout.
            if !l.volume_label.is_hidden() {
                div {
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:{justify}; \
                         color:{fg}; font-size:{fs}px; font-weight:{fw}; \
                         font-variant-numeric:tabular-nums; pointer-events:none;",
                        pos = l.volume_label.css_position(nat),
                        justify = flex_justify(&l.volume_label_margin),
                        fg = vol_label_fg.css(),
                        fs = l.volume_label_font.size,
                        fw = l.volume_label_font.weight,
                    ),
                    "{display_pct}"
                }
            }

            // mcp.io — routing. Image-skinned when the theme ships one.
            if !l.io.is_hidden() {
                if let Some(io) = skin.io.clone() {
                    // div, not button: blitz's UA button background paints
                    // over background-image.
                    div {
                        title: "Routing",
                        style: format!(
                            "{pos} cursor:pointer;",
                            pos = l.io.css_position(nat),
                        ),
                        {skin_button(&io.normal, l.io.w, l.io.h)}
                    }
                } else {
                    div {
                        style: format!(
                            "{pos} display:flex; align-items:center; justify-content:center;",
                            pos = l.io.css_position(nat),
                        ),
                        RoutingButton {
                            parent_send: track.parent_send,
                            sends: track.sends,
                            receives: track.receives,
                            disabled,
                            on_click: move |_| {},
                        }
                    }
                }
            }

            // mcp.phase / mcp.fx / mcp.fxbyp / mcp.env / mcp.folder — inert
            // until the view-model carries their state; themed + positionable.
            if !l.phase.is_hidden() {
                McpStateFlag {
                    pos: l.phase.css_position(nat),
                    box_w: l.phase.w,
                    box_h: l.phase.h,
                    glyph: "ø",
                    title: "Phase invert",
                    active: track.phase,
                    skin: skin.phase.clone(),
                    disabled,
                }
            }
            if !l.fx.is_hidden() {
                McpFlag { pos: l.fx.css_position(nat), box_w: l.fx.w, box_h: l.fx.h, glyph: "FX", title: "FX chain", skin: skin.fx.as_ref().map(|b| b.off.normal.clone()) }
            }
            if !l.fxbyp.is_hidden() {
                McpFlag { pos: l.fxbyp.css_position(nat), box_w: l.fxbyp.w, box_h: l.fxbyp.h, glyph: "BYP", title: "FX bypass", skin: skin.fxbyp.as_ref().map(|b| b.on.normal.clone()) }
            }
            if !l.env.is_hidden() {
                McpFlag { pos: l.env.css_position(nat), box_w: l.env.w, box_h: l.env.h, glyph: "ENV", title: "Envelopes", skin: skin.env.as_ref().map(|b| b.off.normal.clone()) }
            }
            if !l.folder.is_hidden() {
                McpFlag { pos: l.folder.css_position(nat), box_w: l.folder.w, box_h: l.folder.h, glyph: "▼", title: "Folder", skin: skin.folder.as_ref().map(|b| if track.is_folder { b.on.normal.clone() } else { b.off.normal.clone() }) }
            }

            // mcp.label — the track-name field. Double-click edits in place
            // (REAPER's rename behaviour); Enter/Escape or blur commits.
            if !l.label.is_hidden() {
                if editing_name() {
                    input {
                        r#type: "text",
                        value: "{track.name}",
                        autofocus: true,
                        style: format!(
                            "{pos} padding:{pad}; background:{bg}; color:{fg}; \
                             font-size:{fs}px; font-weight:{fw}; \
                             border:1px solid {bd}; border-radius:2px;",
                            pos = l.label.css_position(nat),
                            pad = l.label_margin.css_padding(),
                            bg = theme.tokens.surface_sunken.css(),
                            fg = theme.tokens.text.css(),
                            bd = accent.css(),
                            fs = l.label_font.size,
                            fw = l.label_font.weight,
                        ),
                        oninput: move |e: FormEvent| name_sig.set(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if matches!(e.key(), Key::Enter | Key::Escape) {
                                editing_name.set(false);
                            }
                        },
                        onblur: move |_| editing_name.set(false),
                    }
                } else {
                    div {
                        style: format!(
                            "{pos} display:flex; align-items:center; \
                             justify-content:{justify}; padding:{pad}; \
                             background:{bg}; color:{fg}; font-size:{fs}px; \
                             font-weight:{fw}; letter-spacing:0.02em; \
                             white-space:nowrap; overflow:hidden;",
                            pos = l.label.css_position(nat),
                            justify = flex_justify(&l.label_margin),
                            pad = l.label_margin.css_padding(),
                            bg = label_bg.css(),
                            fg = label_fg.css(),
                            fs = l.label_font.size,
                            fw = l.label_font.weight,
                        ),
                        ondoubleclick: move |_| editing_name.set(true),
                        "{track.name}"
                    }
                }
            }
        }
    }
}

/// A small rotary knob. With a theme filmstrip ([`KnobSkin`]) the frame for
/// the current value renders via `background-position`; otherwise a plain
/// inline-styled vector knob (circle + value dot — no CSS transforms, which
/// keeps it renderer-safe). Vertical drag adjusts; double-click recentres.
#[component]
fn McpKnob(
    pos: String,
    box_w: f32,
    box_h: f32,
    value: Signal<f32>,
    #[props(default)] skin: Option<crate::theming::KnobSkin>,
    accent: Color,
    disabled: bool,
) -> Element {
    let mut is_dragging = use_signal(|| false);
    let mut drag_start = use_signal(|| 0.0f32);
    let mut drag_start_value = use_signal(|| 0.0f32);
    let sensitivity = DragSensitivity::new(180.0, 0.1);

    // A runtime WALTER layout can hand back a non-finite box at degenerate
    // sizes; the derived dot/face geometry below would then emit `left:NaNpx`
    // and poison the vello path. Coerce to a harmless 0 box.
    let box_w = if box_w.is_finite() { box_w } else { 0.0 };
    let box_h = if box_h.is_finite() { box_h } else { 0.0 };
    let raw = value();
    let v = if raw.is_finite() {
        raw.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cursor = if disabled { "not-allowed" } else { "ns-resize" };

    // Filmstrip frame for the value: the frames are pre-sliced at import, so
    // the face is a single plain image (blitz paints `background-position`
    // offsets outside the element box — the unsliced stack leaked every
    // frame as a column of ghost knobs).
    let film = skin.as_ref().and_then(|skin| {
        let frame = ((v * (skin.frames.len().saturating_sub(1)) as f32).round() as usize)
            .min(skin.frames.len().saturating_sub(1));
        skin.frames.get(frame).map(|url| {
            format!(
                "position:absolute; left:0; top:0; width:{bw:.1}px; height:{bh:.1}px; \
                 background-image:url({url}); background-size:100% 100%; \
                 background-repeat:no-repeat; pointer-events:none;",
                bw = box_w,
                bh = box_h,
            )
        })
    });
    let face = if film.is_some() {
        String::new()
    } else {
        let theme = use_theme().theme;
        format!(
            "background:{bg}; border:1px solid {border}; border-radius:{r}px;",
            bg = theme.tokens.surface_sunken.css(),
            border = theme.tokens.border.css(),
            r = box_w.min(box_h) / 2.0,
        )
    };

    // Vector fallback indicator: a dot on the knob circumference at the
    // value angle (-135°..+135°), positioned in Rust — no CSS transforms.
    let dot = (skin.is_none()).then(|| {
        let angle = (-135.0 + v * 270.0_f32).to_radians();
        let r = box_w.min(box_h) / 2.0 - 3.5;
        let cx = box_w / 2.0 + r * angle.sin();
        let cy = box_h / 2.0 - r * angle.cos();
        format!(
            "position:absolute; left:{x:.1}px; top:{y:.1}px; width:4px; height:4px; \
             border-radius:2px; background:{c}; pointer-events:none;",
            x = cx - 2.0,
            y = cy - 2.0,
            c = accent.css(),
        )
    });

    // The knob body (`*_knob_small`): centered at native size under the
    // frame overlay, REAPER's compositing.
    let body = skin.as_ref().and_then(|s| s.face.as_ref()).map(|img| {
        format!(
            "position:absolute; left:{x:.1}px; top:{y:.1}px; width:{w}px; height:{h}px; \
             background-image:url({url}); background-size:100% 100%; \
             background-repeat:no-repeat; pointer-events:none;",
            x = (box_w - img.w as f32) / 2.0,
            y = (box_h - img.h as f32) / 2.0,
            w = img.w,
            h = img.h,
            url = img.url,
        )
    });

    rsx! {
        div {
            style: format!("{pos} cursor:{cursor}; {face}"),
            if let Some(body) = body {
                div { style: body }
            }
            if let Some(film) = film {
                div { style: film }
            }
            if let Some(dot) = dot {
                div { style: dot }
            }
            div {
                style: "position:absolute; inset:0;",
                onmousedown: move |evt: MouseEvent| {
                    if disabled { return; }
                    is_dragging.set(true);
                    drag_start.set(evt.client_coordinates().y as f32);
                    drag_start_value.set(value().clamp(0.0, 1.0));
                },
                onmousemove: move |evt: MouseEvent| {
                    if !*is_dragging.read() || disabled { return; }
                    let delta_px = evt.client_coordinates().y as f32 - drag_start();
                    let modifiers = ModifierKeys::new(
                        evt.modifiers().shift(), evt.modifiers().ctrl(), evt.modifiers().alt(),
                    );
                    let delta = sensitivity.calculate_delta(delta_px, modifiers);
                    value.set((drag_start_value() + delta).clamp(0.0, 1.0));
                },
                onmouseup: move |_| { is_dragging.set(false); },
                onmouseleave: move |_| { is_dragging.set(false); },
                ondoubleclick: move |_| {
                    if disabled { return; }
                    value.set(0.5);
                },
            }
        }
    }
}

/// Fill the parent box with a skin image. Pink-banded images render as
/// three stacked bands — fixed top/bottom caps at native height, the middle
/// band absorbing the size difference (REAPER's "Pink Line Crush") — instead
/// of a uniform (squishing) stretch.
pub(crate) fn skin_fill(img: &crate::theming::SkinImage, box_w: f32, box_h: f32) -> Element {
    match &img.slices {
        Some(n) => {
            // Patch geometry computed in px against the (fixed-size) box so
            // the crush clamps cleanly — a negative CSS band (top+bottom
            // exceeding the box) feeds NaN paths to the renderer.
            let fit = |a: u32, b: u32, total: f32| -> (f32, f32) {
                let (a, b) = (a as f32, b as f32);
                if a + b <= total {
                    (a, b)
                } else if a + b > 0.0 {
                    // Box smaller than the fixed caps: scale them down
                    // proportionally (REAPER stretches the remainder).
                    (a * total / (a + b), b * total / (a + b))
                } else {
                    (0.0, 0.0)
                }
            };
            let (l, r) = fit(n.l, n.r, box_w);
            let (t, b) = fit(n.t, n.b, box_h);
            let mid_w = (box_w - l - r).max(0.0);
            let mid_h = (box_h - t - b).max(0.0);
            let cols = [(0.0, l), (l, mid_w), (box_w - r, r)];
            let rows = [(0.0, t), (t, mid_h), (box_h - b, b)];
            rsx! {
                div {
                    style: "position:absolute; inset:0; pointer-events:none; overflow:hidden;",
                    for (i, patch) in n.patches.iter().enumerate() {
                        if cols[i % 3].1 >= 0.5 && rows[i / 3].1 >= 0.5 {
                            div {
                                key: "{i}",
                                style: format!(
                                    "position:absolute; left:{x:.1}px; top:{y:.1}px; \
                                     width:{w:.1}px; height:{h:.1}px; \
                                     background-image:url({url}); background-size:100% 100%;",
                                    x = cols[i % 3].0,
                                    y = rows[i / 3].0,
                                    w = cols[i % 3].1,
                                    h = rows[i / 3].1,
                                    url = patch.url,
                                ),
                            }
                        }
                    }
                }
            }
        }
        None => rsx! {
            div {
                style: format!(
                    "position:absolute; inset:0; pointer-events:none; \
                     background-image:url({url}); background-size:100% 100%; \
                     background-repeat:no-repeat;",
                    url = img.url,
                ),
            }
        },
    }
}

/// Fill a button box with its skin image, REAPER-style: pink-marked art
/// 9-slices (fixed caps 1:1, bands stretch), but **unmarked button art draws
/// at native size, centered** — REAPER never stretches plain button images,
/// which is why uniform stretching looked vertically squished (the `_ol`
/// overlay shadow rows make the art taller than the WALTER box).
pub(crate) fn skin_button(img: &crate::theming::SkinImage, box_w: f32, box_h: f32) -> Element {
    if img.slices.is_some() {
        return skin_fill(img, box_w, box_h);
    }
    let (w, h) = (img.w as f32, img.h as f32);
    rsx! {
        div {
            style: format!(
                "position:absolute; left:{x:.1}px; top:{y:.1}px; width:{w:.1}px; \
                 height:{h:.1}px; background-image:url({url}); \
                 background-size:100% 100%; background-repeat:no-repeat; \
                 pointer-events:none;",
                x = (box_w - w) / 2.0,
                y = (box_h - h) / 2.0,
                url = img.url,
            ),
        }
    }
}

/// Map a WALTER margin justification to flex `justify-content`.
pub(crate) fn flex_justify(m: &crate::theming::Margin) -> &'static str {
    match m.text_align() {
        "left" => "flex-start",
        "right" => "flex-end",
        _ => "center",
    }
}

// ── element widgets ───────────────────────────────────────────────────────────

/// A toggle button filling its anchor box (mute/solo/recarm). With an image
/// [`ButtonSkin`] the off/on atlas states paint the button (glyph omitted —
/// REAPER images carry their own); otherwise vector toggle styles.
#[component]
fn McpToggle(
    pos: String,
    box_w: f32,
    box_h: f32,
    active: Signal<bool>,
    kind: ToggleKind,
    #[props(default)] skin: Option<crate::theming::ButtonSkin>,
    disabled: bool,
) -> Element {
    let t = use_theme().theme.toggle(kind);
    let (glyph, title) = match kind {
        ToggleKind::Solo => ("S", "Solo"),
        ToggleKind::Mute => ("M", "Mute"),
        ToggleKind::RecArm => ("\u{25cf}", "Record arm"),
    };
    let on = active();
    let cursor = if disabled { "not-allowed" } else { "pointer" };
    // Hook order: created unconditionally (hooks can't live in branches).
    let mut hovered = use_signal(|| false);
    let mut pressed = use_signal(|| false);

    if let Some(skin) = skin {
        let states = if on { &skin.on } else { &skin.off };
        let img = if pressed() {
            &states.pressed
        } else if hovered() {
            &states.hover
        } else {
            &states.normal
        };
        // div, not button: blitz's UA button background paints over
        // background-image.
        let fill = skin_button(img, box_w, box_h);
        return rsx! {
            div {
                title,
                style: format!("{pos} cursor:{cursor};"),
                onmouseenter: move |_| hovered.set(true),
                onmouseleave: move |_| { hovered.set(false); pressed.set(false); },
                onmousedown: move |_| pressed.set(true),
                onmouseup: move |_| pressed.set(false),
                onclick: move |_| {
                    if disabled { return; }
                    let next = !active();
                    active.set(next);
                },
                {fill}
            }
        };
    }

    let colors = if on {
        format!(
            "background:{fill}; color:{fg}; border:1px solid {fill}; \
             box-shadow:0 0 8px {glow}, inset 0 1px 0 rgba(255,255,255,0.3);",
            fill = t.on_fill.css(),
            fg = t.on_text.css(),
            glow = t.on_fill.with_alpha(0x99).css(),
        )
    } else {
        format!(
            "background:{bg}; color:{txt}; border:1px solid {border};",
            bg = t.off_bg.css(),
            txt = t.off_text.css(),
            border = t.off_border.css(),
        )
    };
    rsx! {
        button {
            r#type: "button",
            title,
            style: format!(
                "{pos} display:flex; align-items:center; justify-content:center; \
                 font-size:12px; font-weight:800; line-height:1; border-radius:5px; \
                 cursor:{cursor}; {colors}"
            ),
            onclick: move |_| {
                if disabled { return; }
                let next = !active();
                active.set(next);
            },
            "{glyph}"
        }
    }
}

/// An inert, themed flag button (phase/fx/fxbyp/env/folder/rec row) —
/// positionable by layouts today, wired to state when the view-model carries
/// it. With a skin image the theme's art renders; vector chip otherwise.
#[component]
fn McpFlag(
    pos: String,
    box_w: f32,
    box_h: f32,
    glyph: &'static str,
    title: &'static str,
    #[props(default)] skin: Option<crate::theming::SkinImage>,
) -> Element {
    let tk = use_theme().theme.tokens;
    if let Some(img) = skin {
        let fill = skin_button(&img, box_w, box_h);
        return rsx! {
            div {
                title,
                style: format!("{pos}"),
                {fill}
            }
        };
    }
    rsx! {
        div {
            title,
            style: format!(
                "{pos} display:flex; align-items:center; justify-content:center; \
                 font-size:9px; font-weight:800; border-radius:4px; \
                 background:{bg}; color:{fg}; border:1px solid {border};",
                bg = tk.surface_sunken.css(),
                fg = tk.text_faint.css(),
                border = tk.border.css(),
            ),
            "{glyph}"
        }
    }
}

/// A stateful, image-skinned flag (phase invert): the on/off atlas states
/// follow the bound signal, clicking toggles it. Vector chip fallback.
#[component]
fn McpStateFlag(
    pos: String,
    box_w: f32,
    box_h: f32,
    glyph: &'static str,
    title: &'static str,
    active: Signal<bool>,
    #[props(default)] skin: Option<crate::theming::ButtonSkin>,
    #[props(default)] disabled: bool,
) -> Element {
    let tk = use_theme().theme.tokens;
    let on = active();
    if let Some(skin) = skin {
        let states = if on { &skin.on } else { &skin.off };
        let fill = skin_button(&states.normal, box_w, box_h);
        return rsx! {
            div {
                title,
                style: format!("{pos} cursor:pointer;"),
                onclick: move |_| {
                    if disabled { return; }
                    let next = !active();
                    active.set(next);
                },
                {fill}
            }
        };
    }
    let (bg, fg) = if on {
        (tk.accent.css(), tk.accent.on().css())
    } else {
        (tk.surface_sunken.css(), tk.text_faint.css())
    };
    rsx! {
        div {
            title,
            style: format!(
                "{pos} display:flex; align-items:center; justify-content:center; \
                 font-size:9px; font-weight:800; border-radius:4px; \
                 background:{bg}; color:{fg}; border:1px solid {bd}; cursor:pointer;",
                bd = tk.border.css(),
            ),
            onclick: move |_| {
                if disabled { return; }
                let next = !active();
                active.set(next);
            },
            "{glyph}"
        }
    }
}

/// The volume fader, vertical or horizontal per `mcp.volume.fadermode`.
/// With an image skin, `skin_bg` paints the well (`mcp_volbg`, stretched) and
/// `skin_thumb` replaces the vector cap (`mcp_volthumb`, native aspect).
#[component]
fn McpFader(
    pos: String,
    value: Signal<f32>,
    mode: FaderMode,
    accent: Color,
    #[props(default)] skin_bg: Option<crate::theming::SkinImage>,
    #[props(default)] skin_thumb: Option<crate::theming::SkinImage>,
    disabled: bool,
) -> Element {
    let mut is_dragging = use_signal(|| false);
    let mut drag_start = use_signal(|| 0.0f32);
    let mut drag_start_value = use_signal(|| 0.0f32);

    let sensitivity = DragSensitivity::new(180.0, 0.1);
    let v = value().clamp(0.0, 1.0);
    let fill_pct = v * 100.0;
    let cursor = if disabled {
        "not-allowed"
    } else if mode == FaderMode::Horizontal {
        "ew-resize"
    } else {
        "ns-resize"
    };

    let theme = use_theme().theme;
    let f = theme.fader(&ThemeState::new());

    // Cap: image thumb at native aspect (centered on the value position), or
    // the vector cap + accent line.
    // `Knob` never reaches the fader (callers route it to [`McpKnob`]);
    // treat it as vertical defensively.
    // Cap travel is inset by the cap's own size, REAPER's model: at 100%
    // the cap's TOP edge touches the well top (`bottom = 100% − cap_h`), at
    // 0% its bottom edge sits on the well floor — the cap body never leaves
    // the well. `offset = pct/100 × cap_size` keeps that without knowing
    // the well's px height.
    let cap = match (&skin_thumb, mode) {
        (Some(thumb), FaderMode::Vertical | FaderMode::Knob) => format!(
            "position:absolute; left:calc(50% - {hw}px); width:{w}px; height:{h}px; \
             bottom:calc({fill_pct}% - {inset:.1}px); pointer-events:none; \
             background-image:url({url}); background-size:100% 100%;",
            w = thumb.w,
            h = thumb.h,
            hw = thumb.w as f32 / 2.0,
            inset = fill_pct / 100.0 * thumb.h as f32,
            url = thumb.url,
        ),
        (Some(thumb), FaderMode::Horizontal) => format!(
            "position:absolute; top:calc(50% - {hh}px); width:{w}px; height:{h}px; \
             left:calc({fill_pct}% - {inset:.1}px); pointer-events:none; \
             background-image:url({url}); background-size:100% 100%;",
            w = thumb.w,
            h = thumb.h,
            hh = thumb.h as f32 / 2.0,
            inset = fill_pct / 100.0 * thumb.w as f32,
            url = thumb.url,
        ),
        (None, FaderMode::Vertical | FaderMode::Knob) => format!(
            "position:absolute; left:-3px; right:-3px; \
             bottom:calc({fill_pct}% - {inset:.1}px); \
             height:12px; border-radius:3px; \
             background:linear-gradient(180deg,{top},{bottom}); \
             border:1px solid #15151a; pointer-events:none; \
             box-shadow:0 1px 2px rgba(0,0,0,0.6);",
            inset = fill_pct / 100.0 * 12.0,
            top = f.cap_top.css(),
            bottom = f.cap_bottom.css(),
        ),
        (None, FaderMode::Horizontal) => format!(
            "position:absolute; top:-3px; bottom:-3px; \
             left:calc({fill_pct}% - {inset:.1}px); \
             width:12px; border-radius:3px; \
             background:linear-gradient(90deg,{top},{bottom}); \
             border:1px solid #15151a; pointer-events:none; \
             box-shadow:0 1px 2px rgba(0,0,0,0.6);",
            inset = fill_pct / 100.0 * 12.0,
            top = f.cap_top.css(),
            bottom = f.cap_bottom.css(),
        ),
    };
    let skinned = skin_thumb.is_some();
    let cap_line = match mode {
        FaderMode::Vertical | FaderMode::Knob => format!(
            "position:absolute; left:3px; right:3px; top:5px; height:2px; \
             background:{}; border-radius:1px;",
            accent.css()
        ),
        FaderMode::Horizontal => format!(
            "position:absolute; top:3px; bottom:3px; left:5px; width:2px; \
             background:{}; border-radius:1px;",
            accent.css()
        ),
    };

    // Well: image bg (stretched; its pink-marked fixed regions are baked into
    // the slice) or the vector well.
    let well = match &skin_bg {
        Some(bg) => format!(
            "{pos} cursor:{cursor}; background-image:url({url}); \
             background-size:100% 100%; overflow:visible;",
            url = bg.url,
        ),
        None => format!(
            "{pos} background:{well}; border-radius:5px; border:1px solid {border}; \
             cursor:{cursor}; box-shadow:inset 0 1px 3px rgba(0,0,0,0.5);",
            well = f.well.css(),
            border = f.border.css(),
        ),
    };

    rsx! {
        div {
            style: well,
            div {
                style: cap,
                if !skinned {
                    div { style: cap_line }
                }
            }

            // Drag overlay.
            div {
                style: "position:absolute; inset:0;",
                onmousedown: move |evt: MouseEvent| {
                    if disabled { return; }
                    is_dragging.set(true);
                    let p = evt.client_coordinates();
                    drag_start.set(if mode == FaderMode::Horizontal { p.x as f32 } else { p.y as f32 });
                    drag_start_value.set(value().clamp(0.0, 1.0));
                },
                onmousemove: move |evt: MouseEvent| {
                    if !*is_dragging.read() || disabled { return; }
                    let p = evt.client_coordinates();
                    // `calculate_delta` negates internally (screen-Y convention:
                    // positive input = down = decrease), so pass raw deltas.
                    let delta_px = match mode {
                        // Up = increase.
                        FaderMode::Vertical | FaderMode::Knob => p.y as f32 - drag_start(),
                        // Right = increase.
                        FaderMode::Horizontal => drag_start() - p.x as f32,
                    };
                    let modifiers = ModifierKeys::new(
                        evt.modifiers().shift(), evt.modifiers().ctrl(), evt.modifiers().alt(),
                    );
                    let delta = sensitivity.calculate_delta(delta_px, modifiers);
                    value.set((drag_start_value() + delta).clamp(0.0, 1.0));
                },
                onmouseup: move |_| { is_dragging.set(false); },
                onmouseleave: move |_| { is_dragging.set(false); },
            }
        }
    }
}

/// The level meter. Fill: the theme's `meter_strip_v` image when skinned,
/// else the `mcp.meter.scale.color.lit.*` gradient, else token zone colours;
/// well from `meter_bg_v` / `…unlit.*` / the meter style.
#[component]
fn McpMeter(
    pos: String,
    level: Signal<f32>,
    level_right: Option<Signal<f32>>,
    peak: Signal<f32>,
    #[props(default)] skin_strip: Option<crate::theming::SkinImage>,
    #[props(default)] skin_bg: Option<crate::theming::SkinImage>,
) -> Element {
    // Reading the signals HERE scopes meter ticks to this component —
    // reading them in the strip body re-rendered the whole strip per tick.
    let level = level();
    let level_right = level_right.map(|s| s());
    let peak = peak();
    let theme = use_theme().theme;
    let m = theme.meter();
    let c = theme.mcp.colors;

    let well = match (&skin_bg, c.meter_unlit_top, c.meter_unlit_bottom) {
        (Some(bg), _, _) => format!(
            "background-image:url({}); background-size:100% 100%;",
            bg.url
        ),
        (None, Some(top), Some(bottom)) => format!(
            "background:linear-gradient(180deg,{},{});",
            top.css(),
            bottom.css()
        ),
        _ => format!("background:{};", m.well.css()),
    };

    // One meter column. Zero/non-finite renders nothing (vello NaN guard).
    let column = |lvl: f32, left: f32, w: f32| {
        let lvl = if lvl.is_finite() {
            lvl.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fill = match (&skin_strip, c.meter_lit_top, c.meter_lit_bottom) {
            (Some(strip), _, _) => format!(
                "background-image:url({}); background-size:100% 100%;",
                strip.url
            ),
            (None, Some(top), Some(bottom)) => format!(
                "background:linear-gradient(180deg,{},{});",
                top.css(),
                bottom.css()
            ),
            _ => format!("background:{};", theme.meter_zone(lvl).css()),
        };
        rsx! {
            if lvl > 0.0 {
                div {
                    style: format!(
                        "position:absolute; bottom:0; left:{left}%; width:{w}%; height:{h}%; \
                         {fill} opacity:0.9; pointer-events:none; border-radius:1px;",
                        h = lvl * 100.0,
                    ),
                }
            }
        }
    };

    let peak = if peak.is_finite() {
        peak.clamp(0.0, 1.0)
    } else {
        0.0
    };

    rsx! {
        div {
            style: format!(
                "{pos} {well} border-radius:3px; overflow:hidden; \
                 border:1px solid {border};",
                border = m.border.css(),
            ),
            if let Some(right) = level_right {
                {column(level, 6.0, 41.0)}
                {column(right, 53.0, 41.0)}
            } else {
                {column(level, 10.0, 80.0)}
            }
            if peak > 0.0 {
                div {
                    style: format!(
                        "position:absolute; left:0; right:0; bottom:{p}%; height:2px; \
                         background:{col}; pointer-events:none;",
                        p = peak * 100.0,
                        col = m.peak.css(),
                    ),
                }
            }
        }
    }
}
