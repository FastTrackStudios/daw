//! What a VU movement *looks like*, as data.
//!
//! The companion to [`knob_kit`](crate::hardware::knob_kit), and the same
//! bargain: geometry lives in [`vu_svg`](crate::hardware::vu_svg) as pure
//! functions, the *drawing* is one [`VuSpec`] const per face, and the
//! renderer walks it without knowing which unit it is painting.
//!
//! A meter is a printed card behind glass, lit from behind by a bulb or two,
//! with a needle swinging over it and — if it is mounted through the panel
//! rather than printed on one — a bezel around it. Everything a theme wants
//! to move is one of those.
//!
//! # Adding a face
//!
//! Add a [`VuFace`](crate::hardware::vu::VuFace) variant and one `VuSpec`
//! in [`vu_faces`](crate::hardware::vu_faces). Then look at it:
//!
//! ```sh
//! cargo test -p fts-ui-audio --test vu_sheet
//! ```
//!
//! which paints every face in the kit, at rest and swinging, with and without
//! its bezel.
//!
//! # What is *not* a theme
//!
//! The card's numbers and where they sit. A VU is crowded at the bottom and a
//! decibel readout is evenly spaced, and that is the difference between two
//! *instruments*, not two colour schemes — so it stays in
//! [`VuScale`](crate::hardware::vu_svg::VuScale) where the geometry can test
//! it. A face says how the meter is lit and printed; a scale says what it
//! reads.

/// The needle and the hub it swings on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Needle {
    pub color: &'static str,
    /// Stroke width, in the card's viewBox units.
    pub width: f64,
    /// The hub's radius. It sits at the pivot, below the bottom of the card,
    /// so only its top half shows — which is what a real movement looks like.
    pub hub_r: f64,
    pub hub_opacity: f64,
}

/// The bulb behind the card.
///
/// The lamp is what actually varies between units of the same era — the card
/// is ivory on nearly all of them, and it is the glow that makes an LA-2A
/// warm and a rackmount's whiter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    pub color: &'static str,
    /// Where the bulb sits behind the card, as percentages across and down.
    pub x: f64,
    pub y: f64,
    /// How far the glow reaches, as a percentage of the card.
    pub reach: f64,
}

/// The frame a movement mounted *through* a panel sits in.
///
/// Lit from above, a sunken opening has its top face in shadow and its bottom
/// face catching the light — the opposite of a raised boss, and the whole
/// difference between a movement set into a panel and one printed on it. Get
/// those two the wrong way round and the meter reads as a sticker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezel {
    /// The frame body.
    pub frame: &'static str,
    /// The chamfer's four faces, in shadow-to-light order.
    pub top: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub bottom: &'static str,
    /// Chamfer depth, in design px.
    pub depth: f64,
    /// The vent under the glass, which is most of what says the movement is
    /// mounted through the panel. `None` for a frame without one.
    pub vent: Option<Vent>,
}

/// The louvre under a meter's glass.
///
/// Structured rather than a finished gradient string because the stripe pitch
/// is in *design* px and has to scale with the panel like everything else. A
/// frozen gradient stays 2 px wide on a panel drawn at three times the size,
/// where it reads as a smear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vent {
    pub dark: &'static str,
    pub light: &'static str,
    /// One dark stripe plus one light stripe, in design px.
    pub pitch: f64,
}

impl Vent {
    /// The louvre at a panel's scale.
    pub fn css(&self, scale: f64) -> String {
        let half = self.pitch * scale / 2.0;
        let full = self.pitch * scale;
        format!(
            "repeating-linear-gradient(90deg, {} 0 {half:.2}px, {} {half:.2}px {full:.2}px)",
            self.dark, self.light,
        )
    }
}

/// A VU face, as data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VuSpec {
    /// The printed card behind the scale.
    pub card: &'static str,
    /// Everything silkscreened on it: the arc, the ticks, the numerals, the
    /// legend.
    pub ink: &'static str,
    /// The over-zero stretch of the scale — red on every VU ever made, and
    /// not present at all on a decibel readout.
    pub hot: &'static str,
    pub needle: Needle,
    pub lamp: Option<Lamp>,
    /// The reflection on the glass over the card. `None` for an open face.
    pub glass: Option<&'static str>,
    pub bezel: Bezel,
}

impl VuSpec {
    /// The lamp's CSS, ready to paint.
    pub fn lamp_css(&self) -> Option<String> {
        self.lamp.map(|l| {
            format!(
                "radial-gradient(ellipse at {:.0}% {:.0}%, {} 0%, rgba(0,0,0,0) {:.0}%)",
                l.x, l.y, l.color, l.reach,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::vu::VuFace;

    /// Every face resolves to a spec, and every spec is a meter you could
    /// actually read: something printed on it, a needle wide enough to see,
    /// a hub at the pivot.
    #[test]
    fn every_face_in_the_kit_is_well_formed() {
        for face in VuFace::ALL {
            let spec = face.spec();
            let name = format!("{face:?}");

            assert!(!spec.card.is_empty(), "{name} has no card");
            assert!(!spec.ink.is_empty(), "{name} prints in nothing");
            assert!(
                spec.needle.width > 0.0,
                "{name}'s needle has no width, so it cannot be read",
            );
            assert!(spec.needle.hub_r > 0.0, "{name} has no hub");
            assert!(
                (0.0..=1.0).contains(&spec.needle.hub_opacity),
                "{name}'s hub opacity is not an opacity",
            );
            if let Some(l) = spec.lamp {
                assert!(
                    (0.0..=100.0).contains(&l.x) && (0.0..=100.0).contains(&l.y),
                    "{name}'s bulb is off the card",
                );
                assert!(l.reach > 0.0, "{name}'s lamp reaches nowhere");
            }
        }
    }

    /// The needle has to contrast with the card, or the meter is unreadable —
    /// which is the one way a colour scheme can be *wrong* rather than merely
    /// not to taste. Checked crudely, on the leading hex of each.
    #[test]
    fn a_needle_never_matches_its_own_card() {
        for face in VuFace::ALL {
            let spec = face.spec();
            assert!(
                !spec.card.contains(spec.needle.color),
                "{face:?}'s needle is the same colour as its card",
            );
        }
    }

    /// A sunken bezel is lit from above: the top face is the darkest of the
    /// four and the bottom the lightest. Inverting them is the difference
    /// between an opening and a boss, and it is invisible in code.
    #[test]
    fn a_bezels_opening_reads_as_sunken_rather_than_raised() {
        for face in VuFace::ALL {
            let b = face.spec().bezel;
            let lum = |c: &str| -> u32 {
                u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0)
            };
            assert!(
                lum(b.top) < lum(b.bottom),
                "{face:?}'s bezel is lit like a raised boss, not an opening",
            );
            assert!(
                lum(b.left) < lum(b.right),
                "{face:?}'s bezel is lit from the wrong side",
            );
            assert!(b.depth > 0.0, "{face:?}'s bezel has no depth");
        }
    }

    /// The louvre's stripes are in design px, so they widen with the panel.
    /// A frozen gradient string would not, and at three times the size it
    /// reads as a smear.
    #[test]
    fn a_vent_scales_with_the_panel() {
        let v = Vent {
            dark: "#000",
            light: "#2a2c2e",
            pitch: 4.0,
        };
        assert!(v.css(1.0).contains("2.00px"), "{}", v.css(1.0));
        assert!(v.css(1.0).contains("4.00px"));
        assert!(v.css(3.0).contains("6.00px"), "{}", v.css(3.0));
        assert!(v.css(3.0).contains("12.00px"));
    }

    #[test]
    fn a_lamp_becomes_a_gradient_and_an_unlit_face_does_not() {
        let lit = VuSpec {
            lamp: Some(Lamp {
                color: "rgba(1,2,3,0.5)",
                x: 50.0,
                y: 8.0,
                reach: 68.0,
            }),
            ..*VuFace::Amber.spec()
        };
        let css = lit.lamp_css().expect("a lit face has a lamp");
        assert!(css.contains("50% 8%"), "the bulb moved: {css}");
        assert!(css.contains("68%"), "the reach was dropped: {css}");

        let dark = VuSpec {
            lamp: None,
            ..*VuFace::Amber.spec()
        };
        assert!(dark.lamp_css().is_none());
    }
}
