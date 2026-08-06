//! Panel buttons and indicators — the console's switching.
//!
//! A rack unit's switch is a bat toggle; a console's is a latching rectangular
//! button with a lamp under it, and the lamp is how you read the channel from
//! two feet away. Same job, different idiom, so it is a different widget.
//!
//! Buttons come in two states of wiring, and both draw: one bound to a
//! parameter, and one that is only on the panel because the unit has it. The
//! second is deliberate — the panel is the specification for what the DSP
//! still owes, and drawing it is how that stays visible.

use dioxus::prelude::*;

use crate::param::ParamHandle;

/// A latching panel button with an indicator lamp beneath it.
///
/// With `handle`, it toggles a parameter and the lamp follows it. Without one,
/// it draws in its off state and does nothing — see the module docs.
#[component]
pub fn PanelButton(
    #[props(default)] handle: Option<ParamHandle>,
    testid: String,
    scale: f64,
    label: String,
    /// Face colour.
    #[props(default = "#e6e2d4".to_string())]
    color: String,
    /// Label colour.
    #[props(default = "#23252a".to_string())]
    ink: String,
    /// Lamp colour. Empty draws no lamp.
    #[props(default = "#43d17a".to_string())]
    led: String,
    #[props(default = 34.0)] w: f64,
    #[props(default = 46.0)] h: f64,
) -> Element {
    let on = handle
        .as_ref()
        .map(|h| h.normalized() >= 0.5)
        .unwrap_or(false);
    let wired = handle.is_some();

    rsx! {
        div {
            "data-testid": "hw-button-{testid}",
            "data-on": "{on}",
            "data-wired": "{wired}",
            style: format!(
                "display:flex; flex-direction:column; align-items:center; gap:{:.1}px;",
                5.0 * scale,
            ),

            div {
                style: format!(
                    "width:{:.1}px; height:{:.1}px; border-radius:{:.1}px; \
                     display:flex; align-items:center; justify-content:center; \
                     text-align:center; line-height:1.05; \
                     font-size:{:.1}px; font-weight:800; letter-spacing:0.02em; \
                     color:{ink}; cursor:{}; \
                     background:linear-gradient(180deg, \
                       color-mix(in oklab, {color} 88%, white), {color}); \
                     border:{:.1}px solid rgba(0,0,0,0.62); \
                     box-shadow:{};",
                    w * scale,
                    h * scale,
                    3.0 * scale,
                    9.0 * scale,
                    if wired { "pointer" } else { "default" },
                    (1.0 * scale).max(1.0),
                    if on {
                        format!("inset 0 {:.1}px {:.1}px rgba(0,0,0,0.45)", 1.5 * scale, 3.0 * scale)
                    } else {
                        format!("0 {:.1}px {:.1}px rgba(0,0,0,0.45)", 1.5 * scale, 3.0 * scale)
                    },
                ),
                onclick: {
                    let handle = handle.clone();
                    move |_| {
                        if let Some(handle) = &handle {
                            handle.begin_edit();
                            handle.set_normalized(if on { 0.0 } else { 1.0 });
                            handle.end_edit();
                        }
                    }
                },
                "{label}"
            }

            if !led.is_empty() {
                Lamp { scale, color: led.clone(), lit: on }
            }
        }
    }
}

/// A panel indicator lamp — a domed jewel in a bezel.
///
/// Built from explicit stops rather than `color-mix` on the lamp's colour: the
/// core has to be *brighter than the colour itself* for the lens to read as
/// lit from within, and a mix that silently fails leaves a dark disc that
/// looks like a sticker. The colour supplies the hue in the middle band;
/// white and black do the lighting.
#[component]
pub fn Lamp(
    scale: f64,
    color: String,
    /// Lit lamps glow; dark ones still show as an unlit lens, because an empty
    /// hole on a panel reads as a missing part.
    #[props(default = true)]
    lit: bool,
    #[props(default = 9.0)] d: f64,
) -> Element {
    rsx! {
        div {
            "data-testid": "hw-lamp",
            "data-lit": "{lit}",
            style: format!(
                "width:{:.1}px; height:{:.1}px; border-radius:50%; \
                 background:{}; box-shadow:{}; \
                 border:{:.1}px solid rgba(0,0,0,0.65);",
                d * scale,
                d * scale,
                if lit {
                    format!(
                        "radial-gradient(circle at 36% 30%, \
                           rgba(255,255,255,0.92) 0%, \
                           rgba(255,255,255,0.35) 16%, \
                           {color} 46%, \
                           {color} 66%, \
                           rgba(0,0,0,0.55) 100%)"
                    )
                } else {
                    format!(
                        "radial-gradient(circle at 36% 30%, \
                           rgba(255,255,255,0.10) 0%, \
                           {color} 52%, \
                           rgba(0,0,0,0.72) 100%)"
                    )
                },
                if lit {
                    format!(
                        "0 0 {:.1}px {}, inset 0 {:.1}px {:.1}px rgba(0,0,0,0.35)",
                        d * 0.7 * scale,
                        color,
                        d * 0.16 * scale,
                        d * 0.26 * scale,
                    )
                } else {
                    format!("inset 0 0 {:.1}px rgba(0,0,0,0.75)", d * 0.34 * scale)
                },
                (1.0 * scale).max(1.0),
            ),
        }
    }
}

/// Segment boundaries of a console output meter, in dB — the ladder the
/// numbers are printed against.
pub const METER_STEPS_DB: &[f64] = &[
    0.0, -2.0, -4.0, -6.0, -8.0, -10.0, -15.0, -20.0, -30.0, -40.0,
];

/// Which segments are lit at a given level, top segment first.
///
/// A segment lights when the level reaches its bottom edge, which is what
/// makes a ladder read as a bar rather than as a dot.
pub fn lit_segments(level_db: f32) -> usize {
    METER_STEPS_DB
        .iter()
        .rev()
        .take_while(|edge| (level_db as f64) >= **edge)
        .count()
}

/// A segmented LED level meter.
#[component]
pub fn LedMeter(
    scale: f64,
    level_db: f32,
    #[props(default = 130.0)] h: f64,
    #[props(default = 13.0)] w: f64,
) -> Element {
    let lit = lit_segments(level_db);
    let count = METER_STEPS_DB.len();
    let seg_h = h / count as f64;

    rsx! {
        div {
            "data-testid": "hw-led-meter",
            "data-lit": "{lit}",
            style: format!(
                "display:flex; flex-direction:column-reverse; gap:{:.1}px; \
                 width:{:.1}px; height:{:.1}px; padding:{:.1}px; \
                 background:#0a0b0c; border-radius:{:.1}px; \
                 border:{:.1}px solid rgba(0,0,0,0.7);",
                1.0 * scale,
                w * scale,
                h * scale,
                1.5 * scale,
                2.0 * scale,
                (1.0 * scale).max(1.0),
            ),

            for index in 0..count {
                {
                    // Top of the ladder is the hot end: red, then amber, then
                    // green, as every console meter has ever been.
                    let from_top = count - 1 - index;
                    let hue = if from_top == 0 {
                        "#e0483a"
                    } else if from_top <= 2 {
                        "#e0a03a"
                    } else {
                        "#5ad24a"
                    };
                    let on = index < lit;
                    rsx! {
                        div {
                            style: format!(
                                "flex:1; min-height:{:.1}px; border-radius:{:.1}px; background:{};",
                                (seg_h - 1.0).max(1.0) * scale,
                                1.0 * scale,
                                if on {
                                    hue.to_string()
                                } else {
                                    format!("color-mix(in oklab, {hue} 16%, #0e1012)")
                                },
                            ),
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_lights_nothing_and_zero_lights_everything() {
        assert_eq!(lit_segments(-90.0), 0);
        assert_eq!(lit_segments(0.0), METER_STEPS_DB.len());
    }

    #[test]
    fn the_ladder_fills_from_the_bottom() {
        let quiet = lit_segments(-35.0);
        let loud = lit_segments(-6.0);
        assert!(quiet > 0 && quiet < loud, "{quiet} then {loud}");
        assert!(loud < METER_STEPS_DB.len());
    }

    #[test]
    fn a_segment_lights_at_its_own_edge() {
        // -20 is a printed edge: reaching it lights that segment, and a hair
        // under it does not.
        assert!(lit_segments(-20.0) > lit_segments(-20.1));
    }

    #[test]
    fn the_scale_runs_downward_without_repeating() {
        assert!(METER_STEPS_DB.windows(2).all(|w| w[0] > w[1]));
    }
}

/// A horizontal LED ladder with its scale printed above it — the Distressor's
/// gain-reduction display.
///
/// Reads right to left: 1 dB at the right end, the deepest reduction at the
/// left, so the row fills leftward as the compressor works. Green through
/// yellow to red, and the numbers are the dB each lamp stands for.
#[component]
pub fn LedBar(
    scale: f64,
    /// dB the row is showing (positive = reduction).
    value_db: f32,
    /// The printed scale, left (deepest) to right (least).
    steps: Vec<f64>,
    #[props(default = 9.0)] led_d: f64,
    #[props(default = 20.0)] pitch: f64,
    #[props(default = "#cfd4d8".to_string())] ink: String,
) -> Element {
    rsx! {
        div {
            "data-testid": "hw-led-bar",
            // Fixed-width cells rather than gaps: a row has to be exactly as
            // wide as `labels * pitch`, or the panel cannot place it.
            style: "display:flex; align-items:flex-end;",
            for step in steps.iter().copied() {
                {
                    // A lamp lights once the reduction reaches the number
                    // printed above it.
                    let lit = (value_db as f64) >= step;
                    let hue = if step >= 12.0 {
                        "#e0483a"
                    } else if step >= 6.0 {
                        "#e8c53a"
                    } else {
                        "#5ad24a"
                    };
                    rsx! {
                        div {
                            style: format!(
                                "width:{:.1}px; display:flex; flex-direction:column; \
                                 align-items:center; gap:{:.1}px;",
                                pitch * scale,
                                3.0 * scale,
                            ),
                            div {
                                style: format!(
                                    "font-size:{:.1}px; font-weight:700; color:{ink};",
                                    7.0 * scale,
                                ),
                                "{step:.0}"
                            }
                            Lamp { scale, color: hue.to_string(), lit, d: led_d }
                        }
                    }
                }
            }
        }
    }
}

/// A row of labelled LEDs that selects a stepped parameter — the Distressor's
/// ratio row, where the lamp *is* the readout and the button beside it steps
/// through them.
#[component]
pub fn LedSelect(
    handle: ParamHandle,
    testid: String,
    scale: f64,
    labels: Vec<String>,
    #[props(default = 9.0)] led_d: f64,
    #[props(default = 44.0)] pitch: f64,
    #[props(default = "#cfd4d8".to_string())] ink: String,
    /// Colour of the lit lamp, per position. Falls back to green.
    #[props(default)]
    colors: Vec<String>,
) -> Element {
    let count = labels.len().max(1);
    let selected = if count > 1 {
        (handle.normalized().clamp(0.0, 1.0) * (count - 1) as f32).round() as usize
    } else {
        0
    };

    rsx! {
        div {
            "data-testid": "hw-led-select-{testid}",
            "data-index": "{selected}",
            style: "display:flex; align-items:flex-end;",
            for (index , label) in labels.iter().enumerate() {
                {
                    let active = index == selected;
                    let color = colors
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| "#5ad24a".to_string());
                    let handle = handle.clone();
                    let step = if count > 1 {
                        index as f32 / (count - 1) as f32
                    } else {
                        0.0
                    };
                    rsx! {
                        div {
                            "data-testid": "hw-led-select-{testid}-{index}",
                            style: format!(
                                "width:{:.1}px; display:flex; flex-direction:column; \
                                 align-items:center; gap:{:.1}px; cursor:pointer;",
                                pitch * scale,
                                3.0 * scale,
                            ),
                            onclick: move |_| {
                                handle.begin_edit();
                                handle.set_normalized(step);
                                handle.end_edit();
                            },
                            div {
                                style: format!(
                                    "font-size:{:.1}px; font-weight:700; color:{ink}; \
                                     opacity:{};",
                                    7.5 * scale,
                                    if active { "1" } else { "0.66" },
                                ),
                                "{label}"
                            }
                            Lamp { scale, color, lit: active, d: led_d }
                        }
                    }
                }
            }
        }
    }
}
