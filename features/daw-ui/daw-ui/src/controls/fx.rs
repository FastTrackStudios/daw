//! The FX button, lit by a chain that actually reports itself.
//!
//! The stale-state bug this exists to close: `Track::fx_count` was seeded
//! by the bulk track read and then never updated by anything. `TrackEvent`
//! had thirteen variants and not one of them mentioned FX, so a mixer's FX
//! buttons were correct when it opened and wrong from the first plugin the
//! user added. `TrackEvent::FxCountChanged` is the missing event, and both
//! backends now publish it — REAPER's track poller diffs the counts it was
//! already reading, and the standalone backend recounts whenever a chain
//! gains or loses an entry.
//!
//! It rides the *track* stream deliberately. A lit button is track state;
//! what should force a chain-level subscription is a surface that renders
//! the chain.
//!
//! The pill's width is the other half. `mcp.fx` is a 43-wide box against 28
//! of art, and scaling into it elongates the rounded end into a notch and
//! stretches the `FX` glyph with it. So the pill is drawn as
//! [a row of panes][daw_theme_art::slice::NamedArt::row]: the end and the
//! glyph hold their size, the flat run before the seam takes the slack.

use daw_theme_art::slice::{self, Pane};
use daw_theme_art::vector_controls as art;

use crate::controls::use_track;
use crate::prelude::*;

/// A track's FX button.
///
/// Lit when the track's chain is non-empty, and — unlike everything that
/// came before it — still correct an hour later.
#[component]
pub fn FxButton(
    /// The track's GUID.
    track: String,
    /// The width to draw the pill's labelled half at, in art pixels. The
    /// default is its source width; anything larger grows the flat run.
    #[props(default = 28.0)]
    width: f32,
    /// Pixels per art pixel.
    #[props(default = 1.0)]
    scale: f32,
) -> Element {
    let mut at = use_signal(art::Interaction::default);
    let info = use_track(track.clone());
    let (has_fx, has_input_fx) = info
        .read()
        .as_ref()
        .map(|t| (t.fx_count > 0, t.input_fx_count > 0))
        .unwrap_or((false, false));

    let named = slice::expect_art(if has_fx { "mcp_fx_norm" } else { "mcp_fx_empty" });
    let (src_w, src_h) = named.source;
    let panes = named.row();
    // The slack goes to the one growing pane; the fixed ones keep their
    // source width. Stated in pixels here rather than left to flex-grow so
    // a pane's width is a fact and not a division of leftovers.
    let slack = ((width - src_w) * scale).max(0.0);
    let h = (src_h * scale).round() as u32;
    // The same accent the lit routing lanes use, from the one palette.
    let accent = daw_theme::Theme::default().chrome.accent.css();

    rsx! {
        div {
            style: "display:flex; align-items:stretch; height:{h}px; line-height:0; \
                    user-select:none; cursor:pointer; position:relative;",
            onmouseenter: move |_| at.set(art::Interaction::Hover),
            onmouseleave: move |_| at.set(art::Interaction::Normal),

            for (i, pane) in panes.into_iter().enumerate() {
                FxBand {
                    key: "{i}",
                    pane,
                    lit: has_fx,
                    at: at(),
                    // Only the growing band takes the extra width.
                    width: (pane.view.2 * scale).round() as u32 + if pane.grow { slack.round() as u32 } else { 0 },
                    height: h,
                }
            }

            // The input-FX indicator: the *input* chain, which is a
            // different chain from the one the pill reports and so gets its
            // own mark rather than lighting the same button.
            if has_input_fx {
                div {
                    style: "position:absolute; left:1px; top:1px; width:3px; height:3px; \
                            background:{accent};",
                }
            }
        }
    }
}

/// One band of the pill's labelled half.
#[component]
fn FxBand(pane: Pane, lit: bool, at: art::Interaction, width: u32, height: u32) -> Element {
    rsx! {
        art::FxControl {
            chain: if lit { art::FxChain::Active } else { art::FxChain::Empty },
            part: art::FxPart::Label,
            pane: Some(pane),
            width: Some(width),
            height: Some(height),
            at,
        }
    }
}
