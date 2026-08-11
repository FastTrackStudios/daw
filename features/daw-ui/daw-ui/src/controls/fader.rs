//! The volume fader, at any height, following the finger.
//!
//! Two problems that look unrelated and are both about the same thing —
//! the control being bigger than its art, and the control being ahead of
//! its engine.
//!
//! **Height.** The rail's art is 23x55 with the groove occupying rows
//! 16..39: fixed caps, a stretchy run between. Drawn as one `<svg>` scaled
//! to 300px the caps stretch too and the groove grows a solid bar at each
//! end. So it is drawn as [a stack of panes][daw_theme_art::slice::NamedArt::stack]
//! — fixed bands sized in pixels, the middle band `flex: 1` — and the
//! layout engine does the stretching. Nothing measures anything.
//!
//! The bare 14-ish pixels at each end are **correct**, and are what REAPER
//! renders: the cap art has no groove in it. If they look wrong in the app
//! the drawing is wrong, and the fix belongs in the source art's fixed
//! band, not in stretching the ends.
//!
//! **Latency.** The value under the pointer is the UI's, not the engine's —
//! see [`crate::controls::Drafts`] for why, and for the echo suppression
//! that stops the backend's own confirmation from fighting the drag.

use daw_theme_art::slice::{self, Pane};
use daw_theme_art::vector_controls as art;

use crate::controls::use_track_store;
use crate::core::sensitivity::DragSensitivity;
use crate::prelude::*;

/// A track's volume fader, wired to the DAW.
///
/// Fills the height it is given: put it in a flex column and let the strip
/// decide how tall the mixer is.
#[component]
pub fn VolumeFader(
    /// The track's GUID.
    track: String,
    /// Pixels per art pixel — sets the fader's *width* and the cap's size.
    /// The height comes from the parent box.
    #[props(default = 1.0)]
    scale: f32,
) -> Element {
    let store = use_track_store();
    let mut drafts = store.drafts();

    let mut dragging = use_signal(|| false);
    let mut grab_y = use_signal(|| 0.0f32);
    let mut grab_value = use_signal(|| 0.0f64);

    // A drag is relative, not absolute: grabbing the cap must not jump the
    // value to wherever the pointer happened to land. `normal_pixels` is
    // how far the mouse travels for the full range, and shift is the fine
    // pass the rest of the crate's controls already answer to.
    let feel = DragSensitivity::DEFAULT;

    let rail = slice::expect_art("mcp_volbg");
    let cap = slice::expect_art("mcp_volthumb");
    let (rail_w, _) = rail.source;
    let (cap_w, cap_h) = cap.source;
    let w = (rail_w * scale).round() as u32;
    let cap_px = ((cap_w * scale).round() as u32, (cap_h * scale).round() as u32);

    let guid = track.clone();
    let value = use_memo(use_reactive!(|guid| store.volume(&guid)));
    // The cap rides the rail with 0 at the bottom, and it is positioned by
    // its own top edge — so a cap at full is flush with the top and a cap
    // at zero is flush with the bottom, rather than half of it hanging off
    // each end.
    // Four places: enough that a pixel of travel is never lost on a tall
    // fader, few enough that the style string does not churn on float noise
    // every render.
    let travel = format!("calc((100% - {}px) * {:.4})", cap_px.1, 1.0 - value().clamp(0.0, 1.0));

    let guid = track.clone();
    let press = move |e: MouseEvent| {
        dragging.set(true);
        grab_y.set(e.client_coordinates().y as f32);
        grab_value.set(store.volume(&guid));
    };
    let guid = track.clone();
    let drag = move |e: MouseEvent| {
        if !dragging() {
            return;
        }
        let dy = e.client_coordinates().y as f32 - grab_y();
        let mut span = feel.normal_pixels;
        if e.modifiers().shift() {
            span /= feel.fine_multiplier;
        }
        // Up is louder: screen y grows downward and the fader does not.
        let moved = -(dy / span) as f64;
        drafts.set_volume(&guid, (grab_value() + moved).clamp(0.0, 1.0));
    };
    let guid = track.clone();
    let release = move |_| {
        if dragging.replace(false) {
            drafts.release_volume(&guid);
        }
    };

    rsx! {
        div {
            // `position: relative` so the cap can ride the rail, and an
            // explicit width because the art has one — only the height is
            // the parent's to decide.
            style: "position:relative; width:{w}px; height:100%; min-height:0; \
                    display:flex; flex-direction:column; user-select:none; cursor:ns-resize;",
            onmousedown: press,
            onmousemove: drag,
            onmouseup: release.clone(),
            // Leaving mid-drag ends it. Tracking the pointer outside the
            // element needs a window-level listener, which is a bigger
            // change than this control should carry alone.
            onmouseleave: release,

            // The rail, decomposed. One `<svg>` per band: the caps state
            // their height in pixels, the run takes what is left.
            for (i, pane) in rail.stack().into_iter().enumerate() {
                RailBand { key: "{i}", pane, width: w }
            }

            // The cap, riding on top.
            div {
                style: "position:absolute; left:50%; top:{travel}; \
                        transform:translateX(-50%); line-height:0; pointer-events:none;",
                art::VolumeFaderCap { width: Some(cap_px.0), height: Some(cap_px.1) }
            }
        }
    }
}

/// One band of the rail.
///
/// A component rather than an inline call so each band is its own scope and
/// Dioxus diffs them independently — and so the `key` above means something.
#[component]
fn RailBand(pane: Pane, width: u32) -> Element {
    rsx! {
        art::VolumeFaderTrack { pane: Some(pane), width: Some(width) }
    }
}
