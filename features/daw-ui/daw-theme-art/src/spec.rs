//! What a piece of theme art is: a name, a size, and where it may stretch.

/// The nine-slice guides REAPER reads from an image's outermost pixels.
///
/// WALTER learns which bands of an image may stretch from **magenta pixels**
/// (`#FF00FF`) in the outer row and column: the marked spans stretch, the
/// unmarked corners do not. Get this wrong and a button's rounded corners
/// smear when REAPER resizes it.
///
/// These are stamped after rasterising, never drawn in the SVG — a
/// rasteriser antialiases, and a marker that is one shade off pure magenta
/// is not a marker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NineSlice {
    /// Fixed margins in px at 100%: left, top, right, bottom. Everything
    /// between them stretches.
    pub l: u32,
    pub t: u32,
    pub r: u32,
    pub b: u32,
}

impl NineSlice {
    pub const fn uniform(m: u32) -> Self {
        Self {
            l: m,
            t: m,
            r: m,
            b: m,
        }
    }

    /// Only the horizontal middle stretches — a strip that tiles sideways.
    pub const fn horizontal(l: u32, r: u32) -> Self {
        Self { l, t: 0, r, b: 0 }
    }
}

/// One image the theme needs.
///
/// # Sizes are not yours to choose
///
/// **A spec must be derived from the image it replaces, not invented.**
/// WALTER blits these at dimensions the layout expects, and REAPER only
/// treats magenta as a slice guide when the image matches what it expects
/// there — `mcp_bg` ships as **4×4 with two marker pixels**, and a
/// hand-authored 40×40 with edge lines renders the magenta as *visible
/// pixels* in the mixer instead of being read as geometry.
///
/// So the route to component-drawn art is: read each shipped PNG's size and
/// marker layout, generate the spec from it, then draw into that. Authoring
/// the numbers by hand produces something that looks right in a test and is
/// wrong in REAPER.
#[derive(Clone, Debug)]
pub struct ArtSpec {
    /// REAPER's file name, without extension or scale folder — `mcp_bg`.
    pub name: &'static str,
    /// Size in px at 100%. The 150% and 200% variants are rendered from the
    /// same vector source rather than scaled, so they stay crisp.
    pub width: u32,
    pub height: u32,
    /// Stretch guides, if REAPER resizes this image.
    pub nine: Option<NineSlice>,
}

impl ArtSpec {
    pub const fn new(name: &'static str, width: u32, height: u32) -> Self {
        Self {
            name,
            width,
            height,
            nine: None,
        }
    }

    pub const fn with_nine(mut self, nine: NineSlice) -> Self {
        self.nine = Some(nine);
        self
    }

    /// Pixel size at a DPI scale.
    ///
    /// Rounds rather than truncates: at 150% a 15px element truncates to 22
    /// and loses a pixel against its neighbours, which shows up as a 1px
    /// seam in a nine-sliced strip.
    pub fn size_at(&self, scale: f32) -> (u32, u32) {
        (
            (self.width as f32 * scale).round().max(1.0) as u32,
            (self.height as f32 * scale).round().max(1.0) as u32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_round_rather_than_truncate() {
        let s = ArtSpec::new("x", 15, 15);
        assert_eq!(s.size_at(1.0), (15, 15));
        // 22.5 → 23, not 22: truncating leaves a 1px seam in a sliced strip.
        assert_eq!(s.size_at(1.5), (23, 23));
        assert_eq!(s.size_at(2.0), (30, 30));
    }

    #[test]
    fn a_zero_size_never_reaches_the_rasteriser() {
        // resvg refuses a 0-dimension pixmap, and the error is opaque.
        assert_eq!(ArtSpec::new("x", 1, 1).size_at(0.1), (1, 1));
    }

    #[test]
    fn nine_slice_helpers_describe_what_they_say() {
        assert_eq!(
            NineSlice::uniform(4),
            NineSlice {
                l: 4,
                t: 4,
                r: 4,
                b: 4
            }
        );
        let h = NineSlice::horizontal(3, 5);
        assert_eq!((h.l, h.r), (3, 5));
        assert_eq!((h.t, h.b), (0, 0), "horizontal must not pin vertically");
    }
}
