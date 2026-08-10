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
//! The pill's width is the other half. `mcp.fx` is [7 7 43 20] with
//! `mcp.fxbyp` butted onto its right, and REAPER reaches those widths by
//! nine-slicing — only the flat middle grows. So the pill is drawn the way
//! the panel sheet draws it: two `FxControl`s at their own sizes, the label
//! half 43 wide and the toggle 28, rather than one 86-wide slice of the
//! source bitmap scaled into the box. Driving the slice art through panes
//! stretched the pill across the whole strip and lost the bypass entirely.

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
    /// The width to draw the pill's labelled half at, in art pixels.
    /// `None` draws it at its source width — read from the art rather than
    /// restated here, so it follows a remeasurement.
    #[props(default)]
    width: Option<f32>,
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

    // The two halves, each at its own size. 46 rather than 49 for the
    // toggle: the join is arithmetically at 43 + the art's own leading
    // seam column, and placed at the join the strip shows through a bare
    // pixel between the halves.
    let w = width.unwrap_or(LABEL_W);
    let h = (SRC_H * scale).round() as u32;
    let chain = if has_fx { art::FxChain::Active } else { art::FxChain::Empty };
    let accent = daw_theme::Theme::default().chrome.accent.css();

    rsx! {
        div {
            style: "position:relative; height:{h}px; width:{w + TOGGLE_W - 3.0}px; \
                    line-height:0; user-select:none; cursor:pointer;",
            onmouseenter: move |_| at.set(art::Interaction::Hover),
            onmouseleave: move |_| at.set(art::Interaction::Normal),

            div { style: "position:absolute; left:0; top:0;",
                art::FxControl {
                    pane: None,
                    part: art::FxPart::Label,
                    chain,
                    bypass: art::FxBypass::Empty,
                    family: art::FxFamily::Mixer,
                    width: Some((w * scale).round() as u32),
                    height: Some(h),
                    at: at(),
                }
            }
            div { style: "position:absolute; left:{(w - 3.0) * scale}px; top:0;",
                art::FxControl {
                    pane: None,
                    part: art::FxPart::Toggle,
                    chain,
                    bypass: art::FxBypass::Empty,
                    family: art::FxFamily::Mixer,
                    width: Some((TOGGLE_W * scale).round() as u32),
                    height: Some(h),
                    at: at(),
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

/// `mcp.fx`, `mcp.fxbyp` and the art's own height, from `rtconfig.txt`.
const LABEL_W: f32 = 43.0;
const TOGGLE_W: f32 = 28.0;
const SRC_H: f32 = 22.0;

