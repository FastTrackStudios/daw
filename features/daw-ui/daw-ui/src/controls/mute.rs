//! The mute button, live.
//!
//! The tracer bullet for the whole vector-theme effort: one control that a
//! mixing engineer can hover, click and see toggle in the browser, in the
//! desktop app and inside a REAPER panel — where the *same* component is
//! what the exporter rasterises into REAPER's three-cell mute sprite.

use daw_theme_art::dress::{self, Panel};
use daw_theme_art::vector_controls as art;

use crate::controls::track_store::use_track;
use crate::prelude::*;

/// A track's mute button, wired to the DAW.
///
/// Owns the pointer state, reads the track from [`crate::controls::TrackStore`],
/// and toggles through `daw_control` on press. It does not own the mute
/// *state*: the click asks the backend, and the button lights when the
/// backend's event comes back — which is also what makes it light when the
/// track is muted from REAPER's own menu.
#[component]
pub fn MuteButton(
    /// The track's GUID.
    track: String,
    #[props(default)] panel: Panel,
    /// Pixels per art pixel. 1.0 draws the art at its measured size.
    #[props(default = 1.0)]
    scale: f32,
) -> Element {
    // Hover is a Signal here rather than a `:hover` rule in a stylesheet,
    // and that is forced, not chosen. Every non-browser target hands the
    // `<svg>` subtree to a parser where `:hover` is inert, so nothing
    // *inside* the art can know it is hovered. The state has to be owned out
    // here and handed in as a prop — the same prop the exporter sets three
    // ways to draw a sprite strip.
    let mut at = use_signal(art::Interaction::default);
    let muted = use_track(track.clone());
    let muted = muted.read().as_ref().is_some_and(|t| t.muted);

    let named = dress::mute_art(panel, muted);
    let (w, h) = named.source;
    let (w, h) = ((w * scale).round() as u32, (h * scale).round() as u32);

    let guid = track.clone();
    let press = move |_| {
        at.set(art::Interaction::Pressed);
        let guid = guid.clone();
        spawn(async move {
            if let Err(e) = toggle_mute(&guid).await {
                tracing::warn!("mute {guid}: {e}");
            }
        });
    };

    rsx! {
        div {
            // Explicit, inline, and in pixels — layout must not wait for a
            // stylesheet that Blitz may never load. `line-height: 0` keeps
            // the inline `<svg>` from sitting on a text baseline and
            // growing the box by a few pixels, which in a mixer strip is
            // the difference between the buttons lining up and not.
            style: "display:inline-block; line-height:0; width:{w}px; height:{h}px; cursor:pointer;",
            onmouseenter: move |_| at.set(art::Interaction::Hover),
            // Leaving while held must not strand the button in `Pressed`:
            // the pointer is gone and no `mouseup` is coming here.
            onmouseleave: move |_| at.set(art::Interaction::Normal),
            onmousedown: press,
            onmouseup: move |_| at.set(art::Interaction::Hover),
            art::MuteButton {
                width: Some(w),
                height: Some(h),
                at: at(),
                ..dress::mute(named, muted)
            }
        }
    }
}

/// Ask the backend to flip the track's mute.
///
/// Nothing is written locally on the way out. The store updates when the
/// `MuteChanged` event arrives, so the button shows what the DAW actually
/// did rather than what we asked for — and a mute that the backend refuses
/// (or that never reaches it) shows as a button that does not light, not as
/// a UI that disagrees with the project.
async fn toggle_mute(guid: &str) -> eyre::Result<()> {
    let daw = daw_control::Daw::try_get().ok_or_else(|| eyre::eyre!("no DAW connected"))?;
    let project = daw.current_project().await?;
    let track = project
        .tracks()
        .by_guid(guid)
        .await?
        .ok_or_else(|| eyre::eyre!("no track {guid}"))?;
    track.toggle_mute().await?;
    Ok(())
}
