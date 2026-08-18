//! The strip under the roll — velocity, or a pinned controller lane.
//!
//! A second surface with the same coordinate contract as the roll above
//! it: no `viewBox`, box from layout, so `element_coordinates()` is a
//! document coordinate. It had `preserve_aspect_ratio: none` instead,
//! which stretched rather than letterboxed — visually tidy, and wrong in
//! the way that is hardest to notice, because it wrote the velocity of
//! whichever note sat under the *scaled* position.

use dioxus::prelude::*;
use expression_editor_core::Editor;

use crate::{canvas, theme};

/// Write a strip value from a pointer position.
///
/// A free function over `Signal` (which is `Copy`) rather than a shared
/// closure — otherwise both pointer handlers fight over one `FnMut`.
fn strip_write(mut editor: Signal<Editor>, h: f64, x: f64, y: f64) {
    let v = (1.0 - y / h).clamp(0.0, 1.0);
    let hit: Vec<_> = {
        let ed = editor.read();
        let rx = x - canvas::GUTTER_W;
        let t = ed.camera.t_at(rx);
        ed.doc
            .notes
            .iter()
            // A generous grab window around the onset: the stem is only
            // a few pixels wide, and this is a value edit, not a
            // precision selection.
            .filter(|n| {
                let dx = (ed.camera.x(n.start) - rx).abs();
                dx <= 8.0 || (n.start <= t && n.end > t && dx <= 40.0)
            })
            .map(|n| n.id)
            .collect()
    };
    if hit.is_empty() {
        return;
    }
    let edit = match editor.read().strip_lane {
        expression_editor_core::StripLane::OffVelocity => {
            expression_editor_core::Edit::SetOffVelocity {
                notes: hit,
                velocity: v,
            }
        }
        _ => expression_editor_core::Edit::SetVelocity {
            notes: hit,
            velocity: v,
        },
    };
    editor.write().apply_live(&edit);
}

/// The velocity / CC lane strip below the roll.
///
/// Shares the roll's horizontal camera exactly — a stem must sit under
/// its note — but has its own vertical scale, because the value being
/// edited has nothing to do with pitch.
#[component]
pub fn LaneStrip(editor: Signal<Editor>) -> Element {
    let mut editor = editor;
    let mut drag = use_signal(|| None::<(f64, f64)>);

    let ed = editor.read();
    let h = ed.lane_strip_h;
    if h <= 0.0 {
        return rsx! {};
    }
    let vp = ed.viewport;
    let stems = canvas::stems(&ed, h);
    let curves = canvas::strip_curves(&ed, h);
    let guides = canvas::strip_guides(h);
    let label = ed.strip_lane.label();
    let per_note = ed.strip_lane.is_per_note();
    drop(ed);

    rsx! {
        div {
            "data-testid": "lane-strip",
            style: "position: relative; flex: 0 0 auto; box-sizing: border-box; \
                    height: {h}px; overflow: hidden; \
                    background: {theme::SURFACE_BAR}; border-top: 1px solid {theme::PANEL_BORDER};",
            svg {
                // Declared size and used size are the same number, for
                // the reason spelled out on the roll's svg in `roll.rs`:
                // Blitz paints an inline svg as a replaced element with
                // a hardcoded `object-fit: contain`, so anything drawn
                // is scaled by (element box / declared size).
                //
                // `width: 100%` was *not* enough. A percentage resolves
                // against layout and leaves the declared size absent, so
                // usvg still took the tree size from the content bounding
                // box — and a stem dragged past the top of the strip
                // rescaled the whole lane. Out of flow with both axes
                // given, the scale is exactly 1 and
                // `element_coordinates()` — which feeds `strip_write` —
                // is a document coordinate.
                width: "{vp.w + canvas::GUTTER_W:.0}",
                height: "{h:.0}",
                style: "position: absolute; left: 0; top: 0; display: block; \
                        width: {vp.w + canvas::GUTTER_W:.0}px; height: {h:.0}px; \
                        touch-action: none; user-select: none; cursor: ns-resize;",
                onpointerdown: move |e: PointerEvent| {
                    let c = e.data().element_coordinates();
                    if !per_note {
                        return;
                    }
                    editor.write().begin_gesture();
                    drag.set(Some((c.x, c.y)));
                    strip_write(editor, h, c.x, c.y);
                },
                onpointermove: move |e: PointerEvent| {
                    if drag.read().is_none() {
                        return;
                    }
                    let c = e.data().element_coordinates();
                    strip_write(editor, h, c.x, c.y);
                },
                onpointerup: move |_| drag.set(None),
                onpointerleave: move |_| drag.set(None),

                // The gutter column, so the strip lines up with the roll.
                rect {
                    x: "0", y: "0",
                    width: "{canvas::GUTTER_W:.0}", height: "{h:.0}",
                    fill: theme::GUTTER_BG,
                }
                text {
                    x: "6", y: "14",
                    fill: theme::TEXT_DIM, font_size: "9",
                    "{label}"
                }

                g {
                    transform: "translate({canvas::GUTTER_W:.0} 0)",
                    for (i, (y, major)) in guides.iter().enumerate() {
                        line {
                            key: "sg{i}",
                            x1: "0", y1: "{y:.1}",
                            x2: "{vp.w:.0}", y2: "{y:.1}",
                            stroke: if *major { theme::GRID_BEAT } else { theme::GRID_SUB },
                            stroke_width: "1",
                        }
                    }
                    for (i, s) in stems.iter().enumerate() {
                        g {
                            key: "st{i}",
                            rect {
                                x: "{s.x:.1}",
                                y: "{s.y:.1}",
                                width: "{s.w:.1}",
                                height: "{s.h.max(1.0):.1}",
                                fill: s.color,
                                fill_opacity: if s.muted { "0.2" } else { "0.85" },
                            }
                            // A cap on the selected stems, so the ones a
                            // drag will actually move are obvious.
                            if s.selected {
                                rect {
                                    x: "{s.x - 1.0:.1}",
                                    y: "{s.y - 2.0:.1}",
                                    width: "{s.w + 2.0:.1}",
                                    height: "3",
                                    fill: theme::SELECTED,
                                }
                            }
                        }
                    }
                    for (i, c) in curves.iter().enumerate() {
                        polyline {
                            key: "sc{i}",
                            // Still svg here, so the numbers are
                            // formatted for it at the one place that
                            // needs text. The roll paints instead, and
                            // does not go near this.
                            points: canvas::points_attr(&c.points),
                            fill: "none",
                            stroke: c.color,
                            stroke_width: "1.5",
                            stroke_opacity: if c.selected { "1" } else { "0.6" },
                        }
                    }
                }
            }
        }
    }
}
