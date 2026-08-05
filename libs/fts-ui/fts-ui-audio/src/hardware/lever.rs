//! A lever switch — the paddle a Pultec selects its frequencies with.
//!
//! Not a knob with a long pointer: a lever is a bar through a pivot, and you
//! read it by where the bar points, with the legends printed in an arc above
//! it on the panel rather than around a ring. It also sweeps far less than a
//! knob — the positions have to stay distinguishable at a glance from across a
//! room, which is the whole reason a console uses one.

use dioxus::prelude::*;

use crate::param::ParamHandle;

/// Half the lever's travel, in degrees either side of vertical.
pub const LEVER_SWEEP_DEG: f64 = 62.0;

/// Angle for detent `index` of `count`; 0° is straight up.
pub fn lever_angle(index: usize, count: usize) -> f64 {
    if count < 2 {
        return 0.0;
    }
    (index as f64 / (count - 1) as f64 - 0.5) * 2.0 * LEVER_SWEEP_DEG
}

/// Where a legend sits above the pivot, in the lever's own viewBox units.
fn legend_point(index: usize, count: usize, radius: f64) -> (f64, f64) {
    let rad = lever_angle(index, count).to_radians();
    (radius * rad.sin(), -radius * rad.cos())
}

/// A lever switch bound to a stepped parameter.
///
/// Clicking a legend selects that position; clicking the paddle advances to
/// the next one and wraps, which is how you use one without aiming.
#[component]
pub fn LeverSwitch(
    handle: ParamHandle,
    testid: String,
    scale: f64,
    /// Legends, in order. Printed in an arc above the pivot.
    labels: Vec<String>,
    /// Small caption above the arc — "CPS", "KCS".
    #[props(default)]
    unit: Option<String>,
    #[props(default = "#e6ecf0".to_string())] ink: String,
    /// Paddle length in design-space px, from the pivot to the tip.
    #[props(default = 34.0)]
    length: f64,
) -> Element {
    let count = labels.len().max(1);
    let selected = if count > 1 {
        (handle.normalized().clamp(0.0, 1.0) * (count - 1) as f32).round() as usize
    } else {
        0
    };
    let angle = lever_angle(selected, count);

    // The viewBox is a square around the pivot: the paddle hangs below it, the
    // legends arc above.
    let box_px = length * 3.2 * scale;

    rsx! {
        div {
            "data-testid": "hw-lever-{testid}",
            "data-index": "{selected}",
            style: format!("position:relative; width:{box_px:.1}px; height:{box_px:.1}px;"),

            svg {
                style: "position:absolute; inset:0; width:100%; height:100%; display:block;",
                view_box: "-55 -55 110 110",

                if let Some(unit) = &unit {
                    text {
                        x: "0", y: "-49",
                        fill: "{ink}", font_size: "8.5", font_weight: "700",
                        text_anchor: "middle", letter_spacing: "0.6",
                        "{unit}"
                    }
                }

                // Printed legends. Each is clickable, so a lever can be aimed
                // as well as advanced.
                for (index , label) in labels.iter().enumerate() {
                    {
                        let (lx, ly) = legend_point(index, count, 44.0);
                        let active = index == selected;
                        let handle = handle.clone();
                        let step = if count > 1 {
                            index as f32 / (count - 1) as f32
                        } else {
                            0.0
                        };
                        rsx! {
                            text {
                                "data-testid": "hw-lever-{testid}-{index}",
                                x: "{lx:.2}", y: "{ly + 2.4:.2}",
                                fill: "{ink}",
                                font_size: "10",
                                font_weight: if active { "800" } else { "600" },
                                opacity: if active { "1" } else { "0.62" },
                                text_anchor: "middle",
                                onclick: move |_| {
                                    handle.begin_edit();
                                    handle.set_normalized(step);
                                    handle.end_edit();
                                },
                                "{label}"
                            }
                        }
                    }
                }

                // The paddle: a bar through the pivot, wide below and tapering
                // to the index tip above.
                g {
                    transform: "rotate({angle:.2})",
                    polygon {
                        points: "-4.4,-27 4.4,-27 8.4,26 -8.4,26",
                        fill: "#141416",
                    }
                    // The white stripe you actually read.
                    rect {
                        x: "-1.5", y: "-25", width: "3.0", height: "18", rx: "1.2",
                        fill: "#f1f1ef",
                    }
                }
                // Pivot boss.
                circle { cx: "0", cy: "0", r: "6.5", fill: "#1b1b1e" }
                circle { cx: "0", cy: "0", r: "2.2", fill: "rgba(255,255,255,0.14)" }
            }

            // Click the paddle to advance a position.
            div {
                style: "position:absolute; left:34%; top:38%; width:32%; height:44%; cursor:pointer;",
                onclick: {
                    let handle = handle.clone();
                    move |_| {
                        let next = (selected + 1) % count;
                        let step = if count > 1 {
                            next as f32 / (count - 1) as f32
                        } else {
                            0.0
                        };
                        handle.begin_edit();
                        handle.set_normalized(step);
                        handle.end_edit();
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_travel_is_symmetric_about_vertical() {
        assert_eq!(lever_angle(0, 5), -LEVER_SWEEP_DEG);
        assert_eq!(lever_angle(4, 5), LEVER_SWEEP_DEG);
        assert_eq!(lever_angle(2, 5), 0.0);
    }

    #[test]
    fn a_lever_sweeps_far_less_than_a_knob() {
        // A knob turns 300°; a lever has to stay readable at a glance.
        assert!(LEVER_SWEEP_DEG * 2.0 < 150.0);
    }

    #[test]
    fn legends_clear_the_paddle() {
        // The paddle's index tip reaches 27 units; the legends are printed
        // beyond it or they sit on top of the thing that points at them.
        let (_, ly) = legend_point(1, 4, 44.0);
        assert!(ly < -27.0, "legend at {ly} is inside the paddle's reach");
    }

    #[test]
    fn a_single_position_lever_points_straight_up() {
        assert_eq!(lever_angle(0, 1), 0.0);
        assert_eq!(lever_angle(0, 0), 0.0);
    }

    #[test]
    fn legends_run_left_to_right_and_sit_above_the_pivot() {
        let (lx, ly) = legend_point(0, 4, 34.0);
        let (rx, ry) = legend_point(3, 4, 34.0);
        assert!(lx < 0.0 && rx > 0.0);
        assert!(ly < 0.0 && ry < 0.0, "legends print above the pivot");
    }
}
