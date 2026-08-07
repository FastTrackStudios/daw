//! The artwork itself.
//!
//! Each component draws one piece of chrome from the palette and returns an
//! `<svg>` root. The browser renders these live; `render` rasterises them
//! for REAPER.
//!
//! Components take no props today because the rasteriser needs a plain
//! `fn() -> Element` and the theme is the process-wide default. Parameterising
//! by state (hover, pressed, armed) is the next step and is what turns ~450
//! images into a few dozen components.

use daw_theme::{Theme, defaults as d};
use dioxus::prelude::*;

use crate::spec::{ArtSpec, NineSlice};

/// The panel behind a mixer strip.
///
/// Nine-sliced: REAPER stretches it to the strip's height, so the rounded
/// top and bottom must stay put while the middle grows.
pub const MCP_BG: ArtSpec = ArtSpec::new("mcp_bg", 40, 40).with_nine(NineSlice::uniform(6));

#[component]
pub fn McpBg() -> Element {
    let t = Theme::default();
    let c = &t.chrome;
    rsx! {
        svg {
            width: "40",
            height: "40",
            view_box: "0 0 40 40",
            xmlns: "http://www.w3.org/2000/svg",
            // Body.
            rect {
                x: "0.5",
                y: "0.5",
                width: "39",
                height: "39",
                rx: "{t.metrics.radius}",
                fill: "{c.surface_raised.css()}",
                stroke: "{c.border.css()}",
                stroke_width: "1",
            }
            // A one-pixel top highlight, the cheapest way to make a flat
            // rectangle read as a raised panel rather than a hole.
            rect {
                x: "1.5",
                y: "1",
                width: "37",
                height: "1",
                fill: "{c.surface_raised.shade(0.10).css()}",
            }
        }
    }
}

/// The trough a mixer fader runs in.
pub const MCP_VOLBG: ArtSpec = ArtSpec::new("mcp_volbg", 16, 60).with_nine(NineSlice {
    l: 0,
    t: 6,
    r: 0,
    b: 6,
});

#[component]
pub fn McpVolBg() -> Element {
    let t = Theme::default();
    let c = &t.chrome;
    rsx! {
        svg {
            width: "16",
            height: "60",
            view_box: "0 0 16 60",
            xmlns: "http://www.w3.org/2000/svg",
            rect {
                x: "5",
                y: "1",
                width: "6",
                height: "58",
                rx: "3",
                fill: "{c.surface_sunken.css()}",
                stroke: "{c.border.css()}",
                stroke_width: "1",
            }
        }
    }
}

/// A generic panel button face, unlit.
pub const GEN_BUTTON: ArtSpec = ArtSpec::new("gen_button", 24, 18).with_nine(NineSlice::uniform(5));

#[component]
pub fn GenButton() -> Element {
    let t = Theme::default();
    let c = &t.chrome;
    rsx! {
        svg {
            width: "24",
            height: "18",
            view_box: "0 0 24 18",
            xmlns: "http://www.w3.org/2000/svg",
            rect {
                x: "0.5",
                y: "0.5",
                width: "23",
                height: "17",
                rx: "4",
                fill: "{d::CONTROL}",
                stroke: "{c.border.css()}",
                stroke_width: "1",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every component here, with the spec that describes it.
    fn all() -> Vec<(ArtSpec, fn() -> Element)> {
        vec![
            (MCP_BG, McpBg),
            (MCP_VOLBG, McpVolBg),
            (GEN_BUTTON, GenButton),
        ]
    }

    #[test]
    fn every_component_produces_parseable_svg() {
        for (spec, f) in all() {
            let svg = crate::render::render_to_svg(f);
            assert!(svg.contains("<svg"), "{} is not SVG: {svg}", spec.name);
            assert!(
                svg.contains("xmlns"),
                "{} lacks xmlns — resvg will reject it",
                spec.name
            );
        }
    }

    #[test]
    fn every_component_rasterises_at_every_scale() {
        for (spec, f) in all() {
            for scale in [1.0, 1.5, 2.0] {
                let img = crate::render::render_png(&spec, scale, f)
                    .unwrap_or_else(|e| panic!("{} at {scale}x: {e}", spec.name));
                assert_eq!(img.dimensions(), spec.size_at(scale), "{}", spec.name);
            }
        }
    }

    #[test]
    fn every_component_draws_something() {
        // A typo in an attribute yields valid, empty SVG — which rasterises
        // to a fully transparent image and looks like "REAPER ignored it".
        for (spec, f) in all() {
            let img = crate::render::render_png(&spec, 1.0, f).unwrap();
            let opaque = img.pixels().filter(|p| p.0[3] > 0).count();
            assert!(
                opaque > (img.width() * img.height() / 4) as usize,
                "{} rendered mostly empty ({opaque} opaque px)",
                spec.name
            );
        }
    }

    #[test]
    fn components_use_the_palette_not_literals() {
        // The point of the exercise: change the palette, the art follows.
        let svg = crate::render::render_to_svg(McpBg);
        assert!(
            svg.contains(&Theme::default().chrome.surface_raised.to_hex()),
            "McpBg did not draw with the palette surface: {svg}"
        );
    }
}
