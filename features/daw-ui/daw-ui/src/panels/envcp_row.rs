//! The WALTER-themeable envelope-control-panel row (`envcp.*`).
//!
//! One row per visible envelope, rendered under its parent TCP row at the
//! envelope lane's height: `envcp.custom.*` chrome, the label, the value
//! fader (`envcp.fader`, knob when `.fadermode` forces one), the value
//! readout, and the arm/bypass/hide button cluster — skinned by the theme's
//! `envcp_*` atlases when present.

use crate::panels::mcp_strip::{flex_justify, skin_button, skin_fill};
use crate::panels::model::EnvelopeView;
use crate::prelude::*;
use crate::theming::{ButtonStateSkin, Color, Coord, use_theme};

/// One ECP row. `accent` is the envelope's curve colour (label tint).
#[component]
pub fn EnvcpRow(
    envelope: EnvelopeView,
    /// Named layout (`None` = the theme's first).
    #[props(default)]
    layout: Option<String>,
) -> Element {
    let theme = use_theme().theme;
    let ecp = theme.envcp.clone();
    let l = ecp.layout(layout.as_deref()).clone();
    let nat = l.size;
    let skin = ecp.skin.clone().unwrap_or_default();
    let ar = theme.arrange;

    let accent = envelope
        .color
        .as_deref()
        .and_then(Color::hex)
        .unwrap_or(ar.env_default);

    let label_fg = l.label_color.map(|c| c.fg).unwrap_or(theme.tokens.text);
    let value_fg = l.value_color.map(|c| c.fg).unwrap_or(theme.tokens.text_dim);

    // Last point's value as the readout (static until envelopes are live).
    let value = envelope.points.last().map(|p| p.1).unwrap_or(0.75);
    let value_pct = format!("{:.0}%", value * 100.0);

    rsx! {
        div {
            style: format!(
                "position:relative; width:100%; height:100%; overflow:hidden; \
                 user-select:none; background:{bg}; border-bottom:1px solid {bd};",
                bg = theme.tokens.surface.css(),
                bd = theme.tokens.border.css(),
            ),

            // envcp.custom.* chrome (background fills + declared images).
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

            // envcp.label — envelope name, tinted with the curve colour.
            if !l.label.is_hidden() {
                div {
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:{justify}; \
                         padding:{pad}; color:{fg}; font-size:{fs}px; font-weight:{fw}; \
                         white-space:nowrap; overflow:hidden;",
                        pos = l.label.css_position(nat),
                        justify = flex_justify(&l.label_margin),
                        pad = l.label_margin.css_padding(),
                        fg = label_fg.css(),
                        fs = l.label_font.size,
                        fw = l.label_font.weight,
                    ),
                    span {
                        style: format!(
                            "display:inline-block; width:8px; height:8px; border-radius:2px; \
                             background:{c}; margin-right:5px;",
                            c = accent.css(),
                        ),
                    }
                    "{envelope.name}"
                }
            }

            // envcp.fader — the envelope value fader (inert readout for now).
            if !l.fader.is_hidden() {
                div {
                    style: format!(
                        "{pos}{bg}",
                        pos = l.fader.css_position(nat),
                        bg = skin
                            .fader_bg
                            .as_ref()
                            .map(|i| format!(
                                " background-image:url({}); background-size:100% 100%;",
                                i.url
                            ))
                            .unwrap_or_else(|| format!(
                                " background:{}; border-radius:3px;",
                                theme.tokens.surface_sunken.css()
                            )),
                    ),
                    // Value cap.
                    if let Some(thumb) = skin.fader_thumb.as_ref() {
                        div {
                            style: format!(
                                "position:absolute; top:calc(50% - {hh}px); \
                                 left:calc({pct}% - {hw}px); width:{w}px; height:{h}px; \
                                 background-image:url({url}); background-size:100% 100%; \
                                 pointer-events:none;",
                                pct = value * 100.0,
                                w = thumb.w,
                                h = thumb.h,
                                hw = thumb.w / 2,
                                hh = thumb.h / 2,
                                url = thumb.url,
                            ),
                        }
                    } else {
                        div {
                            style: format!(
                                "position:absolute; top:-2px; bottom:-2px; \
                                 left:calc({pct}% - 3px); width:6px; border-radius:2px; \
                                 background:{c}; pointer-events:none;",
                                pct = value * 100.0,
                                c = accent.css(),
                            ),
                        }
                    }
                }
            }

            // envcp.value — the value readout.
            if !l.value.is_hidden() {
                div {
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:{justify}; \
                         color:{fg}; font-size:{fs}px; font-weight:{fw}; \
                         font-variant-numeric:tabular-nums; pointer-events:none;",
                        pos = l.value.css_position(nat),
                        justify = flex_justify(&l.value_margin),
                        fg = value_fg.css(),
                        fs = l.value_font.size,
                        fw = l.value_font.weight,
                    ),
                    "{value_pct}"
                }
            }

            // Button cluster (inert until envelopes are live).
            EnvcpFlag { coord: l.arm, nat, glyph: "●", title: "Arm envelope", skin: skin.arm.as_ref().map(|b| b.off.normal.clone()) }
            EnvcpFlag { coord: l.bypass, nat, glyph: "byp", title: "Bypass envelope", skin: skin.bypass.as_ref().map(|b| b.off.normal.clone()) }
            EnvcpFlag { coord: l.hide, nat, glyph: "✕", title: "Hide envelope", skin: skin.hide.as_ref().map(|b: &ButtonStateSkin| b.normal.clone()) }
            EnvcpFlag { coord: l.learn, nat, glyph: "L", title: "Learn", skin: skin.learn.as_ref().map(|b| b.off.normal.clone()) }
            EnvcpFlag { coord: l.modulate, nat, glyph: "mod", title: "Parameter modulation", skin: skin.parammod.as_ref().map(|b| b.off.normal.clone()) }
        }
    }
}

/// One inert, themed ECP chip (arm/bypass/hide/learn/mod).
#[component]
fn EnvcpFlag(
    coord: Coord,
    nat: (f32, f32),
    glyph: &'static str,
    title: &'static str,
    #[props(default)] skin: Option<crate::theming::SkinImage>,
) -> Element {
    if coord.is_hidden() {
        return rsx! {};
    }
    let tk = use_theme().theme.tokens;
    let pos = coord.css_position(nat);
    if let Some(img) = skin {
        let fill = skin_button(&img, coord.w, coord.h);
        return rsx! {
            div { title, style: pos, {fill} }
        };
    }
    rsx! {
        div {
            title,
            style: format!(
                "{pos} display:flex; align-items:center; justify-content:center; \
                 font-size:8px; font-weight:700; border-radius:3px; \
                 background:{bg}; color:{fg}; border:1px solid {bd};",
                bg = tk.surface_sunken.css(),
                fg = tk.text_faint.css(),
                bd = tk.border.css(),
            ),
            "{glyph}"
        }
    }
}
