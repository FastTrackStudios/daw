//! The WALTER-themeable transport bar — REAPER's `trans.*` context.
//!
//! Every element is positioned by the active [`TransLayout`]'s anchor coords:
//! the button row (`trans.rew/fwd/play/stop/pause/rec/repeat`), the BPM box
//! (`trans.bpm.edit`), and the status readout (`trans.status` — play state +
//! position). Buttons render the theme's `transport_*` atlas states when the
//! skin carries them, vector glyphs otherwise; `trans.custom.*` chrome paints
//! first in custom z-order.

use crate::panels::mcp_strip::{flex_justify, skin_button, skin_fill};
use crate::prelude::*;
use crate::theming::{ButtonSkin, ButtonStateSkin, Coord, use_theme};

/// The transport bar. Wire `playing` + the callbacks to the engine transport;
/// `position` (seconds) and `bpm` feed the readouts.
#[component]
pub fn TransportBar(
    playing: Signal<bool>,
    #[props(default)] on_play: Option<EventHandler<()>>,
    #[props(default)] on_stop: Option<EventHandler<()>>,
    #[props(default)] position: Option<Signal<f64>>,
    #[props(default = 120.0)] bpm: f64,
    /// Named layout (`None` = the theme's first).
    #[props(default)]
    layout: Option<String>,
) -> Element {
    let theme = use_theme().theme;
    let tr = theme.trans.clone();
    let l = tr.layout(layout.as_deref()).clone();
    let nat = l.size;
    let skin = tr.skin.clone().unwrap_or_default();

    let is_playing = playing();
    let bar_h = l.size.1.max(l.docked_height);
    let bg = tr.bg.css();
    let fg = tr.fg.css();

    let status_fg = l.status_color.map(|c| c.fg).unwrap_or(tr.fg).css();
    let status_bg = l
        .status_color
        .and_then(|c| c.bg)
        .map(|c| format!("background:{};", c.css()))
        .unwrap_or_default();
    let bpm_fg = l.bpm_edit_color.map(|c| c.fg).unwrap_or(tr.fg).css();

    // Position readout, REAPER-style `m:ss.mmm`-ish. (Read inside this
    // component so transport ticks re-render the bar, not the whole app.)
    let position = position.map(|p| p()).unwrap_or(0.0);
    let m = (position / 60.0).floor() as i64;
    let s = position - m as f64 * 60.0;
    let status_text = format!("{}  {m}:{s:05.2}", if is_playing { "▶" } else { "■" });

    rsx! {
        div {
            style: format!(
                "position:relative; flex:0 0 {bar_h}px; height:{bar_h}px; width:100%; \
                 overflow:hidden; background:{bg}; color:{fg}; user-select:none;"
            ),

            // trans.custom.* chrome (background fills + declared images).
            for c in l.customs.iter() {
                if c.image.is_some() || c.bg.is_some() {
                    div {
                        key: "{c.name}",
                        style: format!(
                            "{pos}{fill} pointer-events:none;",
                            pos = c.coord.css_position(nat),
                            fill = c
                                .bg
                                .map(|b| format!(" background:{};", b.css()))
                                .unwrap_or_default(),
                        ),
                        if let Some(img) = &c.image {
                            {skin_fill(img, c.coord.w, c.coord.h)}
                        }
                    }
                }
            }

            // Button row.
            TransButton { coord: l.rew, nat, glyph: "⏮", title: "Go to start", skin: skin.rew.clone(),
                onclick: move |_| {} }
            TransButton { coord: l.fwd, nat, glyph: "⏭", title: "Go to end", skin: skin.fwd.clone(),
                onclick: move |_| {} }
            TransButton {
                coord: l.play, nat, glyph: "▶", title: "Play",
                skin: state_skin(&skin.play, is_playing),
                active: is_playing,
                onclick: move |_| { if let Some(h) = &on_play { h.call(()); } },
            }
            TransButton {
                coord: l.stop, nat, glyph: "■", title: "Stop", skin: skin.stop.clone(),
                onclick: move |_| { if let Some(h) = &on_stop { h.call(()); } },
            }
            TransButton { coord: l.pause, nat, glyph: "⏸", title: "Pause",
                skin: state_skin(&skin.pause, false), onclick: move |_| {} }
            TransButton { coord: l.rec, nat, glyph: "●", title: "Record",
                skin: state_skin(&skin.rec, false), onclick: move |_| {} }
            TransButton { coord: l.repeat, nat, glyph: "🔁", title: "Repeat",
                skin: state_skin(&skin.repeat, false), onclick: move |_| {} }

            // trans.status — play state + position readout.
            if !l.status.is_hidden() {
                div {
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:{justify}; \
                         padding:{pad}; {status_bg} color:{status_fg}; \
                         font-size:{fs}px; font-weight:{fw}; \
                         font-variant-numeric:tabular-nums; white-space:nowrap;",
                        pos = l.status.css_position(nat),
                        justify = flex_justify(&l.status_margin),
                        pad = l.status_margin.css_padding(),
                        fs = l.status_font.size,
                        fw = l.status_font.weight,
                    ),
                    "{status_text}"
                }
            }

            // trans.bpm.edit — tempo readout.
            if !l.bpm_edit.is_hidden() {
                div {
                    title: "Tempo",
                    style: format!(
                        "{pos} display:flex; align-items:center; justify-content:center; \
                         color:{bpm_fg}; font-size:{fs}px; font-weight:{fw}; \
                         font-variant-numeric:tabular-nums;{bg}",
                        pos = l.bpm_edit.css_position(nat),
                        fs = l.bpm_edit_font.size,
                        fw = l.bpm_edit_font.weight,
                        bg = skin
                            .bpm_bg
                            .as_ref()
                            .map(|i| format!(
                                " background-image:url({}); background-size:100% 100%;",
                                i.url
                            ))
                            .unwrap_or_default(),
                    ),
                    "{bpm:.1}"
                }
            }
        }
    }
}

/// Pick the on/off state strip out of a two-state transport skin.
fn state_skin(skin: &Option<ButtonSkin>, on: bool) -> Option<ButtonStateSkin> {
    skin.as_ref()
        .map(|s| if on { s.on.clone() } else { s.off.clone() })
}

/// One transport button: image states when skinned, vector glyph otherwise.
#[component]
fn TransButton(
    coord: Coord,
    nat: (f32, f32),
    glyph: &'static str,
    title: &'static str,
    #[props(default)] skin: Option<ButtonStateSkin>,
    #[props(default)] active: bool,
    onclick: EventHandler<()>,
) -> Element {
    let mut hovered = use_signal(|| false);
    let mut pressed = use_signal(|| false);
    let theme = use_theme().theme;

    if coord.is_hidden() {
        return rsx! {};
    }
    let pos = coord.css_position(nat);

    if let Some(s) = skin {
        let img = if pressed() {
            &s.pressed
        } else if hovered() {
            &s.hover
        } else {
            &s.normal
        };
        let fill = skin_button(img, coord.w, coord.h);
        return rsx! {
            div {
                title,
                style: format!("{pos} cursor:pointer;"),
                onmouseenter: move |_| hovered.set(true),
                onmouseleave: move |_| { hovered.set(false); pressed.set(false); },
                onmousedown: move |_| pressed.set(true),
                onmouseup: move |_| pressed.set(false),
                onclick: move |_| onclick.call(()),
                {fill}
            }
        };
    }

    let tk = theme.tokens;
    let (bg, fg) = if active {
        (tk.accent.css(), tk.accent.on().css())
    } else {
        (tk.surface_sunken.css(), tk.text_dim.css())
    };
    rsx! {
        div {
            title,
            style: format!(
                "{pos} display:flex; align-items:center; justify-content:center; \
                 border-radius:4px; background:{bg}; color:{fg}; cursor:pointer; \
                 border:1px solid {bd}; font-size:12px;",
                bd = tk.border.css(),
            ),
            onclick: move |_| onclick.call(()),
            "{glyph}"
        }
    }
}
