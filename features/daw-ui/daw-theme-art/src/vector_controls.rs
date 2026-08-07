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
    border: Color,
    text: Color,
}

fn ink(lit: Option<Color>, at: Interaction) -> Ink {
    let t = Theme::default();
    let c = &t.chrome;
    // Unlit controls take the neutral control grey, not a shade of the
    // surface ladder: deriving it from `surface_raised` made every mute,
    // solo and FX button blue-cast and far darker than the art it stands
    // in for.
    let base = lit.unwrap_or(c.hardware);
    // The original's three sprite cells are the same button lit differently:
    // hover lifts, pressed sinks. Reproducing that as a shade keeps one
    // drawing doing the work of three images.
    let face = match at {
        Interaction::Normal => base,
        Interaction::Hover => base.shade(0.12),
        Interaction::Pressed => base.shade(-0.12),
    };
    Ink {
        // The originals light from the top. The lighter stop is derived at
        // the point of use rather than carried here, so each control can
        // pick its own falloff.
        face,
        border: c.hardware_edge,
        // Neutral, and the same lit or not: the source prints #cccccc on
        // mute and solo in both states. `text` is the panel blue-white,
        // which on a plastic button reads as a backlit legend.
        text: c.hardware_mark.shade(0.26),
    }
}

// ── a labelled button: mute, solo, FX ────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct LabelButtonProps {
    pub label: String,
    /// Face colour when engaged. `None` draws the resting state.
    #[props(default)]
    pub lit: Option<Color>,
    /// The cell this button replaces, in REAPER's pixels.
    ///
    /// Not cosmetic: mute and solo are 21x20 but FX is 28x22, and drawing
    /// both at 21x20 left the FX button to be stretched into its cell by
    /// whatever rendered it — visibly wide, with an oval `FX` on it.
    #[props(default = (21.0, 20.0))]
    pub cell: (f32, f32),
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// The shape mute, solo and FX all share.
///
/// Measured off `mcp_mute_on`, cell 0: 21x20, a 1px near-black border, a
/// single lighter highlight row just inside the top, and a body gradient
/// running darker downward. The glyph is about 9px tall — under half the
/// cell — in a soft off-white, with **no drop shadow**; the darker pixels
/// around it in the original are antialiasing, not a stamped edge.
///
/// The first version of this had a 0.62-of-height glyph and a hard
/// offset shadow, which read as a bevel at 20px and as a badge at 300px.
#[component]
pub fn LabelButton(props: LabelButtonProps) -> Element {
    let k = ink(props.lit, props.at);
    let (vw, vh) = props.cell;
    let id = format!("lb{}", props.label.replace(' ', ""));
    // Corners are barely rounded in the original — a large radius is what
    // makes a redraw look like a generic UI kit rather than this theme.
    let r = vh * 0.10;
    let edge = vh * 0.05;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(vw as u32)}",
            height: "{props.height.unwrap_or(vh as u32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "{id}", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{k.face.shade(0.06).css()}" }
                    stop { offset: "1", stop_color: "{k.face.shade(-0.10).css()}" }
                }
            }
            // Body, inset by half the border so the stroke sits inside.
            rect {
                x: "{edge / 2.0}", y: "{edge / 2.0}",
                width: "{vw - edge}", height: "{vh - edge}",
                rx: "{r}",
                fill: "url(#{id})",
                stroke: "{k.border.css()}",
                stroke_width: "{edge}",
            }
            // The highlight row just inside the top edge.
            rect {
                x: "{vw * 0.1}", y: "{edge + vh * 0.01}",
                width: "{vw * 0.8}", height: "{vh * 0.05}",
                fill: "{k.face.shade(0.22).css()}",
                fill_opacity: "0.9",
            }
            text {
                x: "{vw * 0.5}",
                // Above the geometric centre, deliberately. The source
                // glyph occupies rows 6..13 of a 20-row cell — centred on
                // 9.5, not 10 — and `dominant-baseline: central` centres on
                // the font's own middle, which put it a further half-pixel
                // down. Sitting it at 0.54 rendered a full row low in every
                // one of mute, solo and FX.
                y: "{vh * 0.49}",
                text_anchor: "middle", dominant_baseline: "central",
                font_family: "Fira Sans, DejaVu Sans, sans-serif",
                // Heavier and larger than the measured glyph height: at
                // 21x20 a normal-weight 9px letter rasterises thin and
                // grey, where the original is crisp and solid. Matching
                // the *measured* size gave a lighter button than the
                // original, which is the trap in measuring geometry
                // without checking how it renders.
                font_weight: "900",
                font_size: "{vh * 0.58}",
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

/// The FX-chain button.
///
/// Not a [`LabelButton`] with a different word on it, in two ways.
///
/// **Colour.** In all three source images the *face is the same dark
/// grey* — empty, active and bypassed are told apart entirely by the
/// colour of the letters (grey, white, red). Lighting the face instead
/// produced coloured slabs that read as toggles.
///
/// **Shape.** Mute and solo are symmetric: a 1px border all the way
/// round a 21x20 cell with 1px corners. FX is not. Tracing cell 0 of
/// `mcp_fx_norm` row by row gives a left edge at x=3, 2, 1, 1 … 1, 2, 3
/// and a right edge that never moves — so the corners are **chamfered**,
/// a straight two-pixel diagonal cut, and only on the left. The right
/// side runs flush off the cell with no border and no corner at all,
/// because these butt against what follows them in the strip.
///
/// The shape spans y=1..18 of a 22-tall cell, and the lit face stops at
/// y=15 — the three dark rows under it are the bottom edge, not a
/// separate shadow. Drawing rounded corners on a shorter body made a
/// button that was both too small and the wrong silhouette.
#[component]
pub fn FxButton(props: FxProps) -> Element {
    let t = Theme::default();
    // `lit: None` — the face never takes a state colour, only the pointer
    // shading every button gets.
    let k = ink(None, props.at);
    let (vw, vh) = (28.0f32, 22.0f32);

    // Neutral, like everything else on a hardware control. The source
    // letters are #9c9c9c empty, #dadada active and a desaturated #c34a54
    // bypassed — so `text_faint` (a dark blue-grey) and `text` (blue-white)
    // both read as lit indicators rather than printing on plastic, and
    // `rec` was far too bright for a chain that is switched *off*.
    let text = match props.state {
        FxChain::Empty => t.chrome.hardware_mark,
        FxChain::Active => t.chrome.hardware_mark.shade(0.35),
        FxChain::Bypassed => t.signal.mute,
    };

    // Chamfered down the left, square down the right, flush to `vw`.
    let cut = |x: f32, y: f32, h: f32, c: f32| {
        format!(
            "M {} {y} H {vw} V {} H {} L {x} {} V {} Z",
            x + c,
            y + h,
            x + c,
            y + h - c,
            y + c,
        )
    };

    let x = vw * 0.036;
    let top = vh * 0.045;
    // y=1..18 for the outer shape, y=2..15 for the face.
    let outer_h = vh * 0.818;
    let face_h = vh * 0.636;
    let chamfer = vh * 0.091;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(28)}",
            height: "{props.height.unwrap_or(22)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "fxface", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{k.face.shade(0.10).css()}" }
                    stop { offset: "1", stop_color: "{k.face.shade(-0.12).css()}" }
                }
            }
            // Edge, then the face laid over it. Stroking instead would draw
            // a line down the right, which is exactly what the source omits.
            path { d: "{cut(x, top, outer_h, chamfer)}", fill: "{k.border.css()}" }
            path {
                d: "{cut(x + vw * 0.036, top + vh * 0.045, face_h, chamfer * 0.75)}",
                fill: "url(#fxface)",
            }
            text {
                x: "{(x + vw) * 0.5}",
                // Not the geometric centre of the face. `dominant-baseline:
                // central` centres on the font's own middle, which lands
                // the cap half a pixel above where the original sits — so
                // this carries a measured nudge rather than pretending the
                // maths comes out even. The source glyphs occupy rows 6..12
                // of the cell, centred on row 9.
                y: "{vh * 0.432}",
                text_anchor: "middle", dominant_baseline: "central",
                font_family: "Fira Sans, DejaVu Sans, sans-serif",
                // Sized so the caps come out 7px in a 22px cell, matching
                // the source. At 500 weight and 0.44 they rendered 6px and
                // visibly thinner than the original — light enough that the
                // button read as disabled in every state.
                font_weight: "700",
                font_size: "{vh * 0.51}",
                letter_spacing: "{vw * 0.025}",
                fill: "{text.css()}",
                "FX"
            }
        }
    }
}

// ── FX bypass: a glowing pill ────────────────────────────────────────────

/// What the FX chain is doing, as the bypass toggle reports it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum FxBypass {
    /// No FX at all — the toggle is an affordance, not a state.
    #[default]
    Empty,
    /// Chain active.
    On,
    /// Chain bypassed.
    Off,
}

#[derive(Props, Clone, PartialEq)]
pub struct FxBypassProps {
    #[props(default)]
    pub state: FxBypass,
    /// The cell this replaces: `track_fx*_h` is 50x22 in three, `_v` 56x22.
    #[props(default = (17.0, 22.0))]
    pub cell: (f32, f32),
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// The FX bypass toggle — the lit pill beside the FX button.
///
/// This one is easy to miss entirely: it lives in the `track_*` family,
/// not `mcp_*`, so nothing that replaced the mixer artwork touched it and
/// it kept REAPER's inherited look while everything around it changed.
///
/// Three states, and the empty one is not a dimmer version of the others:
/// with no FX loaded the source shows a dark pill at rest and swaps to a
/// **plus** under the pointer, because there the control means "add"
/// rather than "toggle". Loaded, it is a lit capsule — blue running, red
/// bypassed — with a bright core and a soft halo, which is what makes it
/// read as an LED rather than as a filled rectangle.
#[component]
pub fn FxBypassToggle(props: FxBypassProps) -> Element {
    let t = Theme::default();
    let (vw, vh) = props.cell;
    let k = ink(None, props.at);

    let lit = match props.state {
        FxBypass::Empty => None,
        FxBypass::On => Some(t.chrome.accent),
        FxBypass::Off => Some(t.signal.rec),
    };
    // Empty + pointer is the "add FX" affordance, not a lit state.
    let plus = props.state == FxBypass::Empty && props.at != Interaction::Normal;

    let (cx, cy) = (vw * 0.5, vh * 0.5);
    let pw = vw * 0.30;
    let ph = vh * 0.52;
    let id = "fxled";

    rsx! {
        svg {
            width: "{props.width.unwrap_or(vw as u32)}",
            height: "{props.height.unwrap_or(vh as u32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                if let Some(c) = lit {
                    linearGradient { id: "{id}", x1: "0", y1: "0", x2: "1", y2: "0",
                        stop { offset: "0", stop_color: "{c.shade(-0.25).css()}" }
                        stop { offset: "0.4", stop_color: "{c.shade(0.30).css()}" }
                        stop { offset: "1", stop_color: "{c.shade(-0.30).css()}" }
                    }
                }
            }
            rect {
                x: "0", y: "0", width: "{vw}", height: "{vh}",
                rx: "{vh * 0.12}",
                fill: "{k.face.shade(-0.72).css()}",
            }
            if plus {
                // A plus, drawn as two bars so its weight is set here rather
                // than by whatever font happens to be installed.
                rect {
                    x: "{cx - pw * 0.75}", y: "{cy - vh * 0.055}",
                    width: "{pw * 1.5}", height: "{vh * 0.11}",
                    fill: "{t.chrome.hardware_mark.css()}",
                }
                rect {
                    x: "{cx - pw * 0.22}", y: "{cy - ph * 0.5}",
                    width: "{pw * 0.44}", height: "{ph}",
                    fill: "{t.chrome.hardware_mark.css()}",
                }
            } else if let Some(c) = lit {
                // The halo: what separates an LED from a coloured rectangle.
                rect {
                    x: "{cx - pw * 0.85}", y: "{cy - ph * 0.62}",
                    width: "{pw * 1.7}", height: "{ph * 1.24}",
                    rx: "{pw * 0.85}",
                    fill: "{c.css()}", fill_opacity: "0.22",
                }
                rect {
                    x: "{cx - pw * 0.5}", y: "{cy - ph * 0.5}",
                    width: "{pw}", height: "{ph}",
                    rx: "{pw * 0.5}",
                    fill: "url(#{id})",
                }
            } else {
                rect {
                    x: "{cx - pw * 0.5}", y: "{cy - ph * 0.5}",
                    width: "{pw}", height: "{ph}",
                    rx: "{pw * 0.5}",
                    fill: "{k.face.shade(-0.25).css()}",
                }
            }
        }
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

    // Measured off `mcp_recarm_on`, cell 0 (36x24), row 12: the ring spans
    // x=10..24 with a 7px hole at x=14..20, centred on x=17.5. So the cell
    // is 3:2 rather than square — drawing it in a square box, as this did
    // first, made the ring too large and the hole too wide.
    let (vw, vh) = (36.0f32, 24.0f32);
    // Traced, in *edge* coordinates rather than pixel indices: the ring
    // covers columns 10..24, which is the span [10, 25), so it is centred
    // on 17.5 with radius 7.5 — and rows 5..19, centred on 12.5. Reading
    // the indices directly gives 17 and 12 and puts the whole control half
    // a pixel up and to the left.
    let (cx, cy) = (17.5, 12.5);
    let outer = 7.5f32;
    let hole = 3.5f32;
    // A stroked circle: radius sits mid-band, width is the band.
    let band = outer - hole;
    let r = hole + band / 2.0;

    // Neutral when unarmed — the source ring is #a6a6a6, which is
    // `hardware_mark`. It was `text_dim`, a blue-grey that made a disarmed
    // track look faintly lit.
    let ring = if armed {
        t.signal.rec
    } else {
        t.chrome.hardware_mark
    };
    let ring = match props.at {
        Interaction::Normal => ring,
        Interaction::Hover => ring.shade(0.15),
        Interaction::Pressed => ring.shade(-0.12),
    };
    // The hole is not a window onto the surface behind — it is the housing
    // showing through, and in the source both are the same #262626. Filling
    // it from `surface` punched a blue-black hole through the middle.
    let hole_fill = t.chrome.hardware.shade(-0.40);

    rsx! {
        svg {
            width: "{props.width.unwrap_or(36)}",
            height: "{props.height.unwrap_or(24)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            // The housing the ring sits in. Drawing the ring alone left it
            // floating: in the original this shape is what seats the button
            // in the strip.
            //
            // Traced off `mcp_recarm_on` by **sub-pixel coverage**, not by
            // thresholding the alpha. Down column 6 the alpha runs 49, 101,
            // 153, 205, 244 from y=8 to y=12 — an edge still creeping
            // outward at about a fifth of a pixel per row, where a
            // threshold reports a hard vertical line at x=6 for six rows
            // running. Every earlier reading of this shape was built on
            // that phantom straight edge.
            //
            // What it actually is: a circle of radius 11.5 centred on
            // (17.5, 12.3) — concentric with the ring, give or take — that
            // simply goes flat near its widest point, sitting on a base
            // whose top corners are 45° flares. No vertical section at all.
            //
            // Two earlier fits both assumed one, and neither could land
            // the corner: an ellipse reached full width by y=6, a chamfer
            // was too narrow through the middle.
            circle {
                cx: "{cx}", cy: "{vh * 0.5125}", r: "{vw * 0.3194}",
                fill: "{hole_fill.css()}",
            }
            path {
                d: "M {cx - vw * 0.3194} {vh * 0.592}
                    L {cx - vw * 0.4028} {vh * 0.717} V {vh}
                    H {cx + vw * 0.4028} V {vh * 0.717}
                    L {cx + vw * 0.3194} {vh * 0.592} Z",
                fill: "{hole_fill.css()}",
            }
            // Auto is a **solid disc with the A knocked out of it**, not a
            // ring with a letter laid inside. Drawing both gave a grey
            // annulus with a second grey A floating in its hole — two
            // marks where the source has one.
            if auto {
                circle {
                    cx: "{cx}", cy: "{cy}", r: "{outer}",
                    fill: "{ring.css()}",
                }
                text {
                    x: "{cx}", y: "{cy}",
                    text_anchor: "middle", dominant_baseline: "central",
                    font_family: "Fira Sans, DejaVu Sans, sans-serif",
                    font_weight: "700", font_size: "{outer * 1.6}",
                    // The housing colour, so the glyph is a hole through
                    // the disc rather than ink on top of it.
                    fill: "{hole_fill.css()}",
                    "A"
                }
            } else {
                circle {
                    cx: "{cx}", cy: "{cy}", r: "{r}",
                    fill: "{hole_fill.css()}",
                    stroke: "{ring.css()}",
                    stroke_width: "{band}",
                }
            }
            if barred && !auto {
                // Four radial notches, not two crossing lines: the original
                // reads as a life-ring, with the cuts running right through
                // the band to the outer edge.
                for (i, deg) in [45.0f32, 135.0, 225.0, 315.0].iter().enumerate() {
                    {
                        let a = deg.to_radians();
                        let (dx, dy) = (a.cos(), a.sin());
                        rsx! {
                            line {
                                key: "{i}",
                                x1: "{cx + dx * (hole - 0.5)}",
                                y1: "{cy + dy * (hole - 0.5)}",
                                x2: "{cx + dx * (outer + 0.8)}",
                                y2: "{cy + dy * (outer + 0.8)}",
                                stroke: "{hole_fill.css()}",
                                stroke_width: "{band * 0.85}",
                                stroke_linecap: "butt",
                            }
                        }
                    }
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
    // `mcp_io_s_r` is 71x32 in three cells — so each is about 23x32,
    // taller than wide. Drawing it square, as this did first, squashed the
    // bars together and lost the stacked-lane read entirely.
    let (vw, vh) = (23.0f32, 32.0f32);

    // Three lanes, not two: the original stacks the track's own output,
    // its sends and its receives. Two bars cannot express what the
    // control reports.
    //
    // Read off the three source cells: the top bar is blue in all of
    // them — a track always has an output — and only the lower two light
    // up. Colouring the top bar conditionally, as this first did, made
    // an unrouted track look broken rather than merely unrouted.
    //
    // An unlit lane is #6c6c6c in the source — plain grey. It was
    // `text_faint`, a blue-grey, so a track with nothing routed looked
    // faintly lit rather than dark.
    let dim = t.chrome.hardware_mark.shade(-0.33);
    let out = t.chrome.accent;
    let send = if props.has_sends { t.signal.meter_warn } else { dim };
    let recv = if props.has_receives { t.signal.rec } else { dim };
    let opacity = if props.disabled { "0.4" } else { "1" };

    // Lanes at y=6, 13 and 20 of 32, each 4 tall.
    let bar_w = vw * 0.56;
    let bar_h = vh * 0.125;
    let bar_x = (vw - bar_w) / 2.0;
    let r = bar_h / 2.0;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(23)}",
            height: "{props.height.unwrap_or(32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            opacity: "{opacity}",
            rect {
                x: "{vw * 0.06}", y: "{vh * 0.06}",
                width: "{vw * 0.88}", height: "{vh * 0.88}",
                rx: "{vw * 0.16}",
                fill: "{k.face.css()}",
                stroke: "{k.border.css()}", stroke_width: "{vh * 0.03}",
            }
            for (i, colour) in [out, send, recv].iter().enumerate() {
                rect {
                    key: "{i}",
                    x: "{bar_x}",
                    y: "{vh * (0.1875 + 0.219 * i as f32)}",
                    width: "{bar_w}", height: "{bar_h}",
                    rx: "{r}",
                    fill: "{colour.css()}",
                }
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
    // 21x20, the same cell as mute and solo — `mcp_monitor_*` is 63x20.
    let (vw, vh) = (21.0f32, 20.0f32);

    // The original is a source radiating *downward*: a filled dot near the
    // top with two arcs opening upward beneath it. This first drew arcs
    // above a dot at the bottom, which is the same parts assembled into a
    // different icon — it read as wifi, not as a monitored input.
    let colour = match props.state {
        Monitoring::Off => t.chrome.text_dim,
        Monitoring::On => t.chrome.text,
        Monitoring::Auto => t.signal.rec,
    };
    let colour = match props.at {
        Interaction::Normal => colour,
        Interaction::Hover => colour.shade(0.18),
        Interaction::Pressed => colour.shade(-0.12),
    };

    let cx = vw * 0.5;
    let cy = vh * 0.24;
    let sw = vh * 0.10;
    let dot = vh * 0.09;

    // Arc endpoints at ±55° either side of straight down.
    let arc = |r: f32| {
        let (dx, dy) = (35.0f32.to_radians().cos(), 35.0f32.to_radians().sin());
        format!(
            "M {} {} A {r} {r} 0 0 1 {} {}",
            cx + dx * r,
            cy + dy * r,
            cx - dx * r,
            cy + dy * r,
        )
    };

    rsx! {
        svg {
            width: "{props.width.unwrap_or(21)}",
            height: "{props.height.unwrap_or(20)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            circle { cx: "{cx}", cy: "{cy}", r: "{dot}", fill: "{colour.css()}" }
            for (i, rad) in [vh * 0.26, vh * 0.44].iter().enumerate() {
                path {
                    key: "{i}",
                    d: "{arc(*rad)}",
                    fill: "none",
                    stroke: "{colour.css()}",
                    stroke_width: "{sw}",
                    stroke_linecap: "round",
                }
            }
            if matches!(props.state, Monitoring::Off) {
                // Struck through corner to corner. The dark casing under it
                // is what makes the slash read as *cutting* the arcs rather
                // than sitting on top of them at 20px.
                line {
                    x1: "{vw * 0.12}", y1: "{vh * 0.88}",
                    x2: "{vw * 0.88}", y2: "{vh * 0.12}",
                    stroke: "{t.chrome.surface.css()}",
                    stroke_width: "{sw * 2.4}",
                    stroke_linecap: "round",
                }
                line {
                    x1: "{vw * 0.12}", y1: "{vh * 0.88}",
                    x2: "{vw * 0.88}", y2: "{vh * 0.12}",
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
    // `mcp_pan_knob_small` is 24x25 — a hair taller than wide.
    let (vw, vh) = (24.0f32, 25.0f32);
    let (cx, cy) = (vw * 0.5, vh * 0.5);
    let r = vw * (if props.large { 0.50 } else { 0.44 });

    // Measured off the source: a plain dark disc with a soft light dot,
    // and at rest the dot is dead centre — not offset, and with no
    // pointer line anywhere. The first version drew a line to the rim,
    // which is a different control: this knob shows pan by *sliding* the
    // dot across, so centre reads as centre.
    let pos = props.position.clamp(-1.0, 1.0);
    let dot_r = r * 0.42;
    let travel = r - dot_r - vw * 0.04;
    let dx = pos * travel;

    // Both neutral. The dot was `text_dim`, which is a light *blue*-grey
    // — right for a label on a panel, wrong for a moulded marker on a
    // knob, where it read as a lit indicator rather than plastic.
    let disc = t.chrome.hardware.shade(-0.09);
    let dot = if props.position == 0.0 {
        t.chrome.hardware_mark
    } else {
        t.chrome.accent
    };

    rsx! {
        svg {
            width: "{props.width.unwrap_or(24)}",
            height: "{props.height.unwrap_or(25)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                radialGradient { id: "panface", cx: "0.5", cy: "0.35", r: "0.75",
                    stop { offset: "0", stop_color: "{disc.shade(0.22).css()}" }
                    stop { offset: "1", stop_color: "{disc.shade(-0.25).css()}" }
                }
            }
            circle { cx: "{cx}", cy: "{cy}", r: "{r}", fill: "url(#panface)" }
            circle {
                cx: "{cx + dx}", cy: "{cy}", r: "{dot_r}",
                fill: "{dot.css()}",
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
    let (vw, vh) = (27.0f32, 53.0f32);
    // Six ridges per half, matching the source. Deriving the count from
    // height would thin them at one size and crowd them at another, which
    // is exactly what drawing this as vector is meant to stop.
    const RIBS: usize = 6;

    // The grip is light grey plastic in the original, and only REAPER's
    // colour variants tint it. Painting the panel with the theme accent by
    // default, as this first did, turned every fader into a coloured slab
    // and lost the ribbed-plastic read entirely.
    let grip = props.accent.unwrap_or(t.chrome.hardware_mark.shade(0.22));
    let body = t.chrome.hardware.shade(-0.25);

    let (px, pw) = (vw * 0.26, vw * 0.48);
    // Two halves either side of a dark seam across the middle.
    let halves = [(vh * 0.14, vh * 0.34), (vh * 0.52, vh * 0.34)];

    rsx! {
        svg {
            width: "{props.width.unwrap_or(27)}",
            height: "{props.height.unwrap_or(53)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "capbody", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{body.shade(0.25).css()}" }
                    stop { offset: "0.5", stop_color: "{body.css()}" }
                    stop { offset: "1", stop_color: "{body.shade(-0.35).css()}" }
                }
                linearGradient { id: "capgrip", x1: "0", y1: "0", x2: "1", y2: "0",
                    stop { offset: "0", stop_color: "{grip.shade(-0.18).css()}" }
                    stop { offset: "0.35", stop_color: "{grip.shade(0.12).css()}" }
                    stop { offset: "1", stop_color: "{grip.shade(-0.25).css()}" }
                }
            }
            rect {
                x: "{vw * 0.07}", y: "{vh * 0.03}",
                width: "{vw * 0.86}", height: "{vh * 0.94}",
                rx: "{vw * 0.26}",
                fill: "url(#capbody)",
                stroke: "{t.chrome.surface_deep().css()}",
                stroke_width: "{vw * 0.06}",
            }
            for (i, (y, h)) in halves.iter().enumerate() {
                rect {
                    key: "h{i}",
                    x: "{px}", y: "{y}", width: "{pw}", height: "{h}",
                    rx: "{vw * 0.09}",
                    fill: "url(#capgrip)",
                }
            }
            // The ridges: dark lines cut across each grip half.
            for (hi, (y, h)) in halves.iter().enumerate() {
                for i in 0..RIBS {
                    {
                        let step = h / RIBS as f32;
                        rsx! {
                            rect {
                                key: "{hi}-{i}",
                                x: "{px + vw * 0.03}",
                                y: "{y + step * (i as f32 + 0.5)}",
                                width: "{pw - vw * 0.06}",
                                height: "{vh * 0.018}",
                                fill: "{grip.shade(-0.55).css()}",
                            }
                        }
                    }
                }
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

    /// A control's own size, as declared by the SVG it renders.
    fn intrinsic(svg: &str) -> (f32, f32) {
        let opts = resvg::usvg::Options::default();
        let tree = resvg::usvg::Tree::from_str(svg, &opts).expect("valid svg");
        (tree.size().width(), tree.size().height())
    }

    /// Every control draws at the aspect of the art it replaces.
    ///
    /// FX shared mute's 21x20 for a while, when `mcp_fx_*` cells are 28x22.
    /// Nothing failed: the button simply got stretched into its cell by
    /// whatever drew it, which looks like a rendering bug and is in fact a
    /// wrong `viewBox`. The cell sizes come from the compiled-in art index,
    /// so this checks against the real images rather than repeating numbers
    /// that could go stale in both places at once.
    #[test]
    fn every_control_is_shaped_like_the_cell_it_replaces() {
        let n = (None, None);
        let cases: [(&str, String); 7] = [
            ("mcp_recarm_on", render_svg(RecordArmButton, RecordArmProps { state: RecordArm::On, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_mute_on", render_svg(MuteButton, ToggleProps { on: true, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_solo_on", render_svg(SoloButton, SoloProps { state: Solo::On, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_fx_norm", render_svg(FxButton, FxProps { state: FxChain::Active, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_io_s_r", render_svg(RoutingButton, RoutingProps { has_sends: true, has_receives: true, disabled: false, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_monitor_on", render_svg(InputMonitorIndicator, MonitoringProps { state: Monitoring::On, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_volthumb", render_svg(VolumeFaderCap, FaderCapProps { accent: None, width: n.0, height: n.1 })),
        ];

        for (name, svg) in &cases {
            let art = crate::generated::by_name(name)
                .unwrap_or_else(|| panic!("no art index entry for {name}"));
            let cell_w = art.width as f32 / art.cells.max(1) as f32;
            let cell_h = art.height as f32;
            let (vw, vh) = intrinsic(svg);

            // Cell widths are not always whole (86/3), so compare aspect
            // with a tolerance rather than demanding exact pixels.
            let want = cell_w / cell_h;
            let got = vw / vh;
            assert!(
                (want - got).abs() < 0.06,
                "{name}: cell is {cell_w}x{cell_h} (aspect {want:.3}) \
                 but the vector is {vw}x{vh} (aspect {got:.3})",
            );
        }
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
        // The grip is a gradient of shaded variants, so the accent never
        // appears verbatim — asserting on its exact hex only passed while
        // the panel was a flat fill, and would fail any shading change
        // without anything actually being wrong.
        assert!(svg.contains(&green.shade(0.12).to_hex()), "{svg}");
        assert!(svg.contains(&green.shade(-0.55).to_hex()), "ribs: {svg}");

        let plain = render_svg(
            VolumeFaderCap,
            FaderCapProps {
                accent: None,
                width: None,
                height: None,
            },
        );
        assert_ne!(svg, plain, "the accent made no difference");
    }
}
