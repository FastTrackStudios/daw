//! DawWorkspace — the Reaper-style composition of the three panels.
//!
//! [`ArrangeView`] (with its [`TrackControlPanel`] sidebar) fills the top, and
//! [`MixerControlPanel`] occupies the bottom third — the canonical Reaper
//! arrange-over-mixer layout. All three panels share the same `Vec<TrackView>`,
//! so faders/mutes/etc. stay in sync across them via the per-track `Signal`s.
//!
//! [`TrackControlPanel`]: super::TrackControlPanel

use crate::panels::arrange_view::{ArrangeEdit, ArrangeView};
use crate::panels::mixer_control_panel::MixerControlPanel;
use crate::panels::model::{MarkerView, RegionView, TempoMarkerView, TrackView};
use crate::prelude::*;

/// Full DAW workspace. `mixer_fraction` is the mixer's share of the height
/// (0.0–1.0; default ⅓).
#[component]
pub fn DawWorkspace(
    tracks: Vec<TrackView>,
    #[props(default = 0.34)] mixer_fraction: f32,
    #[props(default)] markers: Vec<MarkerView>,
    #[props(default)] regions: Vec<RegionView>,
    #[props(default)] tempo_markers: Vec<TempoMarkerView>,
    #[props(default)] time_sel: Option<(f64, f64)>,
    #[props(default)] loop_range: Option<(f64, f64)>,
    #[props(default)] bpm: Option<f64>,
    #[props(default)] playhead: Option<Signal<f64>>,
    #[props(default)] cursor: Option<f64>,
    #[props(default = 12.0)] pps: f64,
    #[props(default = 120.0)] seconds: f64,
    #[props(default = 4)] beats_per_measure: u32,
    /// Arrange-view edit gestures (seek / select / move / resize). `None` =
    /// read-only.
    #[props(default)]
    on_edit: Option<EventHandler<ArrangeEdit>>,
) -> Element {
    let arrange_grow = ((1.0 - mixer_fraction) * 100.0).round() as u32;
    let mixer_grow = (mixer_fraction * 100.0).round() as u32;
    let bg = crate::theming::use_theme()
        .theme
        .tokens
        .surface_sunken
        .css();
    rsx! {
        div {
            style: format!(
                "display:flex; flex-direction:column; width:100%; height:100%; \
                 min-height:0; background:{bg};"
            ),
            // Arrange (top).
            div {
                style: format!("flex:{arrange_grow} 1 0; min-height:0;"),
                ArrangeView {
                    tracks: tracks.clone(),
                    markers,
                    regions,
                    tempo_markers,
                    time_sel,
                    loop_range,
                    bpm,
                    playhead,
                    cursor,
                    pps,
                    seconds,
                    beats_per_measure,
                    on_edit,
                }
            }
            // Mixer (bottom third).
            div {
                style: format!("flex:{mixer_grow} 1 0; min-height:0;"),
                MixerControlPanel { tracks: tracks.clone() }
            }
        }
    }
}
