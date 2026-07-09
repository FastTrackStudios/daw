//! MixerControlPanel (MCP) — the bottom-third mixer console.
//!
//! A horizontal, scrollable row of [`McpStrip`]s driven by [`TrackView`],
//! under a titled header bar. Strips are laid out by the theme's WALTER-style
//! MCP context (`theme.mcp` — anchor coords, per-element colours, named
//! layouts). Folder tracks are rendered as tinted group headers spanning
//! their children (Reaper-MCP style).

use crate::panels::mcp_strip::McpStrip;
use crate::panels::model::TrackView;
use crate::prelude::*;
use crate::theming::use_theme;

/// The mixer console panel. Pass the full track list; strips render left→right
/// in track order with folder tracks shown as group headers.
#[component]
pub fn MixerControlPanel(tracks: Vec<TrackView>) -> Element {
    let p = use_theme().theme.panel();
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
            }

            // Scrollable strip row.
            div {
                style: "flex:1 1 0; min-height:0; display:flex; align-items:stretch; \
                        gap:0; padding:8px; overflow-x:auto; overflow-y:hidden;",
                for track in tracks.iter() {
                    MixerCell { key: "{track.id}", track: track.clone() }
                }
            }
        }
    }
}

/// One mixer column: a WALTER-laid-out [`McpStrip`]. Folder tracks render
/// as normal strips, exactly as REAPER does (the folder button + theme
/// chrome carry the hierarchy cues).
#[component]
fn MixerCell(track: TrackView) -> Element {
    rsx! {
        McpStrip { track }
    }
}
