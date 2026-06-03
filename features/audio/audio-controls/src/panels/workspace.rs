//! DawWorkspace — the Reaper-style composition of the three panels.
//!
//! [`ArrangeView`] (with its [`TrackControlPanel`] sidebar) fills the top, and
//! [`MixerControlPanel`] occupies the bottom third — the canonical Reaper
//! arrange-over-mixer layout. All three panels share the same `Vec<TrackView>`,
//! so faders/mutes/etc. stay in sync across them via the per-track `Signal`s.
//!
//! [`TrackControlPanel`]: super::TrackControlPanel

use crate::panels::arrange_view::ArrangeView;
use crate::panels::mixer_control_panel::MixerControlPanel;
use crate::panels::model::TrackView;
use crate::prelude::*;

/// Full DAW workspace. `mixer_fraction` is the mixer's share of the height
/// (0.0–1.0; default ⅓).
#[component]
pub fn DawWorkspace(
    tracks: Vec<TrackView>,
    #[props(default = 0.34)] mixer_fraction: f32,
) -> Element {
    let arrange_grow = ((1.0 - mixer_fraction) * 100.0).round() as u32;
    let mixer_grow = (mixer_fraction * 100.0).round() as u32;
    rsx! {
        div {
            style: "display:flex; flex-direction:column; width:100%; height:100%; \
                    min-height:0; background:#0a0a0c;",
            // Arrange (top).
            div {
                style: format!("flex:{arrange_grow} 1 0; min-height:0;"),
                ArrangeView { tracks: tracks.clone() }
            }
            // Mixer (bottom third).
            div {
                style: format!("flex:{mixer_grow} 1 0; min-height:0;"),
                MixerControlPanel { tracks: tracks.clone() }
            }
        }
    }
}
