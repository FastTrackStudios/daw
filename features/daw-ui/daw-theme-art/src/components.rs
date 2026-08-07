//! The artwork itself.
//!
//! Each component draws one piece of chrome from the palette. The browser
//! renders these live as SVG; `render` rasterises them for REAPER.
//!
//! # Components draw into a size they are given
//!
//! Every component takes the pixel size it must fill. That is not a style
//! choice — REAPER's images have sizes WALTER expects (`mcp_bg` is 4×4),
//! and a component that picks its own produces art REAPER blits wrongly and
//! magenta it renders as visible pixels. The size comes from
//! [`crate::derive::DerivedSpec`], measured off the image being replaced.
//!
//! Which means a component cannot assume it has room for detail. `mcp_bg`
//! at 4×4 is a nine-slice source — REAPER stretches it — so the useful
//! thing to draw is a correct one-pixel border and fill, not a small
//! picture of a mixer strip.

use daw_theme::Theme;
use dioxus::prelude::*;

/// Props every art component takes: the box to fill, in source pixels.
#[derive(Props, Clone, PartialEq)]
pub struct ArtProps {
    pub width: u32,
    pub height: u32,
}

/// The panel behind a mixer strip.
///
/// A nine-slice source: REAPER stretches the middle, so this is a border
/// plus a fill, and the corners carry whatever radius fits.
#[component]
pub fn McpBg(props: ArtProps) -> Element {
    let t = Theme::default();
    let c = &t.chrome;
    let (w, h) = (props.width, props.height);
    // At 4×4 a 1px stroke *is* the whole border; radius has to stay under
    // half the smaller side or the shape collapses.
    let radius = (w.min(h) as f32 / 2.0 - 0.5).clamp(0.0, t.metrics.radius);
    rsx! {
        svg {
            width: "{w}",
            height: "{h}",
            view_box: "0 0 {w} {h}",
            xmlns: "http://www.w3.org/2000/svg",
            rect {
                x: "0.5",
                y: "0.5",
                width: "{w as f32 - 1.0}",
                height: "{h as f32 - 1.0}",
                rx: "{radius}",
                fill: "{c.surface_raised.css()}",
                stroke: "{c.border.css()}",
                stroke_width: "1",
            }
        }
    }
}

/// The trough a mixer fader runs in.
#[component]
pub fn McpVolBg(props: ArtProps) -> Element {
    let t = Theme::default();
    let c = &t.chrome;
    let (w, h) = (props.width, props.height);
    // The groove sits centred in whatever width the source uses; the art
    // either side is chrome the strip shows through.
    let groove = (w as f32 * 0.35).max(3.0);
    let x = (w as f32 - groove) / 2.0;
    rsx! {
        svg {
            width: "{w}",
            height: "{h}",
            view_box: "0 0 {w} {h}",
            xmlns: "http://www.w3.org/2000/svg",
            rect {
                x: "{x}",
                y: "0.5",
                width: "{groove}",
                height: "{h as f32 - 1.0}",
                rx: "{groove / 2.0}",
                fill: "{c.surface_sunken.css()}",
                stroke: "{c.border.css()}",
                stroke_width: "1",
            }
        }
    }
}

/// Everything this crate can draw, by REAPER image name.
///
/// A component is only used for a name the theme actually ships — the
/// generator looks each one up in the source art and skips what it can't
/// measure.
pub fn registry() -> Vec<(&'static str, fn(ArtProps) -> Element)> {
    vec![("mcp_bg", McpBg), ("mcp_volbg", McpVolBg)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render_sized;

    #[test]
    fn every_component_draws_at_the_sizes_reaper_actually_uses() {
        // The sizes that broke the first attempt: tiny nine-slice sources.
        for (name, f) in registry() {
            for (w, h) in [(4, 4), (23, 55), (16, 60), (2, 2)] {
                let img =
                    render_sized(f, w, h).unwrap_or_else(|e| panic!("{name} at {w}x{h}: {e}"));
                assert_eq!(img.dimensions(), (w, h), "{name} at {w}x{h}");
            }
        }
    }

    #[test]
    fn every_component_draws_something_at_a_tiny_size() {
        // A 4×4 that renders empty is the failure mode that looks exactly
        // like "REAPER ignored the file".
        for (name, f) in registry() {
            let img = render_sized(f, 4, 4).unwrap();
            let opaque = img.pixels().filter(|p| p.0[3] > 0).count();
            assert!(opaque > 0, "{name} rendered empty at 4x4");
        }
    }

    #[test]
    fn components_draw_from_the_palette_not_literals() {
        let svg = crate::render::render_to_svg_sized(McpBg, 40, 40);
        assert!(
            svg.contains(&Theme::default().chrome.surface_raised.to_hex()),
            "McpBg did not draw with the palette surface: {svg}"
        );
    }

    #[test]
    fn a_degenerate_size_does_not_produce_invalid_svg() {
        // 1×1 makes radius and inset arithmetic go negative, which yields
        // SVG resvg rejects — and the error surfaces as "component did not
        // produce valid SVG", a long way from the cause.
        for (name, f) in registry() {
            assert!(render_sized(f, 1, 1).is_ok(), "{name} broke at 1x1");
        }
    }
}
