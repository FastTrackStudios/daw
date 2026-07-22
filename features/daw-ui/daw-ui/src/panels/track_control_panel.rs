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
    /// When set, each row gets a bottom-edge height-resize handle that begins a
    /// resize on this shared state (the arrange body applies + releases it).
    #[props(default)]
    resize: Option<Signal<Option<super::arrange_view::ResizeState>>>,
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
                    resize,
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
    #[props(default)] resize: Option<Signal<Option<super::arrange_view::ResizeState>>>,
    on_select: EventHandler<usize>,
) -> Element {
    let indent = track.depth * INDENT;
    // Displayed row height = base-height Signal × vertical zoom (aligns with
    // the arrange lanes; drag-to-resize sets the base Signal).
    let h = (((track.height)() as f32 * height_scale).round() as u32).max(2);
    let total = ((track.total_height() as f32 * height_scale).round() as u32).max(2);
    let id = track.id;
    let height_sig = track.height;
    // Below `STRIP_REF_H` the row is too short for full-size chrome, so we
    // render the strip at the reference height and CSS-scale it down — the
    // name font, mute/solo/pan buttons etc. shrink together (REAPER's
    // supercollapsed/collapsed feel). At/above it, `s == 1.0` (no scaling).
    let box_w = panel_w.saturating_sub(indent) as f32;
    const STRIP_REF_H: f32 = 46.0;
    let s = (h as f32 / STRIP_REF_H).clamp(0.12, 1.0);
    let render_h = (h as f32 / s).round();
    let render_w = (box_w / s).round();
    // The strip's pre-scale px box — an imported REAPER theme re-runs WALTER at
    // this size (flow themes rewrap/cull per width); the vector theme anchors.
    let size = (render_w, render_h);
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
                    style: format!("flex:0 0 {h}px; height:{h}px; position:relative; overflow:hidden;"),
                    // Strip rendered at the reference size, scaled to fit the row
                    // so all chrome shrinks proportionally at small heights.
                    div {
                        style: format!(
                            "position:absolute; top:0; left:0; width:{render_w}px; \
                             height:{render_h}px; transform:scale({s}); transform-origin:top left;"
                        ),
                        McpStrip { track: track.clone(), tcp: true, size }
                    }
                    // Bottom-edge height-resize handle (REAPER grabs here). Sets
                    // the shared resize state; the arrange body applies + releases.
                    if let Some(resize) = resize {
                        div {
                            class: "fts-rz",
                            style: "position:absolute; left:0; right:0; bottom:0; height:7px; \
                                    cursor:ns-resize; z-index:5;",
                            onmousedown: move |evt: MouseEvent| {
                                if evt.trigger_button()
                                    == Some(dioxus_elements::input_data::MouseButton::Primary)
                                {
                                    evt.stop_propagation();
                                    let mut resize = resize;
                                    resize.set(Some(super::arrange_view::ResizeState::new(
                                        height_sig,
                                        evt.client_coordinates().y,
                                        (height_sig)(),
                                        height_scale.max(0.05),
                                    )));
                                }
                            },
                        }
                    }
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
