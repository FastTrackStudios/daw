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
/// - **Silver-top** (UREI 1176): a black plastic body with a *brushed silver
///   top* and a clear plastic collar around its base — the collar catches the
///   panel light, which is why an 1176's knobs read as rings from across a
///   room. INPUT and OUTPUT take the large one, ATTACK and RELEASE the small.
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
    /// Teletronix LA-2A and its contemporaries: a plain black round knob with
    /// a moulded *nose* that points at a scale printed on the panel. No skirt,
    /// no flutes — you read the nose.
    Pointer,
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
            Self::Marconi => 0.70,
            // The clear collar is a visible ring around the body, not a hair
            // of trim: on the unit it is most of what you see of the knob.
            Self::SilverTop => 0.72,
            // The dark cap inside the numbered skirt.
            Self::Dial => 0.58,
            _ => 1.0,
        }
    }

    /// Whether a wider skirt is drawn under the body.
    fn has_skirt(self) -> bool {
        matches!(self, Self::Daka | Self::Marconi | Self::SilverTop | Self::Dial)
    }

    /// How far out the panel's printed scale has to sit for this knob, in the
    /// knob's viewBox units.
    ///
    /// A pointer knob's nose reaches past its body by design — that is how it
    /// points — so a ring drawn for a flush knob lands underneath it.
    pub fn ring_offset(self) -> f64 {
        match self {
            Self::Pointer => 13.0,
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
            Self::Dial => "rgba(0,0,0,0.30)",
            _ => "rgba(255,255,255,0.30)",
        }
    }

    /// How many flutes are moulded around the grip, if any.
    fn flutes(self) -> usize {
        match self {
            Self::Daka => 44,
            Self::Collet => 28,
            Self::SilverTop => 30,
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
            Self::Metal | Self::SilverTop => "#1c1c1e",
            Self::Dial => "#f2f2f0",
        }
    }
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
    let w = body_r * 0.34;
    format!(
        "{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2}",
        -w,
        body_r * 0.82,
        w,
        body_r * 0.82,
        w * 0.42,
        -(body_r * 1.28),
        -w * 0.42,
        -(body_r * 1.28),
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
    /// Override the knob body's colour.
    ///
    /// A console colour-codes its bands — the SSL's blue LMF, green HMF,
    /// magenta HF — and that colour is how you find the band you want without
    /// reading anything. The [`KnobStyle`] still decides the finish.
    #[props(default)]
    tint: Option<String>,
) -> Element {
    let mut drag: Signal<DragState> = use_context();
    // Re-render while a drag is in flight so the pointer tracks the cursor.
    let _ = drag.read().move_count;

    let normalized = handle.normalized() as f64;
    let angle = knob_angle(normalized);
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

            // The skirt: the wider disc a Daka-Ware or Marconi body sits on.
            // Drawn first so the body stacks over it.
            if style.has_skirt() {
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
                        if style == KnobStyle::SilverTop {
                            // Clear plastic: it takes the panel's light rather
                            // than absorbing it.
                            "radial-gradient(circle at 44% 34%, rgba(226,232,238,0.34) 0%, \
                             rgba(150,164,176,0.20) 56%, rgba(20,22,26,0.55) 100%)"
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
            //
            // Four shadows do the work of making it an object rather than a
            // circle: a cast shadow on the panel, a dark inner rim at the
            // bottom, a light inner rim at the top, and a hairline edge. A
            // knob is a cylinder seen from above, and that is mostly what you
            // read at the rim.
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

            // Pointer, rotated to the value. Kept in its own SVG layer above
            // the body so the rotation is exact at any panel scale.
            svg {
                style: "position:absolute; inset:0; width:100%; height:100%; \
                        display:block; pointer-events:none;",
                view_box: "-55 -55 110 110",
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
                    for i in 0..style.flutes() {
                        {
                            let count = style.flutes() as f64;
                            let a = (i as f64 / count) * std::f64::consts::TAU;
                            // Half a flute over, for the shadowed side.
                            let b = a + std::f64::consts::TAU / (count * 2.0);
                            let (sx, sy) = (a.sin(), -a.cos());
                            let (bx, by) = (b.sin(), -b.cos());
                            let inner = body_r - 4.2;
                            rsx! {
                                line {
                                    x1: "{sx * inner:.2}",
                                    y1: "{sy * inner:.2}",
                                    x2: "{sx * body_r:.2}",
                                    y2: "{sy * body_r:.2}",
                                    stroke: "{style.flute_stroke()}",
                                    stroke_width: "1.2",
                                }
                                line {
                                    x1: "{bx * inner:.2}",
                                    y1: "{by * inner:.2}",
                                    x2: "{bx * body_r:.2}",
                                    y2: "{by * body_r:.2}",
                                    stroke: "rgba(0,0,0,0.40)",
                                    stroke_width: "1.2",
                                }
                            }
                        }
                    }

                    // The Marconi wing: the raised grip you actually turn,
                    // reaching past the body toward the skirt's edge.
                    if style == KnobStyle::Marconi {
                        polygon {
                            points: "{wing_points(body_r)}",
                            fill: "rgba(255,255,255,0.09)",
                            stroke: "rgba(0,0,0,0.55)",
                            stroke_width: "0.9",
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

                    if style == KnobStyle::SilverTop {
                        rect {
                            x: "-1.3",
                            y: "{-(body_r - 1.5):.1}",
                            width: "2.6",
                            height: "{body_r - 4.0:.1}",
                            rx: "1.0",
                            fill: "{style.pointer()}",
                        }
                    }

                    if style == KnobStyle::Daka {
                        rect {
                            x: "-1.2",
                            y: "{-(body_r - 1.5):.1}",
                            width: "2.4",
                            height: "{body_r - 5.0:.1}",
                            rx: "1.0",
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
                    } else {
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
        }
    }
}
