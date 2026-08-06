//! The VU movement — the thing at the centre of both hardware faces.
//!
//! Geometry (the arc, the crowded scale, the needle) comes from
//! [`crate::hardware::vu_svg`]; this is the drawn face over it: the lamp
//! colour, the bezel, the printed scale and the legend. The LA-2A's is warm
//! and amber-lit; the 1176's is the blue-lit UREI face.

use dioxus::prelude::*;

use crate::hardware::vu_svg::{
    scale_arc_for, scale_needle_tip, scale_point, VuScale, PIVOT_X, PIVOT_Y, VU_H, VU_W,
};

/// What the needle is reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VuMode {
    /// Gain reduction — rests at 0 on the right and swings left as the
    /// compressor works, like the hardware.
    GainReduction,
    /// Output level, with -18 dBFS aligned to 0 VU.
    Level,
}

/// The face's colour scheme.
///
/// A period VU movement — Weston, Modutec, Sifam — has an **ivory card with a
/// black scale and a red stretch above 0**, lit from behind by one or two
/// bulbs. The lamp is what varies between units, not the card: the LA-2A's is
/// warm, a rackmount's is whiter. Blue-faced meters are largely a plugin
/// aesthetic rather than something these units wore, so the default is ivory
/// and blue is kept only for a unit that genuinely has one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VuFace {
    /// Warm ivory under a yellow lamp — the LA-2A's, and most tube gear's.
    Amber,
    /// Neutral ivory under a white lamp — the 1176's Modutec, an SSL.
    Ivory,
    /// Amber card printed in *blue* — the dbx's, which is not a VU at all but
    /// a decibel readout, and prints like one.
    AmberBlue,
    /// Blue-lit card, white printing.
    Blue,
}

impl VuFace {
    fn card(self) -> &'static str {
        match self {
            Self::Amber => "linear-gradient(180deg, #f6d79a 0%, #e8b45f 62%, #d99a3c 100%)",
            Self::Ivory => "linear-gradient(180deg, #f4f2ea 0%, #ddd9cd 100%)",
            Self::AmberBlue => "linear-gradient(180deg, #f7cf86 0%, #eab259 58%, #d99b3f 100%)",
            Self::Blue => "linear-gradient(180deg, #2f5f8f 0%, #16324e 100%)",
        }
    }
    fn ink(self) -> &'static str {
        match self {
            Self::Amber => "#3a2a12",
            Self::Ivory => "#2a241c",
            Self::AmberBlue => "#1c4f96",
            Self::Blue => "#e8f2ff",
        }
    }
    fn needle(self) -> &'static str {
        match self {
            Self::Amber => "#241a0c",
            Self::Ivory => "#1d1a15",
            Self::AmberBlue => "#14161a",
            Self::Blue => "#f4f8ff",
        }
    }
    /// The lamp glow across the top of the card.
    fn lamp(self) -> &'static str {
        match self {
            Self::Amber => "rgba(255,214,120,0.55)",
            Self::Ivory => "rgba(255,246,224,0.30)",
            Self::AmberBlue => "rgba(255,216,126,0.50)",
            Self::Blue => "rgba(150,205,255,0.32)",
        }
    }
    /// Colour of the over-zero part of the scale (red on every VU ever made).
    fn hot(self) -> &'static str {
        match self {
            Self::Amber => "#8f2010",
            Self::Ivory => "#a8281c",
            Self::AmberBlue => "#1c4f96",
            Self::Blue => "#ff6a5c",
        }
    }
}

/// A VU meter drawn at panel scale.
///
/// `value_db` is gain reduction in dB (positive) in
/// [`VuMode::GainReduction`], or a level in dBFS in [`VuMode::Level`].
#[component]
pub fn VuMeter(
    scale: f64,
    /// Width of the meter in design-space px; height follows the face aspect.
    #[props(default = 190.0)]
    width: f64,
    face: VuFace,
    mode: VuMode,
    value_db: f32,
    #[props(default = "VU".to_string())] legend: String,
    /// Wrap the movement in a black bezel with a vent below it, as a meter
    /// mounted through a panel rather than printed on one.
    #[props(default = false)]
    bezel: bool,
    /// What the card is printed with — a VU standard or a plain decibel scale.
    #[props(default)]
    card: VuScale,
) -> Element {
    let value = match mode {
        VuMode::GainReduction => card.from_gain_reduction_db(value_db as f64),
        VuMode::Level => card.from_level_db(value_db as f64),
    };
    let (nx, ny) = scale_needle_tip(card, value);
    let arc = scale_arc_for(card, 6.0);

    let w = width * scale;
    let h = width * (VU_H / VU_W) * scale;

    if bezel {
        return rsx! {
            div {
                "data-testid": "vu-bezel",
                style: format!(
                    "display:flex; flex-direction:column; align-items:center; \
                     padding:{:.1}px {:.1}px {:.1}px; border-radius:{:.1}px; \
                     background:linear-gradient(180deg, #1a1b1d, #0c0d0e); \
                     border:{:.1}px solid rgba(0,0,0,0.7); \
                     box-shadow:inset 0 {:.1}px {:.1}px rgba(255,255,255,0.06), \
                       0 {:.1}px {:.1}px rgba(0,0,0,0.5);",
                    9.0 * scale,
                    9.0 * scale,
                    6.0 * scale,
                    5.0 * scale,
                    (1.0 * scale).max(1.0),
                    1.0 * scale,
                    2.0 * scale,
                    2.0 * scale,
                    6.0 * scale,
                ),
                VuMeter { scale, width, face, mode, value_db, legend, card }
                // The vent under the glass, which is most of what says the
                // movement is mounted through the panel.
                div {
                    style: format!(
                        "margin-top:{:.1}px; width:{:.1}px; height:{:.1}px; \
                         border-radius:{:.1}px; \
                         background:repeating-linear-gradient(90deg, \
                           #000 0 {:.1}px, #2a2c2e {:.1}px {:.1}px);",
                        5.0 * scale,
                        width * 0.30 * scale,
                        4.0 * scale,
                        1.0 * scale,
                        2.0 * scale,
                        2.0 * scale,
                        4.0 * scale,
                    ),
                }
            }
        };
    }

    rsx! {
        div {
            "data-testid": "vu-meter",
            "data-vu": "{value:.2}",
            style: format!(
                "position:relative; width:{w:.1}px; height:{h:.1}px; \
                 background:{}; border-radius:{:.1}px; overflow:hidden; \
                 border:{:.1}px solid rgba(0,0,0,0.55); \
                 box-shadow:inset 0 0 {:.1}px rgba(0,0,0,0.45);",
                face.card(),
                3.0 * scale,
                (1.5 * scale).max(1.0),
                10.0 * scale,
            ),

            // Lamp wash — the meter is lit from behind the top of the card.
            div {
                style: format!(
                    "position:absolute; inset:0; background:radial-gradient(\
                     ellipse at 50% 8%, {} 0%, rgba(0,0,0,0) 68%); \
                     pointer-events:none;",
                    face.lamp(),
                ),
            }

            svg {
                style: "position:absolute; inset:0; width:100%; height:100%; display:block;",
                view_box: "0 0 {VU_W} {VU_H}",
                preserve_aspect_ratio: "none",

                // The printed arc, and the red stretch above 0 VU.
                path {
                    d: "{arc}",
                    fill: "none",
                    stroke: "{face.ink()}",
                    stroke_width: "0.8",
                    opacity: "0.85",
                }
                // Only a VU prints a red stretch; a decibel readout does not.
                if card.hot_from().is_some() {
                    path {
                        d: "{hot_arc_path()}",
                        fill: "none",
                        stroke: "{face.hot()}",
                        stroke_width: "1.6",
                    }
                }

                // Scale ticks. Majors are longer and numbered.
                for (v , label , major) in card.ticks().iter().copied() {
                    {
                        let (x1, y1) = scale_point(card, v, 6.0);
                        let (x2, y2) = scale_point(card, v, if major { 13.0 } else { 10.0 });
                        let (lx, ly) = scale_point(card, v, 22.0);
                        let hot = card.hot_from().map(|from| v > from).unwrap_or(false);
                        let color = if hot { face.hot() } else { face.ink() };
                        rsx! {
                            line {
                                x1: "{x1:.2}", y1: "{y1:.2}", x2: "{x2:.2}", y2: "{y2:.2}",
                                stroke: "{color}",
                                stroke_width: if major { "1.1" } else { "0.6" },
                            }
                            if major {
                                text {
                                    x: "{lx:.2}", y: "{ly + 2.2:.2}",
                                    fill: "{color}", font_size: "6",
                                    text_anchor: "middle", font_weight: "600",
                                    "{label}"
                                }
                            }
                        }
                    }
                }

                // Legend under the scale — "VU", "GAIN REDUCTION".
                text {
                    x: "{VU_W * 0.5:.2}", y: "{VU_H * 0.93:.2}",
                    fill: "{face.ink()}", font_size: "5.0",
                    text_anchor: "middle", letter_spacing: "0.6",
                    "{legend}"
                }

                // Needle + hub.
                line {
                    "data-testid": "vu-needle",
                    x1: "{PIVOT_X:.2}", y1: "{PIVOT_Y:.2}",
                    x2: "{nx:.2}", y2: "{ny:.2}",
                    stroke: "{face.needle()}",
                    stroke_width: "1.1",
                    stroke_linecap: "round",
                }
                circle {
                    cx: "{PIVOT_X:.2}", cy: "{VU_H:.2}", r: "4.0",
                    fill: "{face.needle()}", opacity: "0.9",
                }
            }

            // Glass: a soft highlight across the top of the bezel.
            div {
                style: "position:absolute; inset:0; pointer-events:none; \
                        background:linear-gradient(160deg, rgba(255,255,255,0.22) 0%, \
                        rgba(255,255,255,0.04) 38%, rgba(0,0,0,0.10) 100%);",
            }
        }
    }
}

/// The red stretch of the scale, from 0 VU to the right stop.
fn hot_arc_path() -> String {
    let (x0, y0) = scale_point(VuScale::Vu, 0.0, 6.0);
    let (x1, y1) = scale_point(VuScale::Vu, 3.0, 6.0);
    let r = crate::hardware::vu_svg::NEEDLE_LEN - 6.0;
    format!("M {x0:.2} {y0:.2} A {r:.2} {r:.2} 0 0 1 {x1:.2} {y1:.2}")
}
