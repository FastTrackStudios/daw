//! A hardware knob — pointer, skirt, and a scale ring printed on the panel.
//!
//! Different from [`fts_ui_audio::Knob`] in the way that matters: an FTS knob
//! draws its value as an arc, because the value is the point. A hardware knob
//! draws a *pointer*, and the value is read off numbers silkscreened on the
//! panel around it — which is why the ring here is drawn even where the
//! pointer is not, and why the numbers are the unit's own (LA-2A GAIN reads
//! 0–10, 1176 INPUT reads -48…+12) rather than the engine's.
//!
//! Geometry lives in [`crate::hardware::knob_svg`]. Dragging goes through the
//! same [`DragProvider`](fts_ui_audio::drag::DragProvider) as every other FTS
//! control, so a hardware face behaves like the rest of the editor.

use dioxus::prelude::*;
use crate::drag::{begin_drag, DragState};
use crate::prelude::*;

use crate::hardware::knob_svg::{knob_angle, pointer_polygon, ring_arc_path, ring_point, ScaleMark};

/// Design-space radii inside the knob's own `-55 -55 110 110` viewBox.
const BODY_R: f64 = 30.0;
/// Default radii for the printed ring and its numerals. Panels that print
/// their numbers close in around the skirt override them — see
/// [`Ring::geometry`](crate::hardware::rack::Ring::geometry).
const RING_R: f64 = 41.0;
const LABEL_R: f64 = 50.0;

/// Daka-Ware's skirt: coarse lobes you can count across a room, which is what
/// the EQP-1A's knobs read as in a photograph. Fewer and deeper than a knurl.
const DAKA_LOBES: usize = 22;
const DAKA_LOBE_DEPTH: f64 = 2.4;
/// Fine ridging around the raised body's wall, above the skirt.
const DAKA_BODY_RIDGES: usize = 40;

/// The 1073 collar's teeth. Not a knurl — the ring is *geared*, with bumps
/// coarse enough to count, and that toothed silhouette is what the knob reads
/// as before any shading does.
const NEVE_TEETH: usize = 18;
const NEVE_TOOTH_DEPTH: f64 = 3.4;

/// How far a Marconi wing overhangs the skirt it is mounted on, as a multiple
/// of the skirt's radius, and how far its tail runs the other way.
///
/// The wing is a bar laid *across* the knob and overhanging it — that overhang
/// is the whole silhouette, and a wing contained inside its own disc reads as
/// a stripe painted on a circle. `ring_offset` moves the printed scale out to
/// make room for it.
const WING_REACH: f64 = 1.26;
const WING_TAIL: f64 = 0.66;

/// Pixels of vertical drag per full sweep. Looser than the FTS knob's 150 —
/// these are big knobs with printed scales, and a coarse feel suits them.
const SENSITIVITY: f64 = 190.0;
const WHEEL_STEP: f64 = 0.02;
const WHEEL_STEP_FINE: f64 = 0.005;

/// How the knob is built.
///
/// These are the actual knobs the units wear, because the shape is most of
/// what you recognise a panel by before you read a word of it:
///
/// - **Daka-Ware** (Pultec EQP-1A): black phenolic, a 1⅛" ridged body sitting
///   on a 1½" skirt — so the skirt is a third wider than the grip — with the
///   indicator *engraved into the body and filled white* rather than moulded
///   as a pointer.
/// - **Marconi** (Neve 1073, and the 1081/1272): a coloured two-piece knob
///   whose body carries a raised wing to grip, over a wider skirt, with a
///   white line down the wing. Neve's colour coding rides on these — red gain,
///   blue filters, grey shelf.
/// - **Collet** (SSL 4000 channel): a flat-topped coloured cap with a fluted
///   rim and a single white bar across the top. No skirt: the panel prints the
///   travel as dots around it instead.
/// - **Silver-top** (UREI 1176): a wide matte black collar with a *brushed,
///   knurled aluminium cap* set into the middle of it, and the index a white
///   line on the **collar** — outside the cap, not across it. That is the
///   arrangement that makes an 1176's knobs read as rings from across a room:
///   a dark annulus around a bright disc. INPUT and OUTPUT take the large
///   one, ATTACK and RELEASE the small.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KnobStyle {
    /// Black bakelite with a white pointer line — the LA-2A / 1176 knob.
    Bakelite,
    /// Brushed metal with a dark pointer.
    Metal,
    /// Generic skirted knob with a rim index mark.
    Skirted,
    /// Daka-Ware phenolic: ridged body on a wider skirt, engraved index line.
    Daka,
    /// Marconi wing knob: coloured body and wing over a skirt.
    Marconi,
    /// SSL collet cap: flat top, fluted rim, white bar.
    Collet,
    /// UREI 1176: black body, brushed silver top, clear collar.
    SilverTop,
    /// dbx and its generation: a brushed aluminium knob with a fluted rim and
    /// a dark centre cap, read by a line across the metal.
    MetalFluted,
    /// Teletronix LA-2A and its contemporaries: a plain black round knob with
    /// a moulded *nose* that points at a scale printed on the panel. No skirt,
    /// no flutes — you read the nose.
    Pointer,
    /// Neve 1073 and its module family: a **geared** collar — bumps coarse
    /// enough to count around the outside, not a knurl — in matte grey, with a
    /// painted white index out at the teeth, around a matte cap carrying a
    /// shorter white line of its own. It sits inside a ring of white dots
    /// printed on the panel (see
    /// [`Ring::Dots`](crate::hardware::rack::Ring::Dots)).
    ///
    /// The teeth are the whole silhouette, so they are a path rather than a
    /// circle with knurl lines drawn over it, and the metal is deliberately
    /// flat: a brushed gradient here reads as chrome and makes a row of these
    /// look plated rather than painted.
    ///
    /// Pairs with `inner_handle` on
    /// [`HardwareKnob`] — on the module the collar and the cap are two
    /// different controls.
    Neve,
    /// Empirical Labs Distressor: a wide brushed dial whose *numerals are
    /// printed on the skirt* and turn with it, around a dark centre cap. The
    /// scale moving rather than a pointer moving is the whole look, and it is
    /// why the panel around a Distressor knob is bare.
    Dial,
}

impl KnobStyle {
    /// The body's diameter as a fraction of the knob's overall size — the rest
    /// is skirt. Daka-Ware's real ratio is 1.125" over 1.5".
    fn body_fraction(self) -> f64 {
        match self {
            Self::Daka => 0.75,
            // The wing knob's coloured body over its darker skirt.
            Self::Marconi => 0.72,
            // The grey cap inside the bright collar. Just under two thirds —
            // the collar is a wide, obvious ring, not a bezel.
            Self::Neve => 0.62,
            // The silver cap is a little over half the knob; the rest is the
            // black collar the index rides on.
            Self::SilverTop => 0.56,
            // The dark cap inside the numbered skirt.
            Self::Dial => 0.58,
            _ => 1.0,
        }
    }

    /// Whether a wider skirt is drawn under the body.
    fn has_skirt(self) -> bool {
        // Neve's collar is a toothed outline, so it is a path rather than a
        // border-radius disc — drawn in its own layer below.
        matches!(
            self,
            Self::Daka | Self::Marconi | Self::SilverTop | Self::Dial
        )
    }

    /// Whether this knob draws its own index — a nose, a wing, a bar, an
    /// engraved dash — rather than taking the generic blade across the face.
    ///
    /// Getting this wrong is invisible in code and obvious in a screenshot: a
    /// Daka-Ware knob drew both its own rim dash *and* the blade, which read
    /// as one long white wedge and was the first thing you noticed.
    fn draws_own_index(self) -> bool {
        matches!(
            self,
            Self::Daka
                | Self::Marconi
                | Self::Pointer
                | Self::Collet
                | Self::SilverTop
                | Self::MetalFluted
                | Self::Neve
                | Self::Dial
        )
    }

    /// How far out the panel's printed scale has to sit for this knob, in the
    /// knob's viewBox units.
    ///
    /// A pointer knob's nose reaches past its body by design — that is how it
    /// points — so a ring drawn for a flush knob lands underneath it.
    pub fn ring_offset(self) -> f64 {
        match self {
            Self::Pointer => 13.0,
            // The wing overhangs the skirt by WING_REACH, so the dots and
            // numerals move out to clear its tip. Only just, though: the
            // knob's viewBox stops at 55, and pushing the scale the full
            // overhang put the numerals outside it, where they were clipped
            // and read as a lopsided ring.
            Self::Marconi => 5.0,
            _ => 0.0,
        }
    }

    /// Whether the printed scale belongs to the knob rather than the panel.
    pub fn numerals_on_knob(self) -> bool {
        matches!(self, Self::Dial)
    }

    /// How the flutes catch the light. Phenolic is dark, so its ridges read as
    /// highlights; a coloured cap's read as shadow between them.
    fn flute_stroke(self) -> &'static str {
        match self {
            Self::Collet => "rgba(0,0,0,0.42)",
            Self::SilverTop => "rgba(0,0,0,0.34)",
            Self::MetalFluted => "rgba(0,0,0,0.38)",
            // A light cap: the flutes read as the shadow between the ribs.
            Self::Neve => "rgba(0,0,0,0.40)",
            Self::Dial => "rgba(0,0,0,0.30)",
            _ => "rgba(255,255,255,0.30)",
        }
    }

    /// Which band of the knob the flutes are cut into, as radii in the knob's
    /// own viewBox: `(inner, outer)`.
    ///
    /// Most knobs are knurled around the edge of the part you grip, so the
    /// band hangs off the body's rim. The 1073's is not: its knurl is on the
    /// bright *collar* outside the grey cap, which is why a band drawn at the
    /// cap's edge left the collar looking like plain metal.
    fn flute_band(self, body_r: f64) -> (f64, f64) {
        match self {
            Self::Neve => (BODY_R * 0.68, BODY_R * 0.97),
            _ => (body_r - 4.2, body_r),
        }
    }

    /// How many flutes are moulded around the grip, if any.
    fn flutes(self) -> usize {
        match self {
            Self::Daka => 44,
            Self::Collet => 28,
            Self::SilverTop => 46,
            Self::MetalFluted => 40,
            // The gear's teeth are the texture. Knurl lines over them read as
            // dirt at panel size.
            Self::Neve => 0,
            Self::Dial => 72,
            Self::Marconi | Self::Pointer => 0,
            _ => 0,
        }
    }
}

impl KnobStyle {
    fn body(self) -> &'static str {
        match self {
            Self::Bakelite => "radial-gradient(circle at 34% 26%, #4a4a4e 0%, #17171a 62%, #0b0b0d 100%)",
            Self::Metal => "radial-gradient(circle at 34% 26%, #d8d8d4 0%, #9a9a96 58%, #6d6d69 100%)",
            // Brushed: a sweep across the face rather than a point highlight.
            Self::MetalFluted => {
                "linear-gradient(152deg, #e8e8e6 0%, #c0c0be 30%, #979795 62%, #d2d2d0 100%)"
            }
            // Concentric ribs, lit from the top left like the photo.
            Self::Skirted => {
                "radial-gradient(circle at 38% 24%, #4c4c50 0%, #232326 38%, #101012 72%, #0a0a0c 100%)"
            }
            // Glossy phenolic, lifted off the matte skirt beneath it.
            Self::Daka => {
                "radial-gradient(circle at 36% 22%, #63636a 0%, #33333a 34%, #17171b 74%, #101014 100%)"
            }
            Self::Marconi => {
                "radial-gradient(circle at 36% 22%, #3c3c40 0%, #1e1e21 46%, #121214 100%)"
            }
            Self::Pointer => {
                "radial-gradient(circle at 34% 24%, #48484d 0%, #232327 44%, #0f0f12 100%)"
            }
            // A collet cap is flat-topped, so it is lit as a face rather than
            // as a sphere.
            Self::Collet => {
                "linear-gradient(162deg, #4a4a4e 0%, #303034 42%, #202024 100%)"
            }
            // Brushed aluminium: a sweep across the top, not a point highlight.
            Self::SilverTop => {
                "linear-gradient(148deg, #e2e2e0 0%, #b4b4b2 34%, #8e8e8c 62%, #cfcfcd 100%)"
            }
            // A matte light-grey cap. Barely any falloff: the module's knobs
            // are painted metal under studio light, not chrome.
            Self::Neve => {
                "linear-gradient(162deg, #b9bdc2 0%, #a5a9af 52%, #8f939a 100%)"
            }
            // The cap in the middle of the dial.
            Self::Dial => {
                "radial-gradient(circle at 38% 28%, #55575c 0%, #303236 46%, #1c1e21 100%)"
            }
        }
    }
    /// The same finish in a given colour — the gradient's shape is what makes
    /// it read as moulded plastic or brushed metal, so only the hue moves.
    fn tinted(self, color: &str) -> String {
        match self {
            Self::Metal => format!(
                "radial-gradient(circle at 34% 26%, color-mix(in oklab, {color} 55%, white) 0%, \
                 {color} 58%, color-mix(in oklab, {color} 70%, black) 100%)"
            ),
            // Matte moulding: an even face with the light only at the very
            // top, so the bright collar around it stays the brighter thing.
            Self::Neve => format!(
                "linear-gradient(160deg, color-mix(in oklab, {color} 88%, white) 0%, \
                 {color} 38%, color-mix(in oklab, {color} 82%, black) 100%)"
            ),
            // Flat-topped: a sheen across the face, not a highlight on a dome.
            Self::Collet => format!(
                "linear-gradient(162deg, color-mix(in oklab, {color} 76%, white) 0%, \
                 {color} 44%, color-mix(in oklab, {color} 72%, black) 100%)"
            ),
            _ => format!(
                "radial-gradient(circle at 36% 24%, color-mix(in oklab, {color} 72%, white) 0%, \
                 {color} 42%, color-mix(in oklab, {color} 62%, black) 100%)"
            ),
        }
    }

    fn pointer(self) -> &'static str {
        match self {
            Self::Bakelite
            | Self::Skirted
            | Self::Daka
            | Self::Marconi
            | Self::Collet
            | Self::Pointer => "#f2f2f0",
            // A dark line on a silver top, which is how you read one.
            Self::Metal | Self::SilverTop | Self::MetalFluted => "#1c1c1e",
            // White, and out at the edge: the module's index is a painted
            // line on the metal, not a groove cut into the face. A dark line
            // on a grey cap disappeared at panel size.
            Self::Neve => "#f4f4f2",
            Self::Dial => "#f2f2f0",
        }
    }
}

/// The Daka-Ware skirt's outline: a ring of coarse rounded lobes rather than a
/// circle, which is what gives the knob its scalloped silhouette and the grip
/// you actually turn it by.
///
/// Drawn as a closed path of quadratic curves — one control point per lobe,
/// pushed out past the rim — so the lobes are round rather than sawtoothed.
fn scallop_path(r: f64, lobes: usize, depth: f64) -> String {
    use std::f64::consts::TAU;
    if lobes < 3 {
        return String::new();
    }
    let point = |a: f64, rr: f64| (rr * a.sin(), -rr * a.cos());
    let inner = r - depth;
    let (sx, sy) = point(0.0, inner);
    let mut d = format!("M {sx:.2} {sy:.2}");
    for i in 0..lobes {
        let a0 = (i as f64 / lobes as f64) * TAU;
        let a1 = ((i + 1) as f64 / lobes as f64) * TAU;
        let mid = (a0 + a1) * 0.5;
        // The control point rides outside the rim, which rounds the lobe.
        let (cx, cy) = point(mid, r + depth * 0.55);
        let (ex, ey) = point(a1, inner);
        d.push_str(&format!(" Q {cx:.2} {cy:.2} {ex:.2} {ey:.2}"));
    }
    d.push_str(" Z");
    d
}

/// The pointer knob's nose: a teardrop reaching past the body, which is what
/// the panel's numbers are read against.
fn nose_points(body_r: f64) -> String {
    let w = body_r * 0.30;
    format!(
        "{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2}",
        -w,
        -(body_r * 0.55),
        w,
        -(body_r * 0.55),
        w * 0.34,
        -(body_r + 9.0),
        -w * 0.34,
        -(body_r + 9.0),
    )
}

/// The Marconi wing, as an SVG polygon in the knob's viewBox: a raised grip
/// that runs across the body and tapers to the indicator end.
fn wing_points(body_r: f64) -> String {
    let w = body_r * 0.46;
    format!(
        "{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2}",
        -w,
        BODY_R * WING_TAIL,
        w,
        BODY_R * WING_TAIL,
        w * 0.64,
        -(BODY_R * WING_REACH),
        -w * 0.64,
        -(BODY_R * WING_REACH),
    )
}

/// The lit flank of a [`wing_points`] moulding: a narrow sliver down its left
/// side, so the wing reads as a raised bar rather than a painted stripe.
fn wing_highlight_points(body_r: f64) -> String {
    let w = body_r * 0.46;
    format!(
        "{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2}",
        -w,
        BODY_R * WING_TAIL,
        -w * 0.55,
        BODY_R * WING_TAIL,
        -w * 0.36,
        -(BODY_R * WING_REACH),
        -w * 0.64,
        -(BODY_R * WING_REACH),
    )
}

/// A knob on a hardware faceplate.
///
/// `marks` is the printed scale ring — build it with
/// [`scale_ring`](crate::hardware::knob_svg::scale_ring).
#[component]
pub fn HardwareKnob(
    handle: ParamHandle,
    /// Stable test id; rendered as `hw-knob-{testid}`.
    testid: String,
    scale: f64,
    /// Knob diameter in design-space px, excluding the printed ring.
    #[props(default = 62.0)]
    diameter: f64,
    #[props(default = KnobStyle::Bakelite)] style: KnobStyle,
    /// Colour of the silkscreened ring numbers.
    #[props(default = "#2b2620".to_string())]
    ink: String,
    #[props(default)] marks: Vec<ScaleMark>,
    /// Radius of the printed tick ring, in the knob's viewBox units.
    #[props(default = RING_R)]
    ring_r: f64,
    /// Radius the numerals are printed at.
    #[props(default = LABEL_R)]
    label_r: f64,
    /// Draw tick marks, or numerals alone.
    #[props(default = true)]
    ticks: bool,
    /// Dots printed evenly along the sweep instead of ticks, and how many.
    ///
    /// The 1073's rings are dots, not dashes, and they are what the panel
    /// reads as before you can make out a number — a small bright arc around
    /// each knob. Zero draws none.
    #[props(default = 0)]
    dots: usize,
    /// Override the knob body's colour.
    ///
    /// A console colour-codes its bands — the SSL's blue LMF, green HMF,
    /// magenta HF — and that colour is how you find the band you want without
    /// reading anything. The [`KnobStyle`] still decides the finish.
    #[props(default)]
    tint: Option<String>,
    /// The *inner* control of a dual-concentric knob.
    ///
    /// A 1073's EQ knobs are two controls in one place: the bright collar
    /// selects the band's frequency, and the grey cap sitting inside it sets
    /// that band's gain. They turn independently, so this draws two indices at
    /// two angles and hands the cap its own drag region — press the collar and
    /// you are on `handle`, press the cap and you are on this one.
    ///
    /// `None` is an ordinary knob, where the whole thing turns together.
    #[props(default)]
    inner_handle: Option<ParamHandle>,
) -> Element {
    let mut drag: Signal<DragState> = use_context();
    // Re-render while a drag is in flight so the pointer tracks the cursor.
    let _ = drag.read().move_count;

    let normalized = handle.normalized() as f64;
    // The collar's angle, and the cap's. They are the same knob unless an
    // inner control was given, in which case the two halves move apart.
    let angle = knob_angle(normalized);
    let inner_angle = match &inner_handle {
        Some(inner) => knob_angle(inner.normalized() as f64),
        None => angle,
    };
    let display = handle.display_value();
    let name = handle.name();

    // The printed ring is drawn outside the body, so the box is wider than
    // the knob — the viewBox spans -55..55 with the body at r = 30.
    let box_px = diameter * (110.0 / (BODY_R * 2.0)) * scale;
    // A skirted knob is read by the index mark on its rim; the others by a
    // pointer across the face.
    let pointer = if style == KnobStyle::Skirted {
        pointer_polygon(BODY_R - 1.0, 2.2)
    } else {
        pointer_polygon(BODY_R - 5.0, 3.4)
    };
    let ring = ring_arc_path(ring_r);
    let body_px = diameter * style.body_fraction() * scale;
    // Body radius in the knob's own viewBox units, so the rotating detail
    // lands on the body rather than on the skirt.
    let body_r = BODY_R * style.body_fraction();

    rsx! {
        div {
            "data-testid": "hw-knob-{testid}",
            "data-normalized": "{normalized:.4}",
            title: format!("{name} — {display}\nDrag · Shift=fine · Wheel · Alt-click=reset"),
            style: format!(
                "position:relative; width:{box_px:.1}px; height:{box_px:.1}px;"
            ),

            svg {
                style: "position:absolute; inset:0; width:100%; height:100%; display:block;",
                view_box: "-55 -55 110 110",

                // The printed scale ring: a faint band with the unit's own
                // numbers around it.
                if ticks {
                    path {
                        d: "{ring}",
                        fill: "none",
                        stroke: "{ink}",
                        stroke_width: "0.6",
                        opacity: "0.35",
                    }
                }
                // The printed dot ring — the 1073's, and the thing you see of
                // one of its knobs before any number resolves. Evenly spaced
                // along the sweep, on the panel and so not turning.
                for i in 0..dots {
                    {
                        let n = if dots > 1 {
                            i as f64 / (dots - 1) as f64
                        } else {
                            0.5
                        };
                        let (dx, dy) = ring_point(n, ring_r);
                        rsx! {
                            circle {
                                cx: "{dx:.2}", cy: "{dy:.2}", r: "1.35",
                                fill: "{ink}",
                                opacity: "0.92",
                            }
                        }
                    }
                }

                // A dial's scale is printed on its own skirt, so it is drawn
                // with the rotating parts below rather than here on the panel.
                for mark in marks.iter().filter(|_| !style.numerals_on_knob()) {
                    {
                        let (x1, y1) = ring_point(mark.normalized, ring_r - 1.0);
                        let (x2, y2) = ring_point(
                            mark.normalized,
                            if mark.major { ring_r + 5.0 } else { ring_r + 3.0 },
                        );
                        let (lx, ly) = ring_point(mark.normalized, label_r);
                        rsx! {
                            if ticks {
                            line {
                                x1: "{x1:.2}", y1: "{y1:.2}", x2: "{x2:.2}", y2: "{y2:.2}",
                                stroke: "{ink}",
                                stroke_width: if mark.major { "1.4" } else { "0.8" },
                                opacity: if mark.major { "0.9" } else { "0.55" },
                            }
                            }
                            if let Some(label) = &mark.label {
                                text {
                                    x: "{lx:.2}", y: "{ly + 2.6:.2}",
                                    fill: "{ink}", font_size: "7",
                                    font_weight: "700", text_anchor: "middle",
                                    "{label}"
                                }
                            }
                        }
                    }
                }

                // Body shadow — the knob sits proud of the panel.
                circle {
                    cx: "0", cy: "1.5", r: "{BODY_R:.1}",
                    fill: "rgba(0,0,0,0.35)",
                }
            }

            // Daka-Ware draws itself: a lobed skirt is a path, not a border
            // radius, and the tiers above it are concentric discs rather than
            // one gradient. Everything here turns with the knob.
            if style == KnobStyle::Daka {
                svg {
                    style: "position:absolute; inset:0; width:100%; height:100%; display:block;",
                    view_box: "-55 -55 110 110",
                    // Everything in here turns. The light does not — it is
                    // drawn after this group, not inside it.
                    g {
                        transform: "rotate({angle:.2})",
                        // Contact shadow: the same outline, dropped.
                        path {
                            d: "{scallop_path(BODY_R, DAKA_LOBES, DAKA_LOBE_DEPTH)}",
                            transform: "translate(0 1.8)",
                            fill: "rgba(0,0,0,0.50)",
                        }
                        // The scalloped skirt — the widest tier, and the one
                        // you actually grip.
                        path {
                            d: "{scallop_path(BODY_R, DAKA_LOBES, DAKA_LOBE_DEPTH)}",
                            fill: "#131316",
                            stroke: "rgba(0,0,0,0.7)",
                            stroke_width: "0.7",
                        }
                        // The step up to the body, read as a shadowed wall
                        // rather than a drawn edge.
                        circle {
                            cx: "0", cy: "0", r: "{BODY_R * 0.74:.1}",
                            fill: "#0d0d10",
                        }
                        // The body: a raised cylinder, lighter than the skirt.
                        circle {
                            cx: "0", cy: "0", r: "{BODY_R * 0.70:.1}",
                            fill: "#242429",
                        }
                        // Fine ridging around the body's wall — the knurl the
                        // photograph reads as a bright ring at this size.
                        for i in 0..DAKA_BODY_RIDGES {
                            {
                                let a = (i as f64 / DAKA_BODY_RIDGES as f64)
                                    * std::f64::consts::TAU;
                                let (sx, sy) = (a.sin(), -a.cos());
                                let inner = BODY_R * 0.60;
                                let outer = BODY_R * 0.70;
                                rsx! {
                                    line {
                                        x1: "{sx * inner:.2}", y1: "{sy * inner:.2}",
                                        x2: "{sx * outer:.2}", y2: "{sy * outer:.2}",
                                        stroke: "rgba(255,255,255,0.10)",
                                        stroke_width: "0.7",
                                    }
                                }
                            }
                        }
                        // The domed top the index is moulded into.
                        circle { cx: "0", cy: "0", r: "{BODY_R * 0.52:.1}", fill: "#2b2b31" }
                        circle { cx: "0", cy: "0", r: "{BODY_R * 0.30:.1}", fill: "#313138" }
                        // The index: engraved from the dome out across the
                        // body to the skirt's edge and filled white. It is a
                        // long line on this knob, not a dash on the rim — the
                        // one thing you read the unit's settings by.
                        rect {
                            x: "-1.2",
                            y: "{-(BODY_R - 1.5):.1}",
                            width: "2.4",
                            height: "{BODY_R * 0.66:.1}",
                            rx: "1.2",
                            fill: "#eceae4",
                        }
                    }

                    // The panel's light, caught on the upper left. Outside the
                    // rotating group on purpose: a reflection that turns with
                    // the knob is the first thing that reads as wrong, because
                    // the lamp above the rack does not move when you turn a
                    // control.
                    ellipse {
                        // Named so a test can assert it is NOT inside the
                        // rotating group — the bug this replaced was invisible
                        // in code and obvious the moment you turned a knob.
                        "data-testid": "hw-knob-{testid}-light",
                        cx: "{-BODY_R * 0.22:.1}", cy: "{-BODY_R * 0.30:.1}",
                        rx: "{BODY_R * 0.46:.1}", ry: "{BODY_R * 0.30:.1}",
                        transform: "rotate(-32)",
                        fill: "rgba(255,255,255,0.07)",
                    }
                    // A hairline of the same light along the top rim.
                    path {
                        d: "M {-BODY_R * 0.72:.2} {-BODY_R * 0.58:.2} \
                            A {BODY_R * 0.93:.2} {BODY_R * 0.93:.2} 0 0 1 \
                            {BODY_R * 0.40:.2} {-BODY_R * 0.84:.2}",
                        fill: "none",
                        stroke: "rgba(255,255,255,0.12)",
                        stroke_width: "1.0",
                    }
                }
            }

            // The 1073's collar: a geared ring, not a disc.
            //
            // The teeth are the silhouette — bumps coarse enough to count
            // around the outside — so the collar is a path rather than a
            // border-radius circle with knurl lines drawn over it. Flat fills
            // in a mid grey, because these are painted metal on a console
            // under studio light: a bright brushed gradient here read as
            // chrome and made the whole row look plated.
            //
            // Everything in the rotating group belongs to the *outer*
            // control, which on a concentric band is the frequency.
            if style == KnobStyle::Neve {
                svg {
                    style: "position:absolute; inset:0; width:100%; height:100%; display:block;",
                    view_box: "-55 -55 110 110",
                    g {
                        transform: "rotate({angle:.2})",
                        path {
                            d: "{scallop_path(BODY_R, NEVE_TEETH, NEVE_TOOTH_DEPTH)}",
                            transform: "translate(0 1.6)",
                            fill: "rgba(0,0,0,0.45)",
                        }
                        path {
                            d: "{scallop_path(BODY_R, NEVE_TEETH, NEVE_TOOTH_DEPTH)}",
                            fill: "#b4b9be",
                            stroke: "rgba(0,0,0,0.45)",
                            stroke_width: "0.7",
                        }
                        // The flat of the ring inside the teeth.
                        circle {
                            cx: "0", cy: "0", r: "{BODY_R - NEVE_TOOTH_DEPTH - 0.8:.1}",
                            fill: "#c2c7cc",
                        }
                        // The index: a painted white line out at the teeth,
                        // which is where the module's is and what the ring of
                        // dots is read against.
                        rect {
                            x: "-1.6",
                            y: "{-(BODY_R + 0.6):.1}",
                            width: "3.2",
                            height: "{BODY_R * 0.34:.1}",
                            rx: "1.0",
                            fill: "#f4f4f2",
                        }
                    }
                }
            }

            // The skirt: the wider disc a Marconi or 1176 body sits on.
            // Drawn first so the body stacks over it.
            if style.has_skirt() && style != KnobStyle::Daka {
                div {
                    style: format!(
                        "position:absolute; left:50%; top:50%; \
                         width:{:.1}px; height:{:.1}px; \
                         margin-left:{:.1}px; margin-top:{:.1}px; \
                         border-radius:50%; \
                         background:{}; \
                         box-shadow:0 {:.1}px {:.1}px rgba(0,0,0,0.5), \
                           inset 0 0 {:.1}px rgba(255,255,255,0.10);",
                        diameter * scale,
                        diameter * scale,
                        -(diameter * scale) / 2.0,
                        -(diameter * scale) / 2.0,
                        if false {
                            ""
                        } else if style == KnobStyle::SilverTop {
                            // The collar: matte black, lit from the top left,
                            // with a dark rim.
                            "radial-gradient(circle at 38% 28%, #35353a 0%, #1c1c20 46%, \
                             #101013 78%, #0a0a0c 100%)"
                        } else if style == KnobStyle::Neve {
                            // The bright turned-aluminium collar. On the unit
                            // this is the concentric outer knob; here it is
                            // the ring that makes a 1073 knob recognisable
                            // before any colour or number does.
                            "linear-gradient(150deg, #f6f7f8 0%, #d6dade 30%, \
                             #aeb3b8 60%, #eceef0 86%, #c2c6ca 100%)"
                        } else if style == KnobStyle::Dial {
                            "radial-gradient(circle at 40% 26%, #e8e8e6 0%, #c2c2c0 44%, \
                             #9a9a98 78%, #cbcbc9 100%)"
                        } else {
                            "radial-gradient(circle at 44% 36%, #202024 0%, #101013 58%, #08080a 100%)"
                        },
                        1.5 * scale,
                        4.0 * scale,
                        2.0 * scale,
                    ),
                }
            }

            // The knob body itself is a div so it can carry a CSS gradient —
            // the moulded-plastic look does not survive as flat SVG fill.
            // (Daka-Ware is the exception, drawn above.)
            //
            // Four shadows do the work of making it an object rather than a
            // circle: a cast shadow on the panel, a dark inner rim at the
            // bottom, a light inner rim at the top, and a hairline edge. A
            // knob is a cylinder seen from above, and that is mostly what you
            // read at the rim.
            if style != KnobStyle::Daka {
            div {
                style: format!(
                    "position:absolute; left:50%; top:50%; \
                     width:{:.1}px; height:{:.1}px; \
                     margin-left:{:.1}px; margin-top:{:.1}px; \
                     border-radius:50%; background:{}; \
                     border:{:.1}px solid rgba(0,0,0,0.45); \
                     box-shadow:0 {:.1}px {:.1}px rgba(0,0,0,0.55), \
                       0 {:.1}px {:.1}px rgba(0,0,0,0.30), \
                       inset 0 {:.1}px {:.1}px rgba(0,0,0,0.45), \
                       inset 0 {:.1}px {:.1}px rgba(255,255,255,0.16);",
                    body_px,
                    body_px,
                    -body_px / 2.0,
                    -body_px / 2.0,
                    tint.as_deref().map(|c| style.tinted(c)).unwrap_or_else(|| style.body().to_string()),
                    (0.5 * scale).max(0.6),
                    1.5 * scale,
                    3.0 * scale,
                    4.0 * scale,
                    10.0 * scale,
                    -body_px * 0.10,
                    body_px * 0.16,
                    body_px * 0.05,
                    body_px * 0.10,
                ),
            }
            }

            if style != KnobStyle::Daka {
            // Specular: the soft highlight a moulded or turned surface takes
            // from the light every panel is lit by. Offset up and left, and
            // small — a big one reads as gloss paint rather than plastic.
            div {
                style: format!(
                    "position:absolute; left:50%; top:50%; \
                     width:{:.1}px; height:{:.1}px; \
                     margin-left:{:.1}px; margin-top:{:.1}px; \
                     border-radius:50%; pointer-events:none; \
                     background:{};",
                    body_px * 0.62,
                    body_px * 0.42,
                    -body_px * 0.46,
                    -body_px * 0.40,
                    if style == KnobStyle::Collet {
                        // Flat top: a sheen across it rather than a highlight
                        // sitting on a dome.
                        "linear-gradient(150deg, rgba(255,255,255,0.20) 0%, \
                         rgba(255,255,255,0.04) 46%, rgba(255,255,255,0.0) 72%)"
                    } else {
                        "radial-gradient(ellipse at 50% 50%, rgba(255,255,255,0.15) 0%, \
                         rgba(255,255,255,0.0) 70%)"
                    },
                ),
            }
            }

            // Pointer, rotated to the value. Kept in its own SVG layer above
            // the body so the rotation is exact at any panel scale.
            svg {
                style: "position:absolute; inset:0; width:100%; height:100%; \
                        display:block; pointer-events:none;",
                view_box: "-55 -55 110 110",
                // The collar's own layer. Its knurl belongs to the outer
                // control, so on a concentric knob it turns with the collar
                // while the cap's index stays where the inner control put it.
                g {
                    transform: "rotate({angle:.2})",

                    // A dial's numerals: printed around the skirt, turning
                    // with it. The reading is where they line up with the
                    // panel's index, not where a pointer lands.
                    if style.numerals_on_knob() {
                        for mark in marks.iter() {
                            {
                                let (lx, ly) = ring_point(mark.normalized, BODY_R - 6.0);
                                rsx! {
                                    if let Some(label) = &mark.label {
                                        text {
                                            x: "{lx:.2}", y: "{ly + 2.2:.2}",
                                            fill: "#1a1c1f", font_size: "6.5",
                                            font_weight: "700", text_anchor: "middle",
                                            "{label}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Moulded flutes around the grip. They turn with the knob,
                    // which is most of what tells you it moved at a glance.
                    for i in 0..(if style == KnobStyle::Daka { 0 } else { style.flutes() }) {
                        {
                            let count = style.flutes() as f64;
                            let a = (i as f64 / count) * std::f64::consts::TAU;
                            // Half a flute over, for the shadowed side.
                            let b = a + std::f64::consts::TAU / (count * 2.0);
                            let (sx, sy) = (a.sin(), -a.cos());
                            let (bx, by) = (b.sin(), -b.cos());
                            let (inner, outer) = style.flute_band(body_r);
                            rsx! {
                                line {
                                    x1: "{sx * inner:.2}",
                                    y1: "{sy * inner:.2}",
                                    x2: "{sx * outer:.2}",
                                    y2: "{sy * outer:.2}",
                                    stroke: "{style.flute_stroke()}",
                                    stroke_width: "1.2",
                                }
                                line {
                                    x1: "{bx * inner:.2}",
                                    y1: "{by * inner:.2}",
                                    x2: "{bx * outer:.2}",
                                    y2: "{by * outer:.2}",
                                    stroke: "rgba(0,0,0,0.40)",
                                    stroke_width: "1.2",
                                }
                            }
                        }
                    }
                }

                // The cap's layer: every index this knob draws. On an
                // ordinary knob it turns with the collar above; on a
                // concentric one it is the inner control.
                g {
                    transform: "rotate({inner_angle:.2})",

                    // The Marconi wing: the raised coloured grip you turn,
                    // reaching past the body toward the skirt's edge. It is
                    // the *silhouette* of a 1073's gain and filter knobs —
                    // a bar of colour standing off a dark disc, not a line
                    // drawn on a coloured face — so it is filled in the
                    // knob's own colour and lit down one side.
                    if style == KnobStyle::Marconi {
                        polygon {
                            points: "{wing_points(body_r)}",
                            transform: "translate(0.8 1.6)",
                            fill: "rgba(0,0,0,0.45)",
                        }
                        polygon {
                            points: "{wing_points(body_r)}",
                            fill: "{tint.as_deref().unwrap_or(\"#3a3a40\")}",
                            stroke: "rgba(0,0,0,0.6)",
                            stroke_width: "0.9",
                        }
                        // The lit face of the moulding, down its left flank.
                        polygon {
                            points: "{wing_highlight_points(body_r)}",
                            fill: "rgba(255,255,255,0.20)",
                        }
                        rect {
                            x: "-1.3",
                            y: "{-(BODY_R - 2.0):.1}",
                            width: "2.6",
                            height: "{BODY_R * 0.62:.1}",
                            rx: "1.0",
                            fill: "{style.pointer()}",
                        }
                    }

                    // Daka-Ware's index is engraved into the body and filled
                    // white — it runs from the hub out to the body's edge, not
                    // a mark perched on the rim.
                    // The moulded nose: it *is* the pointer, so it reaches past
                    // the body toward the panel's printed scale.
                    if style == KnobStyle::Pointer {
                        polygon {
                            points: "{nose_points(BODY_R)}",
                            fill: "{style.body()}",
                            stroke: "rgba(0,0,0,0.55)",
                            stroke_width: "0.8",
                        }
                        rect {
                            x: "-1.2",
                            y: "{-(BODY_R + 7.0):.1}",
                            width: "2.4",
                            height: "{BODY_R * 0.55:.1}",
                            rx: "1.0",
                            fill: "rgba(255,255,255,0.85)",
                        }
                    }

                    // A dark cap in the middle of the metal, with the index
                    // running from it out across the brushed face.
                    if style == KnobStyle::MetalFluted {
                        circle {
                            cx: "0", cy: "0", r: "{BODY_R * 0.42:.1}",
                            fill: "#3a3c40",
                            stroke: "rgba(0,0,0,0.5)", stroke_width: "0.8",
                        }
                        rect {
                            x: "-1.3",
                            y: "{-(BODY_R - 2.0):.1}",
                            width: "2.6",
                            height: "{BODY_R * 0.78:.1}",
                            rx: "1.0",
                            fill: "#f2f2f0",
                        }
                    }

                    if style == KnobStyle::SilverTop {
                        // Fine knurling around the cap's edge.
                        for i in 0..54 {
                            {
                                let a = (i as f64 / 54.0) * std::f64::consts::TAU;
                                let (sx, sy) = (a.sin(), -a.cos());
                                rsx! {
                                    line {
                                        x1: "{sx * (body_r - 2.4):.2}",
                                        y1: "{sy * (body_r - 2.4):.2}",
                                        x2: "{sx * body_r:.2}",
                                        y2: "{sy * body_r:.2}",
                                        stroke: "rgba(0,0,0,0.34)",
                                        stroke_width: "0.8",
                                    }
                                }
                            }
                        }
                        // Two turned rings on the brushed face.
                        circle {
                            cx: "0", cy: "0", r: "{body_r * 0.62:.1}",
                            fill: "none", stroke: "rgba(0,0,0,0.16)", stroke_width: "0.7",
                        }
                        circle {
                            cx: "0", cy: "0", r: "{body_r * 0.30:.1}",
                            fill: "none", stroke: "rgba(0,0,0,0.13)", stroke_width: "0.6",
                        }
                        // The index: on the collar, from just outside the cap
                        // to just inside the rim.
                        rect {
                            x: "-1.5",
                            y: "{-(BODY_R - 2.5):.1}",
                            width: "3.0",
                            height: "{BODY_R - body_r - 3.5:.1}",
                            rx: "1.0",
                            fill: "#f4f4f2",
                        }
                    }

                    if false {
                        rect {
                            x: "-1.2",
                            y: "{-(body_r - 1.5):.1}",
                            width: "2.4",
                            height: "{body_r - 5.0:.1}",
                            rx: "1.0",
                            fill: "{style.pointer()}",
                        }
                    }

                    // The 1073's band knob: a dark groove cut across the grey
                    // cap, read against the collar and the printed dots. It
                    // stays inside the cap — a line that ran onto the bright
                    // collar would vanish into it.
                    if style == KnobStyle::Neve {
                        // The seam where the cap meets the collar.
                        circle {
                            cx: "0", cy: "0", r: "{body_r:.1}",
                            fill: "none",
                            stroke: "rgba(0,0,0,0.45)",
                            stroke_width: "1.0",
                        }
                        // The cap's own index, for the inner control. Kept
                        // shorter than the collar's so the two readings do
                        // not compete — the gear's line is the loud one.
                        rect {
                            x: "-1.3",
                            y: "{-(body_r - 2.0):.1}",
                            width: "2.6",
                            height: "{body_r * 0.52:.1}",
                            rx: "1.1",
                            fill: "{style.pointer()}",
                        }
                    }

                    // A collet cap is read by one bar across its flat top.
                    if style == KnobStyle::Collet {
                        rect {
                            x: "-1.8",
                            y: "{-(body_r - 3.0):.1}",
                            width: "3.6",
                            height: "{body_r - 6.0:.1}",
                            rx: "1.2",
                            fill: "{style.pointer()}",
                        }
                    }

                    if style == KnobStyle::Skirted {
                        // A short bar on the skirt's rim — the vintage
                        // outboard index, which is what the printed numerals
                        // are read against.
                        rect {
                            x: "-1.9",
                            y: "{-(BODY_R - 1.0):.1}",
                            width: "3.8",
                            height: "{BODY_R * 0.34:.1}",
                            rx: "1.2",
                            fill: "{style.pointer()}",
                        }
                    } else if !style.draws_own_index() {
                        polygon {
                            points: "{pointer}",
                            fill: "{style.pointer()}",
                        }
                    }
                }
                if style == KnobStyle::Skirted {
                    // The smooth cap inside the fluted skirt.
                    circle {
                        cx: "0", cy: "0", r: "{BODY_R * 0.62:.1}",
                        fill: "rgba(255,255,255,0.035)",
                        stroke: "rgba(0,0,0,0.45)",
                        stroke_width: "0.8",
                    }
                } else {
                    circle { cx: "0", cy: "0", r: "4.5", fill: "rgba(0,0,0,0.35)" }
                }
            }

            // The panel's index, which a dial's moving numerals are read
            // against. Fixed, unlike everything else on the knob.
            if style.numerals_on_knob() {
                svg {
                    style: "position:absolute; inset:0; width:100%; height:100%; \
                            display:block; pointer-events:none;",
                    view_box: "-55 -55 110 110",
                    polygon {
                        points: "0,{-(BODY_R + 6.0):.1} -3.2,{-(BODY_R + 12.0):.1} 3.2,{-(BODY_R + 12.0):.1}",
                        fill: "#e8eaec",
                    }
                }
            }

            // Interaction overlay — same gestures as every FTS control.
            div {
                style: "position:absolute; inset:0; cursor:ns-resize; user-select:none;",
                onmousedown: {
                    let handle = handle.clone();
                    move |evt: MouseEvent| {
                        if evt.modifiers().alt() {
                            evt.prevent_default();
                            handle.reset_to_default();
                            return;
                        }
                        begin_drag(
                            &mut drag,
                            handle.clone(),
                            evt.client_coordinates().y,
                            SENSITIVITY,
                        );
                    }
                },
                onwheel: {
                    let handle = handle.clone();
                    move |evt: WheelEvent| {
                        evt.prevent_default();
                        let delta_y = evt.delta().strip_units().y;
                        if delta_y == 0.0 {
                            return;
                        }
                        let direction = if delta_y < 0.0 { 1.0 } else { -1.0 };
                        let mods = evt.modifiers();
                        let step = if mods.shift() { WHEEL_STEP_FINE } else { WHEEL_STEP };
                        let next = (handle.normalized() as f64 + direction * step)
                            .clamp(0.0, 1.0) as f32;
                        handle.begin_edit();
                        handle.set_normalized(next);
                        handle.end_edit();
                    }
                },
            }

            // The cap's own drag region, on a dual-concentric knob.
            //
            // Laid over the collar's overlay and sized to the cap, so the two
            // controls are hit exactly where they are drawn: press the grey
            // middle and you have the inner control, press the bright ring
            // around it and the press falls through to the outer one. That is
            // how the real thing is operated, and it needs no modifier key.
            if let Some(inner) = &inner_handle {
                div {
                    "data-testid": "hw-knob-{testid}-inner",
                    style: format!(
                        "position:absolute; left:50%; top:50%; \
                         width:{0:.1}px; height:{0:.1}px; \
                         margin-left:{1:.1}px; margin-top:{1:.1}px; \
                         border-radius:50%; cursor:ns-resize; user-select:none;",
                        body_px,
                        -body_px / 2.0,
                    ),
                    onmousedown: {
                        let inner = inner.clone();
                        move |evt: MouseEvent| {
                            evt.stop_propagation();
                            if evt.modifiers().alt() {
                                evt.prevent_default();
                                inner.reset_to_default();
                                return;
                            }
                            begin_drag(
                                &mut drag,
                                inner.clone(),
                                evt.client_coordinates().y,
                                SENSITIVITY,
                            );
                        }
                    },
                    onwheel: {
                        let inner = inner.clone();
                        move |evt: WheelEvent| {
                            evt.prevent_default();
                            evt.stop_propagation();
                            let delta_y = evt.delta().strip_units().y;
                            if delta_y == 0.0 {
                                return;
                            }
                            let direction = if delta_y < 0.0 { 1.0 } else { -1.0 };
                            let step = if evt.modifiers().shift() {
                                WHEEL_STEP_FINE
                            } else {
                                WHEEL_STEP
                            };
                            let next = (inner.normalized() as f64 + direction * step)
                                .clamp(0.0, 1.0) as f32;
                            inner.begin_edit();
                            inner.set_normalized(next);
                            inner.end_edit();
                        }
                    },
                }
            }
        }
    }
}
