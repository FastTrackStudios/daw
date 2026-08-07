//! Rasterising a component to a REAPER-ready PNG.
//!
//! `rsx!` → SVG string (dioxus-ssr) → pixmap (resvg) → PNG, then the WALTER
//! marker pixels are stamped on.
//!
//! Markers are stamped **after** rasterising and never drawn in the SVG. A
//! rasteriser antialiases everything it touches, and a marker pixel one
//! shade off pure magenta is not a marker — REAPER just stops finding the
//! stretch guides, and the image smears on resize with nothing obviously
//! wrong in the file.

use dioxus::prelude::*;
use image::{Rgba, RgbaImage};

use crate::spec::{ArtSpec, NineSlice};

/// WALTER's stretch-guide colour.
const MARKER: Rgba<u8> = Rgba([255, 0, 255, 255]);

#[derive(Debug)]
pub enum RenderError {
    /// The component produced markup resvg could not parse.
    Svg(String),
    /// resvg refused the target size.
    Pixmap(u32, u32),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Svg(e) => write!(f, "component did not produce valid SVG: {e}"),
            Self::Pixmap(w, h) => write!(f, "could not allocate a {w}x{h} pixmap"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render `element` — which must be an `<svg>` root — to RGBA at `scale`.
pub fn render_png(
    spec: &ArtSpec,
    scale: f32,
    element: fn() -> Element,
) -> Result<RgbaImage, RenderError> {
    let markup = render_to_svg(element);
    let (w, h) = spec.size_at(scale);

    let mut opts = resvg::usvg::Options::default();
    opts.default_size = resvg::usvg::Size::from_wh(spec.width as f32, spec.height as f32)
        .unwrap_or(opts.default_size);
    let tree = resvg::usvg::Tree::from_str(&markup, &opts)
        .map_err(|e| RenderError::Svg(format!("{e}")))?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or(RenderError::Pixmap(w, h))?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut img = RgbaImage::from_raw(w, h, pixmap.take()).ok_or(RenderError::Pixmap(w, h))?;

    if let Some(nine) = spec.nine {
        stamp_markers(&mut img, nine, scale);
    }
    Ok(img)
}

/// Render a component to an SVG string.
///
/// The same components render live in the browser; this is only how they
/// reach a DAW that cannot load SVG.
pub fn render_to_svg(element: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(element);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

/// Write WALTER's stretch guides into the outer row and column.
///
/// REAPER reads the *unmarked* spans as fixed and the marked span as
/// stretchable, so the margins define what must not smear.
///
/// # Known incomplete
///
/// This matches the shape of the convention — marker runs in the outer
/// row/column — but not yet its details. Measured against the shipped art:
/// `mcp_bg` is 4×4 with exactly two markers at `(0,0)` and `(3,3)`, while
/// `mcp_volbg` (23×55) marks only its left and right columns, 16 rows each,
/// and leaves the horizontal axis unmarked entirely. So an image marks only
/// the axes it actually stretches on, and the runs are not full-length.
///
/// Marking both axes on every image — what this does — produced **visible
/// magenta** in REAPER's mixer rather than slice guides. Deriving the
/// marker layout per image from the art being replaced is the fix, and is
/// why [`ArtSpec`] carries the warning it does.
fn stamp_markers(img: &mut RgbaImage, nine: NineSlice, scale: f32) {
    let (w, h) = (img.width(), img.height());
    if w < 3 || h < 3 {
        // Nothing meaningful to slice, and marking the only pixels there
        // are would replace the image with guides.
        return;
    }
    let s = |v: u32| (v as f32 * scale).round() as u32;
    let (l, t, r, b) = (s(nine.l), s(nine.t), s(nine.r), s(nine.b));

    // Top and bottom rows mark the horizontally-stretchable span.
    if l + r + 1 < w {
        for x in l..w.saturating_sub(r) {
            img.put_pixel(x, 0, MARKER);
            img.put_pixel(x, h - 1, MARKER);
        }
    }
    // Left and right columns mark the vertically-stretchable span.
    if t + b + 1 < h {
        for y in t..h.saturating_sub(b) {
            img.put_pixel(0, y, MARKER);
            img.put_pixel(w - 1, y, MARKER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swatch() -> Element {
        rsx! {
            svg {
                width: "20",
                height: "20",
                view_box: "0 0 20 20",
                xmlns: "http://www.w3.org/2000/svg",
                rect { x: "0", y: "0", width: "20", height: "20", fill: "#123456" }
            }
        }
    }

    #[test]
    fn a_component_renders_to_svg_markup() {
        let svg = render_to_svg(swatch);
        assert!(svg.contains("<svg"), "not SVG: {svg}");
        assert!(svg.contains("#123456"), "lost the fill: {svg}");
    }

    #[test]
    fn rasterises_at_every_dpi_scale() {
        let spec = ArtSpec::new("swatch", 20, 20);
        for (scale, want) in [(1.0, 20), (1.5, 30), (2.0, 40)] {
            let img = render_png(&spec, scale, swatch).expect("render");
            assert_eq!((img.width(), img.height()), (want, want), "at {scale}x");
            // And it actually drew: the centre is the fill, not transparent.
            let px = img.get_pixel(want / 2, want / 2);
            assert_eq!(px.0[3], 255, "centre transparent at {scale}x");
        }
    }

    #[test]
    fn the_150_percent_render_is_vector_not_upscaled() {
        // Rendering from the vector at 1.5x must be sharper than blowing up
        // the 100% raster — that is the entire reason to do this.
        let spec = ArtSpec::new("swatch", 20, 20);
        let big = render_png(&spec, 2.0, swatch).unwrap();
        // A vector render of a flat rect has exactly one colour; an
        // upscaled one would have interpolated edge pixels.
        let corner = big.get_pixel(1, 1);
        let middle = big.get_pixel(20, 20);
        assert_eq!(corner, middle, "vector render should be flat");
    }

    #[test]
    fn markers_land_on_the_stretchable_span_only() {
        let spec = ArtSpec::new("btn", 20, 20).with_nine(NineSlice::uniform(6));
        let img = render_png(&spec, 1.0, swatch).unwrap();
        // Inside the margin: fixed, so unmarked.
        assert_ne!(*img.get_pixel(2, 0), MARKER, "corner was marked");
        // Between the margins: stretchable, so marked.
        assert_eq!(*img.get_pixel(10, 0), MARKER, "middle not marked");
        assert_eq!(*img.get_pixel(10, 19), MARKER);
        assert_eq!(*img.get_pixel(0, 10), MARKER);
        assert_eq!(*img.get_pixel(19, 10), MARKER);
    }

    #[test]
    fn markers_are_exactly_magenta() {
        // Anything else is not a marker — REAPER silently stops slicing and
        // the image smears on resize.
        let spec = ArtSpec::new("btn", 20, 20).with_nine(NineSlice::uniform(4));
        let img = render_png(&spec, 1.0, swatch).unwrap();
        assert_eq!(img.get_pixel(10, 0).0, [255, 0, 255, 255]);
    }

    #[test]
    fn markers_scale_with_the_image() {
        // A margin fixed in 100% px must stay proportional, or the corners
        // that "must not stretch" cover the wrong region at 200%.
        let spec = ArtSpec::new("btn", 20, 20).with_nine(NineSlice::uniform(6));
        let img = render_png(&spec, 2.0, swatch).unwrap();
        assert_ne!(*img.get_pixel(6, 0), MARKER, "margin did not scale");
        assert_eq!(*img.get_pixel(20, 0), MARKER);
    }

    #[test]
    fn a_margin_larger_than_the_image_marks_nothing() {
        // Rather than panicking on an underflowing range, or marking the
        // whole edge and destroying the art.
        let spec = ArtSpec::new("btn", 20, 20).with_nine(NineSlice::uniform(40));
        let img = render_png(&spec, 1.0, swatch).unwrap();
        for x in 0..20 {
            assert_ne!(*img.get_pixel(x, 0), MARKER);
        }
    }

    #[test]
    fn no_nine_slice_means_no_markers() {
        let spec = ArtSpec::new("flat", 20, 20);
        let img = render_png(&spec, 1.0, swatch).unwrap();
        for x in 0..20 {
            assert_ne!(*img.get_pixel(x, 0), MARKER);
            assert_ne!(*img.get_pixel(x, 19), MARKER);
        }
    }
}
