//! TrackControlPanel (TCP) — the left sidebar of the arrange view.
//!
//! A vertical stack of per-track control rows driven by the theme's **TCP
//! context** (`theme.tcp` — REAPER's `tcp.*` WALTER vocabulary), rendered by
//! the same strip machinery as the mixer ([`McpStrip`] with `tcp: true`).
//! Each row is `track.height` px tall so the rows line up with the arrange
//! lanes on the right; folder depth indents the row, REAPER-style.

use crate::panels::envcp_row::EnvcpRow;
use crate::panels::mcp_strip::McpStrip;
use crate::panels::model::TrackView;
use crate::prelude::*;
use crate::theming::use_theme;

/// Per-track indent (px) applied per folder-depth level.
const INDENT: u32 = 12;

/// The track-control sidebar. `width` is the sidebar width in px; rows scroll
/// vertically. Intended to sit left of [`super::ArrangeView`]'s timeline.
#[component]
pub fn TrackControlPanel(
    tracks: Vec<TrackView>,
    #[props(default = 380)] width: u32,
    /// Scroll its own rows vertically. Set `false` when embedded in
    /// [`super::ArrangeView`], where the arrange body owns the shared scroll
    /// so the rows stay aligned with the timeline lanes.
    #[props(default = true)]
    scroll: bool,
    /// Vertical-zoom multiplier on row heights (matches the arrange lanes so
    /// rows stay aligned). `1.0` = base height.
    #[props(default = 1.0)]
    height_scale: f32,
) -> Element {
    let overflow = if scroll { "auto" } else { "visible" };
    let surface = use_theme().theme.panel().surface.css();
    // Exclusive selection (REAPER: clicking a panel selects its track).
    let selections: Vec<(usize, Signal<bool>)> =
        tracks.iter().map(|t| (t.id, t.selected)).collect();
    rsx! {
        div {
            style: format!(
                "flex:0 0 {width}px; width:{width}px; display:flex; flex-direction:column; \
                 background:{surface}; overflow-y:{overflow}; user-select:none;"
            ),
            for track in tracks.iter() {
                TcpRow {
                    key: "{track.id}",
                    track: track.clone(),
                    panel_w: width,
                    height_scale,
                    on_select: {
                        let selections = selections.clone();
                        move |id: usize| {
                            for (tid, mut sel) in selections.iter().copied() {
                                sel.set(tid == id);
                            }
                        }
                    },
                }
            }
        }
    }
}

/// One TCP control row for a track: a fixed-height row box (so rows align
/// with the arrange lanes) filled by the theme's TCP context, with REAPER's
/// folder indent on the left.
#[component]
fn TcpRow(
    track: TrackView,
    panel_w: u32,
    #[props(default = 1.0)] height_scale: f32,
    on_select: EventHandler<usize>,
) -> Element {
    let indent = track.depth * INDENT;
    // Displayed row height = base-height Signal × vertical zoom (aligns with
    // the arrange lanes; drag-to-resize sets the base Signal).
    let h = (((track.height)() as f32 * height_scale).round() as u32).max(8);
    let total = ((track.total_height() as f32 * height_scale).round() as u32).max(8);
    let id = track.id;
    // The strip's actual px box — lets an imported REAPER theme re-run
    // WALTER at this exact size (flow themes rewrap/cull per width).
    let size = (panel_w.saturating_sub(indent) as f32, h as f32);
    let envelopes: Vec<_> = track
        .envelopes
        .iter()
        .filter(|e| e.visible)
        .cloned()
        .collect();
    rsx! {
        div {
            style: format!(
                "flex:0 0 {total}px; height:{total}px; display:flex; align-items:stretch;"
            ),
            onclick: move |_| on_select.call(id),
            if indent > 0 {
                div { style: format!("flex:0 0 {indent}px;") }
            }
            div {
                style: "flex:1 1 0; min-width:0; display:flex; flex-direction:column;",
                div {
                    style: format!("flex:0 0 {h}px; height:{h}px; position:relative;"),
                    McpStrip { track: track.clone(), tcp: true, size }
                }
                // ECP rows under the track, one per visible envelope.
                for (i, env) in envelopes.into_iter().enumerate() {
                    {
                        let eh = ((env.height as f32 * height_scale).round() as u32).max(6);
                        rsx! {
                            div {
                                key: "e{i}",
                                style: format!("flex:0 0 {eh}px; height:{eh}px; position:relative;"),
                                EnvcpRow { envelope: env }
                            }
                        }
                    }
                }
            }
        }
    }
}
