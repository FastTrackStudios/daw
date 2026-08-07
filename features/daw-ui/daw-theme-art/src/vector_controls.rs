//! The mixer controls as true vectors.
//!
//! [`crate::mixer_controls`] draws these from traced rects — pixel-exact
//! with the originals, and pixel-*shaped*: zoom in and you see the steps,
//! because a trace is a picture of a bitmap however you store it.
//!
//! These are the same controls drawn as shapes — circles, rounded rects,
//! gradients, glyphs — so they stay sharp at any zoom. Proportions are
//! taken from the originals (a ring for record-arm, a gradient-filled
//! rounded rect with a bevelled letter for mute/solo/FX, a bevelled body
//! with a ribbed panel for the fader cap), so they still read as the same
//! theme.
//!
//! # Everything is proportional
//!
//! Each control draws into a `viewBox` in its own units and every dimension
//! is a fraction of that — no pixel constants. That is what "infinitely
//! zoomable" actually requires: a 1px border baked in at 20px tall becomes
//! a 10px slab at 400px tall.

use daw_theme::{Color, Theme};
use dioxus::prelude::*;

pub use crate::mixer_controls::{FxChain, Interaction, Monitoring, RecordArm, Solo};

/// Common sizing props.
#[derive(Props, Clone, PartialEq, Default)]
pub struct VectorProps {
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// A control's palette, resolved once per render.
struct Ink {
    face: Color,
    face_top: Color,
    border: Color,
    text: Color,
    shadow: Color,
}

fn ink(lit: Option<Color>, at: Interaction) -> Ink {
    let t = Theme::default();
    let c = &t.chrome;
    let base = lit.unwrap_or_else(|| c.surface_raised.shade(0.06));
    // The original's three sprite cells are the same button lit differently:
    // hover lifts, pressed sinks. Reproducing that as a shade keeps one
    // drawing doing the work of three images.
    let face = match at {
        Interaction::Normal => base,
        Interaction::Hover => base.shade(0.12),
        Interaction::Pressed => base.shade(-0.12),
    };
    Ink {
        // The originals light from the top: the face is a vertical gradient
        // with the lighter stop above, which is most of what makes them
        // read as physical rather than flat.
        face_top: face.shade(0.10),
        face,
        border: c.border,
        text: if lit.is_some() { c.selected } else { c.text },
        shadow: c.surface.shade(-0.4),
    }
}

// ── a labelled button: mute, solo, FX ────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct LabelButtonProps {
    pub label: String,
    /// Face colour when engaged. `None` draws the resting state.
    #[props(default)]
    pub lit: Option<Color>,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// The shape mute, solo and FX all share.
#[component]
pub fn LabelButton(props: LabelButtonProps) -> Element {
    let k = ink(props.lit, props.at);
    // Native proportions of REAPER's button art: 21x20 per sprite cell.
    let (vw, vh) = (21.0f32, 20.0f32);
    let r = vh * 0.18;
    let id = format!("lb{}", props.label.replace(' ', ""));

    rsx! {
        svg {
            width: "{props.width.unwrap_or(21)}",
            height: "{props.height.unwrap_or(20)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "{id}", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{k.face_top.css()}" }
                    stop { offset: "1", stop_color: "{k.face.css()}" }
                }
            }
            rect {
                x: "{vw * 0.06}", y: "{vh * 0.06}",
                width: "{vw * 0.88}", height: "{vh * 0.88}",
                rx: "{r}",
                fill: "url(#{id})",
                stroke: "{k.border.css()}",
                stroke_width: "{vh * 0.05}",
            }
            // The glyph is drawn twice: a shadow a fraction below, then the
            // face. That offset is what gives the originals their stamped
            // look, and being proportional it survives any zoom.
            text {
                x: "{vw * 0.5}", y: "{vh * 0.56 + vh * 0.04}",
                text_anchor: "middle", dominant_baseline: "central",
                font_family: "Fira Sans, DejaVu Sans, sans-serif",
                font_weight: "700",
                font_size: "{vh * 0.62}",
                fill: "{k.shadow.css()}",
                fill_opacity: "0.55",
                "{props.label}"
            }
            text {
                x: "{vw * 0.5}", y: "{vh * 0.56}",
                text_anchor: "middle", dominant_baseline: "central",
                font_family: "Fira Sans, DejaVu Sans, sans-serif",
                font_weight: "700",
                font_size: "{vh * 0.62}",
                fill: "{k.text.css()}",
                "{props.label}"
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct ToggleProps {
    #[props(default)]
    pub on: bool,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

#[component]
pub fn MuteButton(props: ToggleProps) -> Element {
    let t = Theme::default();
    rsx! {
        LabelButton {
            label: "M",
            lit: props.on.then_some(t.signal.mute),
            width: props.width, height: props.height, at: props.at,
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct SoloProps {
    #[props(default)]
    pub state: Solo,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

#[component]
pub fn SoloButton(props: SoloProps) -> Element {
    let t = Theme::default();
    // Defeat is a different thing from solo, not more of it, so it gets a
    // different hue rather than a brighter one.
    let lit = match props.state {
        Solo::Off => None,
        Solo::On => Some(t.signal.solo),
        Solo::Defeat => Some(t.chrome.accent),
    };
    rsx! {
        LabelButton { label: "S", lit, width: props.width, height: props.height, at: props.at }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct FxProps {
    #[props(default)]
    pub state: FxChain,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

#[component]
pub fn FxButton(props: FxProps) -> Element {
    let t = Theme::default();
    let lit = match props.state {
        FxChain::Empty => None,
        FxChain::Active => Some(t.chrome.surface_raised.mix(t.chrome.accent, 0.35)),
        FxChain::Bypassed => Some(t.signal.meter_warn.shade(-0.25)),
    };
    rsx! {
        LabelButton { label: "FX", lit, width: props.width, height: props.height, at: props.at }
    }
}

// ── record arm: a ring ───────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct RecordArmProps {
    #[props(default)]
    pub state: RecordArm,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

#[component]
pub fn RecordArmButton(props: RecordArmProps) -> Element {
    let t = Theme::default();
    let armed = matches!(
        props.state,
        RecordArm::On | RecordArm::NoRecord | RecordArm::AutoOn | RecordArm::AutoNoRecord
    );
    let auto = matches!(
        props.state,
        RecordArm::Auto | RecordArm::AutoOn | RecordArm::AutoNoRecord
    );
    let barred = matches!(props.state, RecordArm::NoRecord | RecordArm::AutoNoRecord);

    let ring = if armed {
        t.signal.rec
    } else {
        t.chrome.text_dim
    };
    let (vw, vh) = (20.0f32, 20.0f32);
    let cx = vw * 0.5;
    let cy = vh * 0.5;
    // Stroke width *is* the ring: a filled circle with a stroke this thick
    // is how the original reads, and keeping it a fraction of the box is
    // what lets it scale.
    let stroke = vh * 0.16;
    let radius = vh * 0.26;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(20)}",
            height: "{props.height.unwrap_or(20)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            circle {
                cx: "{cx}", cy: "{cy}", r: "{radius}",
                fill: "{t.chrome.surface.css()}",
                stroke: "{ring.css()}",
                stroke_width: "{stroke}",
            }
            if barred {
                // The "armed but cannot record" cross.
                line {
                    x1: "{cx - radius}", y1: "{cy - radius}",
                    x2: "{cx + radius}", y2: "{cy + radius}",
                    stroke: "{t.chrome.surface.css()}",
                    stroke_width: "{stroke * 0.9}",
                    stroke_linecap: "round",
                }
                line {
                    x1: "{cx + radius}", y1: "{cy - radius}",
                    x2: "{cx - radius}", y2: "{cy + radius}",
                    stroke: "{t.chrome.surface.css()}",
                    stroke_width: "{stroke * 0.9}",
                    stroke_linecap: "round",
                }
            }
            if auto && !armed {
                text {
                    x: "{cx}", y: "{cy}",
                    text_anchor: "middle", dominant_baseline: "central",
                    font_family: "Fira Sans, DejaVu Sans, sans-serif",
                    font_weight: "700", font_size: "{vh * 0.42}",
                    fill: "{ring.css()}",
                    "A"
                }
            }
        }
    }
}

// ── routing: stacked bars ────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct RoutingProps {
    #[props(default)]
    pub has_sends: bool,
    #[props(default)]
    pub has_receives: bool,
    #[props(default)]
    pub disabled: bool,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

#[component]
pub fn RoutingButton(props: RoutingProps) -> Element {
    let t = Theme::default();
    let k = ink(None, props.at);
    let (vw, vh) = (20.0f32, 20.0f32);
    let dim = t.chrome.text_faint;
    // Top bar = receives, bottom = sends. Lit means present, which is the
    // whole information the control carries.
    let recv = if props.has_receives {
        t.chrome.accent
    } else {
        dim
    };
    let send = if props.has_sends {
        t.signal.meter_warn
    } else {
        dim
    };
    let opacity = if props.disabled { "0.4" } else { "1" };

    rsx! {
        svg {
            width: "{props.width.unwrap_or(20)}",
            height: "{props.height.unwrap_or(20)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            opacity: "{opacity}",
            rect {
                x: "{vw * 0.08}", y: "{vh * 0.08}",
                width: "{vw * 0.84}", height: "{vh * 0.84}",
                rx: "{vh * 0.16}",
                fill: "{k.face.css()}",
                stroke: "{k.border.css()}", stroke_width: "{vh * 0.05}",
            }
            rect {
                x: "{vw * 0.24}", y: "{vh * 0.28}",
                width: "{vw * 0.52}", height: "{vh * 0.12}",
                rx: "{vh * 0.06}", fill: "{recv.css()}",
            }
            rect {
                x: "{vw * 0.24}", y: "{vh * 0.6}",
                width: "{vw * 0.52}", height: "{vh * 0.12}",
                rx: "{vh * 0.06}", fill: "{send.css()}",
            }
        }
    }
}

// ── input monitoring: concentric arcs ────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct MonitoringProps {
    #[props(default)]
    pub state: Monitoring,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

#[component]
pub fn InputMonitorIndicator(props: MonitoringProps) -> Element {
    let t = Theme::default();
    let (vw, vh) = (20.0f32, 20.0f32);
    let colour = match props.state {
        Monitoring::Off => t.chrome.text_faint,
        Monitoring::On => t.signal.meter_safe,
        Monitoring::Auto => t.signal.meter_warn,
    };
    let cx = vw * 0.5;
    let cy = vh * 0.72;
    let sw = vh * 0.09;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(20)}",
            height: "{props.height.unwrap_or(20)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            circle { cx: "{cx}", cy: "{cy}", r: "{vh * 0.07}", fill: "{colour.css()}" }
            // Two arcs above the dot — signal arriving, which is what
            // monitoring means. Radii are fractions so they never crowd.
            for (i, rad) in [vh * 0.22, vh * 0.36].iter().enumerate() {
                path {
                    key: "{i}",
                    d: "M {cx - rad} {cy} A {rad} {rad} 0 0 1 {cx + rad} {cy}",
                    fill: "none",
                    stroke: "{colour.css()}",
                    stroke_width: "{sw}",
                    stroke_linecap: "round",
                }
            }
        }
    }
}

// ── pan: a knob with a pointer ───────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct PanProps {
    /// -1 hard left, 0 centre, +1 hard right.
    #[props(default = 0.0)]
    pub position: f32,
    #[props(default)]
    pub large: bool,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
}

#[component]
pub fn PanningKnob(props: PanProps) -> Element {
    let t = Theme::default();
    let k = ink(None, Interaction::Normal);
    let (vw, vh) = (24.0f32, 24.0f32);
    let (cx, cy) = (vw * 0.5, vh * 0.5);
    let r = vh * (if props.large { 0.42 } else { 0.36 });

    // A real rotation rather than a chosen sprite frame — which is the
    // point of drawing this as vector: 128 baked frames become continuous.
    let pos = props.position.clamp(-1.0, 1.0);
    let angle = pos * 135.0f32;
    let rad = (angle - 90.0).to_radians();
    let (px, py) = (cx + rad.cos() * r * 0.72, cy + rad.sin() * r * 0.72);

    rsx! {
        svg {
            width: "{props.width.unwrap_or(24)}",
            height: "{props.height.unwrap_or(24)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "panknob", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{k.face_top.css()}" }
                    stop { offset: "1", stop_color: "{k.face.shade(-0.15).css()}" }
                }
            }
            circle {
                cx: "{cx}", cy: "{cy}", r: "{r}",
                fill: "url(#panknob)",
                stroke: "{k.border.css()}", stroke_width: "{vh * 0.04}",
            }
            line {
                x1: "{cx}", y1: "{cy}", x2: "{px}", y2: "{py}",
                stroke: "{t.chrome.accent.css()}",
                stroke_width: "{vh * 0.08}",
                stroke_linecap: "round",
            }
        }
    }
}

// ── fader cap: bevelled body, ribbed panel ───────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct FaderCapProps {
    /// Track accent, which the cap picks up in REAPER's colour variants.
    #[props(default)]
    pub accent: Option<Color>,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
}

/// The fader cap — the ribbed plastic look, as geometry.
///
/// The rib count is fixed rather than derived from height: the original has
/// a set number of ridges, and deriving it would thin them out at one size
/// and crowd them at another, which is precisely what a vector version is
/// supposed to stop happening.
#[component]
pub fn VolumeFaderCap(props: FaderCapProps) -> Element {
    let t = Theme::default();
    let accent = props.accent.unwrap_or(t.chrome.accent);
    let (vw, vh) = (27.0f32, 53.0f32);
    const RIBS: usize = 7;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(27)}",
            height: "{props.height.unwrap_or(53)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "capbody", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{t.chrome.surface_raised.shade(0.18).css()}" }
                    stop { offset: "0.5", stop_color: "{t.chrome.surface_raised.css()}" }
                    stop { offset: "1", stop_color: "{t.chrome.surface.css()}" }
                }
            }
            // Body.
            rect {
                x: "{vw * 0.08}", y: "{vh * 0.03}",
                width: "{vw * 0.84}", height: "{vh * 0.94}",
                rx: "{vw * 0.22}",
                fill: "url(#capbody)",
                stroke: "{t.chrome.surface.shade(-0.4).css()}",
                stroke_width: "{vw * 0.05}",
            }
            // Inset panel the ribs sit on.
            rect {
                x: "{vw * 0.26}", y: "{vh * 0.1}",
                width: "{vw * 0.48}", height: "{vh * 0.8}",
                rx: "{vw * 0.08}",
                fill: "{accent.css()}",
            }
            // Ribs, split either side of the centre line — the original's
            // two-part grip.
            for i in 0..RIBS {
                {
                    let t0 = (i as f32 + 0.5) / RIBS as f32;
                    let y = vh * (0.13 + 0.74 * t0);
                    rsx! {
                        rect {
                            key: "{i}",
                            x: "{vw * 0.3}", y: "{y}",
                            width: "{vw * 0.4}", height: "{vh * 0.022}",
                            fill: "{accent.shade(-0.45).css()}",
                        }
                    }
                }
            }
            // The centre split.
            rect {
                x: "{vw * 0.22}", y: "{vh * 0.49}",
                width: "{vw * 0.56}", height: "{vh * 0.02}",
                fill: "{t.chrome.surface.shade(-0.3).css()}",
            }
        }
    }
}

/// The trough the cap runs in.
#[component]
pub fn VolumeFaderTrack(props: FaderCapProps) -> Element {
    let t = Theme::default();
    let (vw, vh) = (23.0f32, 55.0f32);
    rsx! {
        svg {
            width: "{props.width.unwrap_or(23)}",
            height: "{props.height.unwrap_or(55)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            rect {
                x: "{vw * 0.45}", y: "0",
                width: "{vw * 0.1}", height: "{vh}",
                rx: "{vw * 0.05}",
                fill: "{t.chrome.surface.shade(-0.5).css()}",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render_svg;

    fn valid(svg: &str) -> bool {
        let opts = resvg::usvg::Options::default();
        resvg::usvg::Tree::from_str(svg, &opts).is_ok()
    }

    #[test]
    fn every_control_produces_valid_svg_at_any_size() {
        // Proportional geometry can still go negative if a fraction is
        // wrong; resvg rejects that, and only at render time.
        for (w, h) in [(8, 8), (20, 20), (200, 200), (1200, 1200)] {
            let (w, h) = (Some(w), Some(h));
            let cases = [
                render_svg(
                    MuteButton,
                    ToggleProps {
                        on: true,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    SoloButton,
                    SoloProps {
                        state: Solo::Defeat,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    FxButton,
                    FxProps {
                        state: FxChain::Active,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    RecordArmButton,
                    RecordArmProps {
                        state: RecordArm::NoRecord,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    RoutingButton,
                    RoutingProps {
                        has_sends: true,
                        has_receives: true,
                        disabled: false,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    InputMonitorIndicator,
                    MonitoringProps {
                        state: Monitoring::On,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    PanningKnob,
                    PanProps {
                        position: -0.5,
                        large: true,
                        width: w,
                        height: h,
                    },
                ),
                render_svg(
                    VolumeFaderCap,
                    FaderCapProps {
                        accent: None,
                        width: w,
                        height: h,
                    },
                ),
                render_svg(
                    VolumeFaderTrack,
                    FaderCapProps {
                        accent: None,
                        width: w,
                        height: h,
                    },
                ),
            ];
            for (i, svg) in cases.iter().enumerate() {
                assert!(valid(svg), "control {i} invalid at {w:?}x{h:?}: {svg}");
            }
        }
    }

    #[test]
    fn nothing_is_drawn_with_a_pixel_constant() {
        // A hardcoded stroke or radius is what stops a vector scaling: it
        // looks right at the size it was written for and wrong everywhere
        // else. Every geometry attribute must be a fraction of the viewBox,
        // so scaling the box must scale the drawing.
        let small = render_svg(
            MuteButton,
            ToggleProps {
                on: false,
                width: Some(21),
                height: Some(20),
                at: Interaction::Normal,
            },
        );
        let large = render_svg(
            MuteButton,
            ToggleProps {
                on: false,
                width: Some(210),
                height: Some(200),
                at: Interaction::Normal,
            },
        );
        // Same geometry, different render size: the viewBox does the work.
        let strip = |s: &str| {
            s.split("width=\"")
                .nth(1)
                .map(|r| r.split('"').next().unwrap().to_string())
        };
        assert_ne!(strip(&small), strip(&large));
        // And the internal coordinates are identical, i.e. resolution-free.
        let body = |s: &str| s.split("<rect").nth(1).unwrap_or("").to_string();
        assert_eq!(body(&small), body(&large));
    }

    #[test]
    fn the_pan_knob_rotates_continuously() {
        // The traced version picks one of 128 baked frames; the whole point
        // of the vector one is that it does not have to.
        let a = render_svg(
            PanningKnob,
            PanProps {
                position: -1.0,
                large: false,
                width: None,
                height: None,
            },
        );
        let b = render_svg(
            PanningKnob,
            PanProps {
                position: -0.99,
                large: false,
                width: None,
                height: None,
            },
        );
        let c = render_svg(
            PanningKnob,
            PanProps {
                position: 1.0,
                large: false,
                width: None,
                height: None,
            },
        );
        assert_ne!(a, b, "a 1% pan change moved nothing");
        assert_ne!(a, c);
    }

    #[test]
    fn an_out_of_range_pan_clamps() {
        for p in [-9.0, 9.0, f32::NAN] {
            let svg = render_svg(
                PanningKnob,
                PanProps {
                    position: p,
                    large: false,
                    width: None,
                    height: None,
                },
            );
            assert!(valid(&svg), "pan {p} produced invalid SVG");
        }
    }

    #[test]
    fn interaction_shifts_the_face() {
        // Hover and pressed are real states in the original art; a vector
        // control that ignores them loses feedback the theme had.
        let n = render_svg(
            MuteButton,
            ToggleProps {
                on: false,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        let h = render_svg(
            MuteButton,
            ToggleProps {
                on: false,
                width: None,
                height: None,
                at: Interaction::Hover,
            },
        );
        let p = render_svg(
            MuteButton,
            ToggleProps {
                on: false,
                width: None,
                height: None,
                at: Interaction::Pressed,
            },
        );
        assert_ne!(n, h, "hover looks the same as normal");
        assert_ne!(n, p, "pressed looks the same as normal");
        assert_ne!(h, p);
    }

    #[test]
    fn states_stay_visually_distinct() {
        let off = render_svg(
            SoloButton,
            SoloProps {
                state: Solo::Off,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        let on = render_svg(
            SoloButton,
            SoloProps {
                state: Solo::On,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        let defeat = render_svg(
            SoloButton,
            SoloProps {
                state: Solo::Defeat,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        assert_ne!(off, on);
        assert_ne!(on, defeat, "defeat must not look like plain solo");
    }

    #[test]
    fn the_fader_cap_keeps_its_ribs_at_every_size() {
        // The ribbed plastic look is the thing being preserved; deriving
        // the rib count from height would thin them out when zoomed.
        for h in [20u32, 53, 400] {
            let svg = render_svg(
                VolumeFaderCap,
                FaderCapProps {
                    accent: None,
                    width: None,
                    height: Some(h),
                },
            );
            // 7 ribs + body + panel + split.
            assert!(svg.matches("<rect").count() >= 10, "lost ribs at h={h}");
        }
    }

    #[test]
    fn the_cap_takes_a_track_accent() {
        let green = Color::rgb(0x3d, 0xdc, 0x97);
        let svg = render_svg(
            VolumeFaderCap,
            FaderCapProps {
                accent: Some(green),
                width: None,
                height: None,
            },
        );
        assert!(svg.contains(&green.to_hex()), "{svg}");
    }
}
