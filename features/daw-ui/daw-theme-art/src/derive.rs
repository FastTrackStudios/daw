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

/// How many equal-width cells an image is a sprite strip of.
///
/// REAPER packs a control's interaction states side by side: `mcp_solo_off`
/// is three 21x20 cells — normal, hover, pressed — in one 63x20 file.
/// Drawing the whole file draws all three, which is right for REAPER (it
/// blits the cell it wants) and wrong everywhere else.
pub fn sprite_cells(img: &RgbaImage) -> u32 {
    cell_bounds(img).len() as u32
}

/// Where each sprite cell actually sits, as `(x, width)`.
///
/// Found by **correlating the image's column profile with itself**, which
/// is the only method here that survived contact with the whole theme.
/// Four earlier attempts each worked on the images they were written for
/// and failed on the next family:
///
/// - *Even division.* `track_mute_on` is 65px of three buttons — 21.67
///   each — so `w % n == 0` rejected it and the export stretched one
///   button across the whole strip.
/// - *Transparent gaps.* Finds strips whose cells are separated by clear
///   columns, and nothing else; the track panel's buttons touch.
/// - *Alpha silhouette.* Says almost nothing about opaque rectangles —
///   a 3-cell strip matches `n = 4` happily.
/// - *Colour, or edge maps.* Too strict in the other direction: the cells
///   of a strip are the same button lit differently, so hover and pressed
///   differ from normal everywhere at once.
///
/// A z-normalised profile is brightness-invariant by construction, which
/// is exactly the property the last two lacked, and correlating it finds
/// the period *and* the offset — `mcp_fx_norm` turns out to be three 28px
/// cells starting at x=1, with a marker column before them and a spare
/// after, which no arithmetic on 86 and 3 will ever produce.
pub fn cell_bounds(img: &RgbaImage) -> Vec<(u32, u32)> {
    match detect_strip(img) {
        Some((n, period, origin)) => (0..n).map(|i| (origin + i * period, period)).collect(),
        None => vec![(0, img.width())],
    }
}

/// Mean brightness of each column, weighted by alpha.
///
/// Markers are skipped: they are WALTER geometry rather than art, sit in
/// the outer row and column, and are bright enough to dominate a profile.
fn column_profile(img: &RgbaImage) -> Vec<f32> {
    let (w, h) = (img.width(), img.height());
    (0..w)
        .map(|x| {
            let sum: f32 = (0..h)
                .map(|y| {
                    let p = img.get_pixel(x, y).0;
                    if MARKERS.contains(&p) {
                        return 0.0;
                    }
                    let l = p[0] as f32 * 0.3 + p[1] as f32 * 0.6 + p[2] as f32 * 0.1;
                    l * p[3] as f32 / 255.0
                })
                .sum();
            sum / h as f32
        })
        .collect()
}

/// Shift and scale a slice to zero mean and unit deviation.
fn znorm(v: &[f32]) -> Vec<f32> {
    let n = v.len() as f32;
    let mean = v.iter().sum::<f32>() / n;
    let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n;
    let sd = var.sqrt().max(1e-6);
    v.iter().map(|x| (x - mean) / sd).collect()
}

/// `(cells, period, origin)` for the best-fitting strip, if any.
fn detect_strip(img: &RgbaImage) -> Option<(u32, u32, u32)> {
    let w = img.width();
    if w < 12 {
        return None;
    }
    let profile = column_profile(img);
    let guides = marker_columns(img);

    // Larger counts first: a 4-cell strip also half-matches at 2.
    for n in [4u32, 3, 2] {
        let widest = w / n;
        if widest < 6 {
            continue;
        }
        // The cells cover nearly the whole file — anything less is a
        // coincidence between two parts of one drawing.
        let narrowest = ((w as f32 * 0.85 / n as f32) as u32).max(6);
        let mut found: Vec<(f32, u32, u32, u32)> = Vec::new();

        for period in narrowest..=widest {
            // Strips start at the very edge or one guide column in; a
            // wider search costs time and finds only noise.
            for origin in 0..=(w - n * period).min(3) {
                let segs: Vec<Vec<f32>> = (0..n)
                    .map(|c| {
                        let a = (origin + c * period) as usize;
                        znorm(&profile[a..a + period as usize])
                    })
                    .collect();
                let score = (1..n as usize)
                    .map(|c| {
                        segs[0].iter().zip(&segs[c]).map(|(a, b)| a * b).sum::<f32>()
                            / period as f32
                    })
                    .fold(f32::INFINITY, f32::min);
                // How many guide columns this layout swallows. A cell that
                // begins on a magenta column is a layout shifted by one:
                // the guides sit *outside* the cells, and offsets either
                // side of the truth score within a thousandth of each
                // other, so the correlation alone cannot choose. Two
                // controls landed a pixel left before this was added.
                let swallowed = (0..n)
                    .flat_map(|c| {
                        let a = origin + c * period;
                        (a..a + period).map(|x| guides[x as usize] as u32)
                    })
                    .sum();
                found.push((score, swallowed, period, origin));
            }
        }

        let best = found
            .iter()
            .map(|(s, ..)| *s)
            .fold(f32::NEG_INFINITY, f32::max);
        if best > 0.9 {
            let (_, _, period, origin) = found
                .into_iter()
                .filter(|(s, ..)| best - s < 0.01)
                .min_by_key(|(_, swallowed, _, origin)| (*swallowed, *origin))
                .expect("a best exists");
            return Some((n, period, origin));
        }
    }
    None
}

/// Which columns hold WALTER guide pixels.
fn marker_columns(img: &RgbaImage) -> Vec<bool> {
    (0..img.width())
        .map(|x| (0..img.height()).any(|y| MARKERS.contains(&img.get_pixel(x, y).0)))
        .collect()
}

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
    /// Where each sprite cell sits, as `(x, width)` — measured, not
    /// divided. See [`cell_bounds`].
    pub cells: Vec<(u32, u32)>,
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
            cells: cell_bounds(img),
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
            cells: vec![(0, 10)],
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
