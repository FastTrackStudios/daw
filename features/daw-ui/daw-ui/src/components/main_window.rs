//! The main window — REAPER's shape, composed from the native panels.
//!
//! Transport bar across the top; the track panel down the left with the
//! arrangement beside it; the mixer docked underneath. Every piece already
//! exists — [`NativeTransportBar`], [`TrackRow`], [`ArrangePreview`],
//! [`ChannelStrip`] — so this file is composition and the alignment
//! contract, not new art.
//!
//! # The alignment contract
//!
//! The TCP and the arrange view are two renderings of one row list, so
//! they share one pitch: [`geometry::tcp::ROW_H`] plus a 1px divider. The
//! TCP column carries a spacer the height of the arrange ruler, because in
//! REAPER the first track begins below the ruler line, not beside it.

use crate::components::arrangement_view::ArrangePreview;
use crate::components::mixer::ChannelStripPreview;
use crate::components::tcp::TrackRow;
use crate::panels::native::NativeTransportBar;
use crate::prelude::*;
use daw_proto::{Item, Track};
use daw_theme_art::geometry::tcp::{ROW_H, ROW_W};

/// The arrange ruler's height — stated here as well as in the arrange
/// view because the TCP's spacer must be exactly it.
const RULER_H: f32 = 26.0;
/// The transport bar's band. The bar's art is 26 tall; the band gives it
/// REAPER's breathing room.
const TRANSPORT_H: f32 = 40.0;
/// The docked mixer. Tall enough for the strip to keep its fader
/// (`Collapse` swaps to a knob below ~130 of stretch), short enough to
/// leave the arrangement most of a 768-row window.
const MIXER_H: f32 = 230.0;

/// The whole window, for a caller with the data in hand.
///
/// Pure like every `*Preview`: tracks and items in, a window out. The
/// screenshot test paints it; a live wrapper wires the same shape to the
/// DAW.
#[component]
pub fn MainWindowPreview(
    tracks: Vec<Track>,
    #[props(default)] items: Vec<Item>,
    #[props(default = 1024.0)] width: f32,
    #[props(default = 768.0)] height: f32,
    #[props(default = 120.0)] bpm: f64,
) -> Element {
    let t = daw_theme::Theme::default();
    let ground = t.chrome.surface.css();
    let bar_bg = t.chrome.surface_sunken.shade(-0.05).css();
    let rule = t.chrome.surface_sunken.shade(-0.25).css();

    let playing = use_signal(|| false);

    let middle_h = height - TRANSPORT_H - MIXER_H;
    let arrange_w = width - ROW_W;

    rsx! {
        div {
            style: "position:relative; width:{width}px; height:{height}px; \
                    background:{ground}; overflow:hidden; \
                    display:flex; flex-direction:column;",

            // ── Transport ──
            div {
                style: "height:{TRANSPORT_H}px; flex:0 0 auto; display:flex; \
                        align-items:center; padding-left:8px; \
                        background:{bar_bg}; border-bottom:1px solid {rule};",
                NativeTransportBar { playing, bpm }
            }

            // ── TCP | arrangement ──
            div {
                style: "height:{middle_h}px; flex:0 0 auto; display:flex; \
                        overflow:hidden;",
                div {
                    style: "width:{ROW_W}px; flex:0 0 auto; overflow:hidden;",
                    // The ruler's height in empty panel, so row one starts
                    // where lane one does.
                    div { style: "height:{RULER_H + 1.0}px;" }
                    for (i, track) in tracks.iter().enumerate() {
                        TrackRow {
                            key: "{track.guid}",
                            track: track.clone(),
                            index: i as u32,
                        }
                    }
                }
                ArrangePreview {
                    tracks: tracks.clone(),
                    items,
                    width: arrange_w,
                    height: middle_h,
                    bpm,
                }
            }

            // ── The docked mixer ──
            div {
                style: "height:{MIXER_H}px; flex:0 0 auto; display:flex; \
                        border-top:1px solid {rule}; overflow:hidden; \
                        background:{bar_bg};",
                for (i, track) in tracks.iter().enumerate() {
                    ChannelStripPreview {
                        key: "{track.guid}",
                        track: track.clone(),
                        index: i as u32,
                        height: MIXER_H,
                    }
                }
            }
        }
    }
}
