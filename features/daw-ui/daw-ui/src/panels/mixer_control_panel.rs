//! MixerControlPanel (MCP) — the bottom-third mixer console.
//!
//! A horizontal, scrollable row of [`McpStrip`]s driven by [`TrackView`],
//! under a titled header bar. Strips are laid out by the theme's WALTER-style
//! MCP context (`theme.mcp` — anchor coords, per-element colours, named
//! layouts). Folder tracks are rendered as tinted group headers spanning
//! their children (Reaper-MCP style).

use input::{InputCommand, KeymapConfig};
use input_dioxus::use_input_processor;

use crate::panels::mcp_strip::McpStrip;
use crate::panels::model::TrackView;
use crate::prelude::*;
use crate::theming::use_theme;

/// Mixer wheel gestures, via the same input keymap layer as the arrange —
/// our one deviation from REAPER: the MCP zooms horizontally. Shift+wheel pans
/// left/right; Alt+Shift+wheel zooms the strip width (the mixer's "track
/// width", analogous to track height in the TCP).
fn mixer_keymap() -> KeymapConfig {
    use std::collections::HashMap;
    let normal = HashMap::from([
        ("Shift+Scroll".to_string(), "mixer.pan".to_string()),
        ("Alt+Shift+Scroll".to_string(), "mixer.zoom".to_string()),
    ]);
    KeymapConfig {
        scroll: HashMap::from([("normal".to_string(), normal)]),
        ..Default::default()
    }
}

/// The mixer console panel. Pass the full track list; strips render left→right
/// in track order with folder tracks shown as group headers.
#[component]
pub fn MixerControlPanel(tracks: Vec<TrackView>) -> Element {
    let p = use_theme().theme.panel();
    let input = use_input_processor(mixer_keymap());
    // Horizontal pan + strip-width zoom (both signals, like the arrange).
    let mut scroll_x = use_signal(|| 0.0f64);
    let mut strip_scale = use_signal(|| 1.0f32);
    let sx = strip_scale();
    rsx! {
        div {
            style: format!(
                "display:flex; flex-direction:column; height:100%; min-height:0; \
                 background:{surface}; border-top:1px solid {border};",
                surface = p.surface.css(),
                border = p.border.css(),
            ),

            // Header bar.
            div {
                style: format!(
                    "flex:0 0 auto; display:flex; align-items:center; gap:8px; \
                     padding:4px 10px; background:{header}; border-bottom:1px solid {border}; \
                     font-size:10px; font-weight:800; letter-spacing:0.08em; text-transform:uppercase; \
                     color:{label}; user-select:none;",
                    header = p.header.css(),
                    border = p.border.css(),
                    label = p.label.css(),
                ),
                span { "Mixer" }
                span { style: "opacity:0.5; font-weight:600;", "{tracks.len()} tracks" }
                span { style: "opacity:0.4; font-weight:600;", "· {(sx * 100.0) as u32}%" }
            }

            // Strip row: horizontal position driven by `scroll_x` (translation
            // under overflow:hidden); Shift+wheel pans, Alt+Shift+wheel zooms
            // the strip width.
            div {
                style: "flex:1 1 0; min-height:0; overflow:hidden; position:relative;",
                onwheel: move |evt: WheelEvent| {
                    let d = evt.data().delta().strip_units();
                    for cmd in input.handle_wheel(&evt) {
                        let action = match &cmd {
                            InputCommand::Action(a) => Some(a.as_str()),
                            InputCommand::ActionWithArgs { action, .. } => Some(action.as_str()),
                            _ => None,
                        };
                        match action {
                            Some("mixer.pan") => {
                                scroll_x.set((scroll_x() + d.y + d.x).max(0.0));
                            }
                            Some("mixer.zoom") => {
                                let f = if d.y < 0.0 { 1.1 } else { 1.0 / 1.1 };
                                strip_scale.set((strip_scale() * f).clamp(0.4, 3.0));
                            }
                            _ => {}
                        }
                    }
                    if d.x.abs() > d.y.abs() {
                        scroll_x.set((scroll_x() + d.x).max(0.0));
                    }
                },
                div {
                    style: format!(
                        "display:flex; align-items:stretch; height:100%; padding:8px; \
                         width:max-content; position:relative; left:{lx:.1}px;",
                        lx = -scroll_x(),
                    ),
                    for track in tracks.iter() {
                        MixerCell { key: "{track.id}", track: track.clone(), width_scale: sx }
                    }
                }
            }
        }
    }
}

/// One mixer column: a WALTER-laid-out [`McpStrip`]. Folder tracks render
/// as normal strips, exactly as REAPER does (the folder button + theme
/// chrome carry the hierarchy cues). `width_scale` is the horizontal zoom.
#[component]
fn MixerCell(track: TrackView, #[props(default = 1.0)] width_scale: f32) -> Element {
    rsx! {
        McpStrip { track, width_scale }
    }
}
