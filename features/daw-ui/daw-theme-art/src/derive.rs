//! Deriving a spec from the image it replaces.
//!
//! The first attempt at component-drawn art authored sizes and nine-slice
//! margins by hand. That produced magenta *visible in REAPER's mixer*,
//! because a theme image is not a free-form drawing: `mcp_bg` ships as
//! **4×4 with two marker pixels**, WALTER blits it at a size the layout
//! expects, and REAPER only reads magenta as geometry when it lands where
//! it expects.
//!
//! So nothing here is authored. Each image's size and marker layout are
//! **read off the PNG being replaced**, and the markers are stamped back
//! verbatim afterwards.
//!
//! That last point matters more than it sounds: it means the exact
//! semantics of WALTER's guides never have to be understood, only
//! preserved. A convention that is copied cannot be copied wrong.

use image::RgbaImage;

/// WALTER's stretch-guide colours: magenta marks non-stretched regions,
/// yellow the outer extents.
pub const MARKERS: [[u8; 4]; 2] = [[255, 0, 255, 255], [255, 255, 0, 255]];

/// The geometry of one theme image, measured from the original.
#[derive(Clone, PartialEq, Debug)]
pub struct DerivedSpec {
    /// Size of the source image, in its own pixels. Not scaled — each DPI
    /// variant is measured from its own file, so a theme whose 150% art
    /// isn't exactly 1.5× still round-trips.
    pub width: u32,
    pub height: u32,
    /// Every marker pixel, as `(x, y, rgba)`, to be stamped back verbatim.
    pub markers: Vec<(u32, u32, [u8; 4])>,
}

impl DerivedSpec {
    /// Measure an image.
    pub fn from_image(img: &RgbaImage) -> Self {
        let mut markers = Vec::new();
        for (x, y, px) in img.enumerate_pixels() {
            if MARKERS.contains(&px.0) {
                markers.push((x, y, px.0));
            }
        }
        Self {
            width: img.width(),
            height: img.height(),
            markers,
        }
    }

    /// Does this image carry stretch guides at all?
    pub fn is_sliced(&self) -> bool {
        !self.markers.is_empty()
    }

    /// Stamp the measured markers onto `img`, replacing whatever is there.
    ///
    /// Applied after rasterising: a rasteriser antialiases, and a marker one
    /// shade off pure magenta is not a marker — REAPER silently stops
    /// slicing and the image smears on resize.
    pub fn stamp(&self, img: &mut RgbaImage) {
        for &(x, y, rgba) in &self.markers {
            if x < img.width() && y < img.height() {
                img.put_pixel(x, y, image::Rgba(rgba));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn img(pixels: &[[u8; 4]]) -> RgbaImage {
        RgbaImage::from_fn(pixels.len() as u32, 1, |x, _| Rgba(pixels[x as usize]))
    }

    #[test]
    fn measures_size_and_finds_markers() {
        let src = img(&[
            [255, 0, 255, 255],
            [0x2a, 0x2a, 0x2a, 255],
            [255, 255, 0, 255],
        ]);
        let spec = DerivedSpec::from_image(&src);
        assert_eq!((spec.width, spec.height), (3, 1));
        assert_eq!(spec.markers.len(), 2, "missed magenta or yellow");
        assert!(spec.is_sliced());
    }

    #[test]
    fn an_unsliced_image_reports_no_markers() {
        let spec = DerivedSpec::from_image(&img(&[[0x2a, 0x2a, 0x2a, 255]]));
        assert!(!spec.is_sliced());
        assert!(spec.markers.is_empty());
    }

    #[test]
    fn stamping_restores_the_exact_original_positions() {
        // The whole point: the convention is copied, never interpreted.
        let src = img(&[
            [255, 0, 255, 255],
            [0x2a, 0x2a, 0x2a, 255],
            [255, 255, 0, 255],
        ]);
        let spec = DerivedSpec::from_image(&src);

        // A freshly "rendered" image with no markers at all.
        let mut drawn = img(&[[1, 2, 3, 255], [1, 2, 3, 255], [1, 2, 3, 255]]);
        spec.stamp(&mut drawn);

        assert_eq!(drawn.get_pixel(0, 0).0, [255, 0, 255, 255]);
        assert_eq!(drawn.get_pixel(2, 0).0, [255, 255, 0, 255]);
        // The art between them is untouched.
        assert_eq!(drawn.get_pixel(1, 0).0, [1, 2, 3, 255]);
    }

    #[test]
    fn stamping_a_smaller_image_does_not_panic() {
        // A component that renders at the wrong size shouldn't take the
        // whole generate run down; the size mismatch is reported elsewhere.
        let spec = DerivedSpec {
            width: 10,
            height: 10,
            markers: vec![(9, 9, [255, 0, 255, 255])],
        };
        let mut small = img(&[[0, 0, 0, 255]]);
        spec.stamp(&mut small);
        assert_eq!(small.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn markers_survive_a_round_trip() {
        let src = img(&[
            [255, 0, 255, 255],
            [0x2a, 0x2a, 0x2a, 255],
            [255, 0, 255, 255],
        ]);
        let spec = DerivedSpec::from_image(&src);
        let mut out = src.clone();
        spec.stamp(&mut out);
        assert_eq!(DerivedSpec::from_image(&out), spec);
    }
}
