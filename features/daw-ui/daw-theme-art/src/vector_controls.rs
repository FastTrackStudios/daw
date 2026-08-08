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

/// Which way a control is laid out.
///
/// The track panel and the mixer draw the *same* controls along different
/// axes — routing stacks its three lanes in the mixer and sets them side
/// by side in the track panel; input monitoring radiates downward there
/// and rightward here. Same geometry, turned a quarter turn, so it is a
/// prop rather than a second component.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Axis {
    /// Mixer: lanes stacked, waves radiating down.
    #[default]
    Vertical,
    /// Track panel: lanes in a row, waves radiating right.
    Horizontal,
}

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

/// Deepen a colour without dimming it.
///
/// Holds the strongest channel and pulls the others down, which is what
/// ReaperTips' lit buttons actually do from top to bottom: solo runs
/// #d3a738 to #d0943a — red pinned at 210, green falling 167 to 148 —
/// and defeat holds its blue at 211 while red drops away.
///
/// `shade` cannot express that. It mixes toward black, so it scales all
/// three channels together and takes the dominant one with it: the face
/// loses chroma as it descends and the middle of the button reads dull
/// against the original, which is exactly how this was first noticed.
fn deepen(c: Color, amount: f32) -> Color {
    let peak = c.r.max(c.g).max(c.b);
    // A grey has no non-dominant channel to pull, so holding the peak
    // would leave it untouched — which silently cost every *unlit* button
    // both its gradient and its pressed state, since those faces are
    // neutral. There, deepening is just darkening.
    if peak == c.r.min(c.g).min(c.b) {
        return c.shade(-amount);
    }
    let pull = |v: u8| {
        if v == peak {
            v
        } else {
            (v as f32 * (1.0 - amount)).round().clamp(0.0, 255.0) as u8
        }
    };
    Color::rgb(pull(c.r), pull(c.g), pull(c.b))
}

/// Brighten a colour by scaling its channels, clamping at the top.
///
/// The counterpart to [`deepen`], and what a hover is here: solo goes
/// #d29e37 to #ffdb59 under the pointer — the same colour turned up, red
/// running into the ceiling while green and blue climb.
///
/// Three models were measured against the art. A wash toward white
/// (`shade`) moves it a fifth as far and leaves every lit button barely
/// changed when hovered. An HSL lightness lift looks right on paper —
/// the source's three buttons all rise about 14 points — but holding
/// saturation while lightness climbs desaturates in RGB, and defeat's
/// hover *gains* saturation in the source. Scaling the channels lands
/// solo and defeat within a few points each; mute's red runs about 8%
/// hot, which is the price of one rule for three buttons.
fn lift(c: Color, amount: f32) -> Color {
    let up = |v: u8| (v as f32 * (1.0 + amount)).round().clamp(0.0, 255.0) as u8;
    Color::rgb(up(c.r), up(c.g), up(c.b))
}

/// A control's palette, resolved once per render.
struct Ink {
    face: Color,
    border: Color,
    text: Color,
}

/// `sinks` — does pressing darken the face?
///
/// In the mixer it does: `mcp_mute_off` goes #3f3f3f to #363636. In the
/// track panel it does *not* — `track_mute_off` shows #494949 in both
/// cells, identical. Assuming either way invents a state one of the two
/// families does not have.
fn ink(lit: Option<Color>, at: Interaction, sinks: bool, hover: f32) -> Ink {
    let t = Theme::default();
    let c = &t.chrome;
    // Unlit controls take the neutral control grey, not a shade of the
    // surface ladder: deriving it from `surface_raised` made every mute,
    // solo and FX button blue-cast and far darker than the art it stands
    // in for.
    let base = lit.unwrap_or(c.hardware);
    // Hover is far stronger than the 12% wash this used: solo lifts to
    // #ffdb59, with red clipping at the ceiling.
    //
    // How far is per button, because the source's is. Measured as the
    // per-channel ratio from normal to hover:
    //
    //     mute    x1.25  x1.56  x1.52
    //     solo    x1.21  x1.39  x1.62
    //     defeat  x1.85  x1.43  x1.21
    //
    // Mute's is the gentlest and the flattest; a single amount tuned to
    // solo drove mute's red to 238 against the source's 220.
    let face = match at {
        Interaction::Normal => base,
        Interaction::Hover => lift(base, hover),
        Interaction::Pressed if sinks => deepen(base, 0.12),
        Interaction::Pressed => base,
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
    /// Where the button sits in its cell: `(top, height)` as fractions.
    ///
    /// The mixer's buttons fill their cell; the track panel's are inset a
    /// row at the top and three at the bottom — `track_mute_on` draws rows
    /// 1..20 of 24, and the magenta guide runs to row 20 to say so.
    /// Filling the cell there pushed the button past its own guide.
    #[props(default = (0.0, 1.0))]
    pub body: (f32, f32),
    pub label: String,
    /// The printed letter's colour.
    ///
    /// Brighter in the track panel than in the mixer — #f2f2f2 against
    /// #cccccc — which is not a shade of one value but two different
    /// choices, so it comes from the caller rather than from `ink`.
    #[props(default)]
    pub legend: Option<Color>,
    /// Does pressing darken the face? See [`ink`].
    #[props(default = true)]
    pub sinks: bool,
    /// How far the face lifts under the pointer — see [`ink`].
    #[props(default = 0.35)]
    pub hover: f32,
    /// How far the face deepens from top to bottom — see [`deepen`].
    ///
    /// Not shared, because the source does not share it: mute falls 15%
    /// in its non-dominant channels over the button's height, solo 11%,
    /// and blue-defeat more still.
    #[props(default = 0.15)]
    pub depth: f32,
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
    let k = ink(props.lit, props.at, props.sinks, props.hover);
    let (vw, vh) = props.cell;
    let (body_y, body_h) = (vh * props.body.0, vh * props.body.1);
    let id = format!("lb{}", props.label.replace(' ', ""));
    // The radius was never the problem — the stroke was.
    //
    // With a stroked border the corner looked far too round, and shrinking
    // the radius to a pixel was the obvious fix. It was the wrong one: a
    // stroke covers only half its path, so what actually showed at the
    // corner was the *face* bleeding past it. With a filled frame the
    // original 0.10 of height lands within a few points of the source at
    // every corner pixel.
    let r = vh * 0.10;
    // Exactly one pixel, offset half of one, so the border lands inside a
    // single row rather than straddling two. At `vh * 0.05` it was 1.2px
    // centred on an integer: the bottom row came out a blend of border
    // and face — #832d3b where the source still has body colour — and the
    // whole button read a row short.
    let edge = 1.0f32;

    rsx! {
        svg {
            width: "{props.width.unwrap_or(vw as u32)}",
            height: "{props.height.unwrap_or(vh as u32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "{id}", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{k.face.shade(0.06).css()}" }
                    stop { offset: "1", stop_color: "{deepen(k.face, props.depth).css()}" }
                }
            }
            // Inset by half the border so the stroke sits inside.
            // Border as a *filled* shape with the face inset on top of
            // it, not a stroke around the face.
            //
            // A stroke is centred on the path it follows, so it covers
            // only half of the fill's antialiased edge — and at a rounded
            // corner the rest of that edge shows past it. The corner
            // pixels came out a blend of border and face (R=54 where the
            // source is a clean R=23 at reduced alpha), which reads as
            // the colour leaking out of the corner rather than being held
            // by the frame. Filled and inset, the face cannot reach the
            // outside: there is a solid pixel of border in the way.
            rect {
                x: "0", y: "{body_y}",
                width: "{vw}", height: "{body_h}",
                rx: "{r}",
                fill: "{k.border.css()}",
            }
            rect {
                x: "{edge}", y: "{body_y + edge}",
                width: "{vw - edge * 2.0}", height: "{body_h - edge * 2.0}",
                rx: "{(r - edge).max(0.0)}",
                fill: "url(#{id})",
            }
            // The lit row immediately inside the top border — exactly one
            // row, at y=2 of the source's 1..20 button. A 1.2px band
            // starting at 2.24 spilled into row 3 and left row 2 dark,
            // which put the highlight a row low.
            rect {
                x: "{vw * 0.1}", y: "{body_y + 1.0}",
                width: "{vw * 0.8}", height: "1.0",
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
                y: "{body_y + body_h * 0.49}",
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
                fill: "{props.legend.unwrap_or(k.text).css()}",
                "{props.label}"
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct ToggleProps {
    /// Does pressing darken the face? See [`ink`].
    #[props(default = true)]
    pub sinks: bool,
    /// How far the face lifts under the pointer — see [`ink`].
    #[props(default = 0.35)]
    pub hover: f32,
    /// How far the face darkens — see [`LabelButtonProps::depth`].
    #[props(default = 0.14)]
    pub depth: f32,
    /// The printed letter's colour — see [`LabelButtonProps::legend`].
    #[props(default)]
    pub legend: Option<Color>,
    /// Where the button sits in its cell: `(top, height)` as fractions.
    ///
    /// The mixer's buttons fill their cell; the track panel's are inset a
    /// row at the top and three at the bottom — `track_mute_on` draws rows
    /// 1..20 of 24, and the magenta guide runs to row 20 to say so.
    /// Filling the cell there pushed the button past its own guide.
    #[props(default = (0.0, 1.0))]
    pub body: (f32, f32),
    #[props(default)]
    pub on: bool,
    /// The cell this replaces: `mcp_mute_*` is 21x20, `track_mute_*` 22x24.
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

#[component]
pub fn MuteButton(props: ToggleProps) -> Element {
    let t = Theme::default();
    rsx! {
        LabelButton {
            label: "M",
            lit: props.on.then_some(t.signal.mute),
            cell: props.cell, body: props.body, legend: props.legend,
            depth: props.depth, sinks: props.sinks, hover: props.hover,
            width: props.width, height: props.height, at: props.at,
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct SoloProps {
    /// Does pressing darken the face? See [`ink`].
    #[props(default = true)]
    pub sinks: bool,
    /// How far the face lifts under the pointer — see [`ink`].
    #[props(default = 0.35)]
    pub hover: f32,
    /// How far the face darkens — see [`LabelButtonProps::depth`].
    #[props(default = 0.14)]
    pub depth: f32,
    /// The printed letter's colour — see [`LabelButtonProps::legend`].
    #[props(default)]
    pub legend: Option<Color>,
    /// Where the button sits in its cell: `(top, height)` as fractions.
    ///
    /// The mixer's buttons fill their cell; the track panel's are inset a
    /// row at the top and three at the bottom — `track_mute_on` draws rows
    /// 1..20 of 24, and the magenta guide runs to row 20 to say so.
    /// Filling the cell there pushed the button past its own guide.
    #[props(default = (0.0, 1.0))]
    pub body: (f32, f32),
    #[props(default)]
    pub state: Solo,
    /// The cell this replaces: `mcp_mute_*` is 21x20, `track_mute_*` 22x24.
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

#[component]
pub fn SoloButton(props: SoloProps) -> Element {
    let t = Theme::default();
    // Defeat is a different thing from solo, not more of it, so it gets a
    // different hue rather than a brighter one.
    let lit = match props.state {
        Solo::Off => None,
        Solo::On => Some(t.signal.solo),
        // #3898d3 in the source — its own blue, a clear step below the
        // accent used for a lit routing lane rather than the same one.
        // At -0.22 it lost too much blue (198 against 211).
        Solo::Defeat => Some(t.chrome.accent.shade(-0.18)),
    };
    rsx! {
        LabelButton {
            label: "S", lit, cell: props.cell, body: props.body,
            legend: props.legend, depth: props.depth, sinks: props.sinks,
            hover: props.hover,
            width: props.width, height: props.height, at: props.at,
        }
    }
}

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
pub struct FxProps {
    #[props(default)]
    pub state: FxChain,
    #[props(default)]
    pub family: FxFamily,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// The labelled half of [`FxControl`], as its own image.
#[component]
pub fn FxButton(props: FxProps) -> Element {
    rsx! {
        FxControl {
            chain: props.state,
            family: props.family,
            part: FxPart::Label,
            width: props.width,
            height: props.height,
            at: props.at,
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct FxBypassProps {
    #[props(default)]
    pub state: FxBypass,
    #[props(default)]
    pub family: FxFamily,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// The lit half of [`FxControl`], as its own image.
#[component]
pub fn FxBypassToggle(props: FxBypassProps) -> Element {
    rsx! {
        FxControl {
            bypass: props.state,
            family: props.family,
            part: FxPart::Toggle,
            width: props.width,
            height: props.height,
            at: props.at,
        }
    }
}

// ── FX: one pill, blitted as two images ──────────────────────────────────

/// Which panel's FX control this is.
///
/// The two are the same control at different sizes with different
/// materials, and every dimension differs, so the table below is the one
/// place those numbers live.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum FxFamily {
    /// `mcp_fx_*` + `track_fx*_v`: opaque plastic, 28 + 18 wide.
    #[default]
    Mixer,
    /// `track_fx_*` + `track_fx*_h`: a translucent scrim, 20 + 16 wide.
    TrackPanel,
}

/// Which half of the pill an image wants.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum FxPart {
    /// The labelled end — the `FX` button.
    #[default]
    Label,
    /// The lit end — the bypass toggle.
    Toggle,
}

/// The measured geometry of one family's FX control.
struct Pill {
    /// Whole pill, both halves.
    w: f32,
    h: f32,
    /// Where the toggle begins.
    split: f32,
    /// Body top and height, as fractions of `h`.
    body: (f32, f32),
    /// Translucent over the strip rather than opaque.
    scrim: bool,
}

impl FxFamily {
    fn pill(self) -> Pill {
        match self {
            // `mcp_fx_norm` draws a 28-wide cell and `track_fxempty_v` an
            // 18-wide one, rows 1..18 of 22 in both.
            Self::Mixer => Pill {
                w: 46.0,
                h: 22.0,
                split: 28.0,
                body: (1.0 / 22.0, 18.0 / 22.0),
                scrim: false,
            },
            // `track_fx_norm` is 20 wide and `track_fxempty_h` 16, rows
            // 1..20 in both.
            Self::TrackPanel => Pill {
                w: 36.0,
                h: 22.0,
                split: 20.0,
                body: (1.0 / 22.0, 20.0 / 22.0),
                scrim: true,
            },
        }
    }

    /// `(x, width)` of `part` within the pill.
    fn window(self, part: FxPart) -> (f32, f32) {
        let p = self.pill();
        match part {
            FxPart::Label => (0.0, p.split),
            FxPart::Toggle => (p.split, p.w - p.split),
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct FxControlProps {
    /// What the chain is doing — drives the label's colour.
    #[props(default)]
    pub chain: FxChain,
    /// What the toggle is showing.
    #[props(default)]
    pub bypass: FxBypass,
    #[props(default)]
    pub family: FxFamily,
    /// Which half to emit.
    #[props(default)]
    pub part: FxPart,
    #[props(default)]
    pub width: Option<u32>,
    #[props(default)]
    pub height: Option<u32>,
    /// Pointer state — hover lifts the face, pressed sinks it.
    #[props(default)]
    pub at: Interaction,
}

/// The FX control: a labelled button and a lit toggle, as **one pill**.
///
/// REAPER blits these as two images, which is why they were built as two
/// components — and every seam between them then had to be maintained by
/// hand. The toggle's inner corners had to be squared because the button
/// runs flush there; the edge down that side had to be suppressed because
/// there is no edge in the middle of a shape; the two had to be given the
/// same body rows and the same material. Each was found separately, after
/// looking wrong.
///
/// Drawing it as one shape and *windowing* the viewBox for each image
/// makes those relationships structural: the pill is rounded at both outer
/// ends and continuous through the middle because it is one path, and the
/// two images cannot disagree about height or material because neither
/// knows it is an image.
#[component]
pub fn FxControl(props: FxControlProps) -> Element {
    let t = Theme::default();
    let k = ink(None, props.at, true, 0.35);
    let p = props.family.pill();
    let (win_x, win_w) = props.family.window(props.part);

    let (body_y, body_h) = (p.h * p.body.0, p.h * p.body.1);
    // Exactly one pixel, on the pixel grid.
    //
    // The viewBox is in the source's own pixels, so `1.0` here is one
    // output pixel — and offsetting the path by half of it puts the
    // stroke inside a single column instead of straddling two. At
    // `p.h * 0.05` it was 1.1px centred on an integer, which antialiases
    // across two columns and reads as a soft gradient rather than an
    // edge.
    let edge = 1.0f32;
    let r = p.h * 0.12;

    // One outline for the whole pill: rounded at both outer ends, and
    // nothing at all in the middle.
    let (x, y) = (edge / 2.0, body_y + edge / 2.0);
    let (w, h) = (p.w - edge, body_h - edge);
    let outline = format!(
        "M {} {y} H {} A {r} {r} 0 0 1 {} {} V {} A {r} {r} 0 0 1 {} {}          H {} A {r} {r} 0 0 1 {x} {} V {} A {r} {r} 0 0 1 {} {y} Z",
        x + r,
        x + w - r,
        x + w,
        y + r,
        y + h - r,
        x + w - r,
        y + h,
        x + r,
        y + h - r,
        y + r,
        x + r,
    );

    let (fill, alpha) = if p.scrim {
        ("#000000".to_string(), 0.35)
    } else {
        ("url(#fxface)".to_string(), 1.0)
    };

    // Neutral, like everything else printed on a hardware control. The
    // source letters are #9c9c9c empty, #dadada active and a desaturated
    // #c34a54 bypassed — `text_faint` and `text` are the chrome ramp's
    // blues and read as lit indicators rather than print on plastic.
    let text = match props.chain {
        FxChain::Empty => t.chrome.hardware_mark,
        FxChain::Active => t.chrome.hardware_mark.shade(0.35),
        FxChain::Bypassed => t.signal.mute,
    };

    // The toggle's lamp, in pill coordinates.
    let lamp = match props.bypass {
        FxBypass::Empty => None,
        FxBypass::On => Some(t.chrome.accent),
        FxBypass::Off => Some(t.signal.meter_danger),
    };
    let plus = props.bypass == FxBypass::Empty && props.at != Interaction::Normal;
    // 8px from the toggle's left edge, not centred in it. The lens sits
    // at the same offset in both families — which *is* centred in the
    // 16-wide half and a pixel left of centre in the 18-wide one, so
    // centring it put the mixer's a pixel too far right.
    let (tx, ty) = (p.split + p.h * 0.364, body_y + body_h * 0.5);
    // Measured: a 4px pill and an 8x8 plus with 2px arms, in both
    // families — so neither scales with the cell width.
    let (pw, ph) = (p.h * 0.182, body_h * 0.5);
    let (arm, bar) = (p.h * 0.364, p.h * 0.091);

    rsx! {
        svg {
            width: "{props.width.unwrap_or(win_w as u32)}",
            height: "{props.height.unwrap_or(p.h as u32)}",
            // The window is the only thing that differs between the two
            // images: same drawing, different slice of it.
            view_box: "{win_x} 0 {win_w} {p.h}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "fxface", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{k.face.shade(0.19).css()}" }
                    stop { offset: "1", stop_color: "{k.face.shade(0.02).css()}" }
                }
            }
            path { d: "{outline}", fill: "{fill}", fill_opacity: "{alpha}" }
            path {
                d: "{outline}",
                fill: "none",
                stroke: "{k.border.css()}",
                stroke_width: "{edge}",
            }
            // The lit row just inside the top, which every ReaperTips
            // control has — one shape, so it runs the whole pill.
            rect {
                x: "{x + r}", y: "{y + edge}",
                width: "{w - r * 2.0}", height: "{p.h * 0.045}",
                fill: "#ffffff", fill_opacity: "0.07",
            }

            text {
                x: "{p.split * 0.5}", y: "{body_y + body_h * 0.5}",
                text_anchor: "middle", dominant_baseline: "central",
                font_family: "Fira Sans, DejaVu Sans, sans-serif",
                // 0.51, not 0.44. The peak colour is right either way —
                // what differs is *coverage*: at 0.44 the source has twice
                // as many bright pixels as ours, so the letters read dim
                // rather than dark. Sized so the caps come out 7px in a
                // 22px cell, which the glyph-row test pins.
                font_weight: "700",
                font_size: "{p.h * 0.51}",
                letter_spacing: "{p.h * 0.03}",
                fill: "{text.css()}",
                "FX"
            }

            if plus {
                rect {
                    x: "{tx - arm * 0.5}", y: "{ty - bar * 0.5}",
                    width: "{arm}", height: "{bar}",
                    fill: "{t.chrome.hardware_mark.shade(0.33).css()}",
                }
                rect {
                    x: "{tx - bar * 0.5}", y: "{ty - arm * 0.5}",
                    width: "{bar}", height: "{arm}",
                    fill: "{t.chrome.hardware_mark.shade(0.33).css()}",
                }
            } else if let Some(c) = lamp {
                // The halo is what separates an LED from a coloured slab.
                rect {
                    x: "{tx - pw * 0.85}", y: "{ty - ph * 0.62}",
                    width: "{pw * 1.7}", height: "{ph * 1.24}",
                    rx: "{pw * 0.85}",
                    fill: "{c.css()}", fill_opacity: "0.22",
                }
                rect {
                    x: "{tx - pw * 0.5}", y: "{ty - ph * 0.5}",
                    width: "{pw}", height: "{ph}",
                    rx: "{pw * 0.5}",
                    fill: "{c.css()}",
                }
            } else {
                // Dormant: a *recessed* lens. Light in the middle —
                // #656565, brighter than the face — inside a ring that is
                // darker than the face again. Measured across the source
                // it goes 65, 58, 102, 102, 58, 65: without the ring it
                // reads as a flat light blob stuck on rather than a lamp
                // set into the plastic.
                // Ring behind, core on top — not a stroke, which
                // straddles the edge it is drawn on and ate half a pixel
                // off each side, leaving a 3px core where the source has
                // 4 (cols 6..9) inside a ring (cols 5 and 10).
                //
                // Only on the opaque family: against a scrim the source
                // puts the lens straight onto the black, with nothing
                // between.
                if !p.scrim {
                    rect {
                        x: "{tx - pw * 0.5 - 1.0}", y: "{ty - ph * 0.5 - 1.0}",
                        width: "{pw + 2.0}", height: "{ph + 2.0}",
                        rx: "{pw * 0.5 + 1.0}",
                        fill: "{t.chrome.hardware.shade(-0.1).css()}",
                    }
                }
                rect {
                    x: "{tx - pw * 0.5}", y: "{ty - ph * 0.5}",
                    width: "{pw}", height: "{ph}",
                    rx: "{pw * 0.5}",
                    fill: "{t.chrome.hardware_mark.shade(-0.38).css()}",
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
    /// The cell this replaces: `mcp_recarm_*` is 36x24, `track_*` 20x20.
    #[props(default = (36.0, 24.0))]
    pub cell: (f32, f32),
    /// Draw the moulded housing the ring is seated in.
    ///
    /// The mixer has one; the track panel draws a bare ring on the strip.
    /// Keeping it a prop rather than inferring it from the cell size means
    /// a narrow mixer cell still gets its housing.
    #[props(default = true)]
    pub housing: bool,
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

    let (vw, vh) = props.cell;
    let unit = vw.min(vh);

    // Traced, in *edge* coordinates rather than pixel indices. In the
    // mixer's 36x24 the ring covers columns 10..24 — the span [10, 25) —
    // so it is centred on 17.5 with radius 7.5, and rows 5..19, centred on
    // 12.5. Reading the indices directly gives 17 and 12 and puts the whole
    // control half a pixel up and to the left.
    //
    // The track panel's 20x20 ring is centred and **larger** relative to
    // its cell (radius 8 of 20) because there is no housing competing for
    // the room, so the two fractions differ rather than one being wrong.
    let (cx, cy, outer, hole) = if props.housing {
        (vw * 0.486, vh * 0.521, unit * 0.3125, unit * 0.1458)
    } else {
        // Outer exactly 0.40 — radius 8 of 20, which lands the edge on
        // the pixel boundary at x=2 and x=18 and keeps it crisp. At 0.41
        // it falls at 8.2, mid-pixel, and the whole rim antialiases into
        // a dim blur that reads as a square with soft corners rather than
        // a circle.
        //
        // The hole is radius 4, not 3. Reading it off a thresholded
        // silhouette gave 3 — the hole's edge is antialiased, so where
        // you call its boundary depends on the threshold, and a stricter
        // one moves it a pixel. Sampled properly both track variants
        // agree at 6..13 of 20.
        (vw * 0.5, vh * 0.5, unit * 0.40, unit * 0.20)
    };
    // The ring is an annulus *path*, not a stroked circle.
    //
    // A stroke took the gradient as a flat average — the whole ring came
    // out one value where the source runs 239 to 251 — and it also
    // straddles its own radius, so the crisp outer edge had to be found
    // by nudging. A fill with an even-odd hole gradates properly and puts
    // its boundary exactly where the number says.
    let annulus = format!(
        "M {} {cy} A {outer} {outer} 0 1 0 {} {cy} A {outer} {outer} 0 1 0 {} {cy} Z \
         M {} {cy} A {hole} {hole} 0 1 1 {} {cy} A {hole} {hole} 0 1 1 {} {cy} Z",
        cx - outer,
        cx + outer,
        cx - outer,
        cx - hole,
        cx + hole,
        cx - hole,
    );

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
    //
    // Without a housing there is nothing behind it, so the hole is a hole.
    // The ring's lit end, and **not** `shade`: that mixes toward white,
    // and #e23b53 is already near the top of its red channel, so
    // `shade(0.14)` moved it by four points and the gradient rendered
    // flat. The source's highlight is a different red — #fa4e5e — which
    // is most of the way from `rec` to `meter_danger`, both of which are
    // measured from this theme already.
    let ring_hi = if armed {
        ring.mix(t.signal.meter_danger, 0.8)
    } else {
        ring.shade(0.18)
    };
    let hole_fill = t.chrome.hardware.shade(-0.40);
    // With a housing, the hole is that housing showing through — #262626
    // in the source, the same colour as the surround, so it is painted
    // *behind* the ring. Without one there is nothing behind: the
    // source's hole is fully transparent, and filling it laid a grey disc
    // over the strip that read as a much thicker ring.
    //
    // Cuts then need something to cut *with*, and over nothing that has
    // to be a mask, since "paint transparent" does not erase.
    let notch_id = "recnotch";

    rsx! {
        svg {
            width: "{props.width.unwrap_or(vw as u32)}",
            height: "{props.height.unwrap_or(vh as u32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                // The ring is lit from the top, like every other moulded
                // part here: the source carries #fa4e5e and #e23b53 in
                // equal measure and this drew only the darker of the two,
                // which is why it read flat and dull beside the original.
                linearGradient { id: "recring", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{ring_hi.css()}" }
                    stop { offset: "1", stop_color: "{ring.css()}" }
                }
            }
            if props.housing {
                // Traced off `mcp_recarm_on` by **sub-pixel coverage**, not
                // by thresholding the alpha. Down column 6 the alpha runs
                // 49, 101, 153, 205, 244 from y=8 to y=12 — an edge still
                // creeping outward at about a fifth of a pixel per row,
                // where a threshold reports a hard vertical line at x=6 for
                // six rows running. Every earlier reading of this shape was
                // built on that phantom straight edge.
                //
                // What it is: a circle of radius 11.5 centred on
                // (17.5, 12.3) — concentric with the ring, give or take —
                // that goes flat near its widest point, sitting on a base
                // whose top corners are 45° flares. No vertical section.
                circle {
                    // 12.5, not 12.3: at 12.3 the circle's top lands at
                    // y=0.8 and catches row 0, where the source starts at
                    // row 1. Half a pixel of centre, a whole row of art.
                    cx: "{cx}", cy: "{vh * 0.5208}", r: "{vw * 0.3194}",
                    fill: "{hole_fill.css()}",
                }
                path {
                    d: "M {cx - vw * 0.3194} {vh * 0.592}
                        L {cx - vw * 0.4028} {vh * 0.717} V {vh}
                        H {cx + vw * 0.4028} V {vh * 0.717}
                        L {cx + vw * 0.3194} {vh * 0.592} Z",
                    fill: "{hole_fill.css()}",
                }
            }
            // Auto is a **solid disc with the A knocked out of it**, not a
            // ring with a letter laid inside. Drawing both gave a grey
            // annulus with a second grey A floating in its hole — two
            // marks where the source has one.
            if auto {
                circle {
                    cx: "{cx}", cy: "{cy}", r: "{outer}",
                    fill: "url(#recring)",
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
                // Four radial notches when barred, not two crossing lines:
                // the original reads as a life-ring, with the cuts running
                // through the band to the outer edge. They are a mask so
                // they cut the ring whether or not anything is behind it.
                if props.housing {
                    circle {
                        cx: "{cx}", cy: "{cy}", r: "{hole}",
                        fill: "{hole_fill.css()}",
                    }
                }
                if barred {
                    defs {
                        mask { id: "{notch_id}",
                            rect {
                                x: "0", y: "0", width: "{vw}", height: "{vh}",
                                fill: "#ffffff",
                            }
                            // Two straight bars, crossing at the
                            // centre — an **X laid over the whole ring**,
                            // not four wedges cut out of it.
                            //
                            // Both leave four blobs at this size, which is
                            // why wedges looked plausible, but they are
                            // not the same shape: a wedge's cut edges
                            // point at the centre, and the source's are
                            // parallel. Mapping the pixels shows the top
                            // blob narrowing from 8 columns to 6 over two
                            // rows, which is a straight edge crossing it,
                            // not a radial one.
                            for (i, deg) in [45.0f32, 135.0].iter().enumerate() {
                                {
                                    let a = deg.to_radians();
                                    let (dx, dy) = (a.cos(), a.sin());
                                    let far = outer + unit * 0.1;
                                    rsx! {
                                        line {
                                            key: "{i}",
                                            x1: "{cx - dx * far}", y1: "{cy - dy * far}",
                                            x2: "{cx + dx * far}", y2: "{cy + dy * far}",
                                            stroke: "#000000",
                                            stroke_width: "{unit * 0.145}",
                                        }
                                    }
                                }
                            }
                        }
                    }
                    path {
                        d: "{annulus}",
                        fill: "url(#recring)",
                        fill_rule: "evenodd",
                        mask: "url(#{notch_id})",
                    }
                } else {
                    path {
                        d: "{annulus}",
                        fill: "url(#recring)",
                        fill_rule: "evenodd",
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
    /// The cell this replaces: `mcp_io*` is 23x32, `track_io*` 29x22.
    #[props(default = (23.0, 32.0))]
    pub cell: (f32, f32),
    /// Mixer stacks the lanes; the track panel sets them in a row.
    #[props(default)]
    pub axis: Axis,
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
    let k = ink(None, props.at, true, 0.35);
    let (vw, vh) = props.cell;

    // Three lanes, not two: the original stacks the track's own output,
    // its sends and its receives. Two bars cannot express what the
    // control reports.
    //
    // Read off the source cells: the top lane is blue in all of them — a
    // track always has an output — and only the lower two light up.
    // Colouring it conditionally made an unrouted track look broken
    // rather than merely unrouted.
    //
    // An unlit lane is #6c6c6c — plain grey. It was `text_faint`, a
    // blue-grey, so a track with nothing routed looked faintly lit.
    let dim = t.chrome.hardware_mark.shade(-0.33);
    let out = t.chrome.accent;
    let send = if props.has_sends { t.signal.meter_warn } else { dim };
    // `meter_danger`, not `rec`: the source uses a brighter #ff5260 for a
    // lit lane than the #e23b53 of the record ring.
    let recv = if props.has_receives { t.signal.meter_danger } else { dim };
    let opacity = if props.disabled { "0.4" } else { "1" };

    // Traced lane geometry, per family. The mixer runs three 11x4 bars
    // down a 23x32 cell at y=6, 13, 20; the track panel runs three 4x10
    // bars across a 28x22 cell at x=5, 12, 19. Same rhythm — pitch 7 —
    // turned a quarter turn, but *not* the same fractions: guessing them
    // as "56% of the width, centred" put every lane a pixel and a bit
    // left of the art.
    let horizontal = props.axis == Axis::Horizontal;
    // x, y, w, h of the panel as fractions of the cell — traced.
    let (box_x, box_y, box_w, box_h) = if horizontal {
        (0.0, 1.0 / 22.0, 1.0, 20.0 / 22.0)
    } else {
        (1.0 / 23.0, 1.0 / 32.0, 21.0 / 23.0, 28.0 / 32.0)
    };
    let (lane_l, lane_t) = if horizontal {
        (vw * 5.0 / 28.0, vw * 7.0 / 28.0)
    } else {
        (vh * 6.0 / 32.0, vh * 7.0 / 32.0)
    };
    let (bar_w, bar_h) = if horizontal {
        (vw * 4.0 / 28.0, vh * 10.0 / 22.0)
    } else {
        (vw * 11.0 / 23.0, vh * 4.0 / 32.0)
    };
    let cross = if horizontal {
        (vh - bar_h) / 2.0
    } else {
        (vw - bar_w) / 2.0
    };
    let r = bar_w.min(bar_h) / 2.0;
    let edge = vh * 0.03;
    let (panel, panel_alpha) = if horizontal {
        ("#000000".to_string(), 0.35)
    } else {
        // #464646 measured, which is 0.04 above the hardware grey — not
        // the 0.10 this had, which read #565656 and made the mixer's
        // panel noticeably paler than the track panel's beside it.
        (k.face.shade(0.04).css(), 1.0)
    };

    rsx! {
        svg {
            width: "{props.width.unwrap_or(vw as u32)}",
            height: "{props.height.unwrap_or(vh as u32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            opacity: "{opacity}",
            // The panel does not fill its cell. Measured: the mixer's is
            // 21x28 in a 23x32 cell, the track panel's 28x20 in 28x22 —
            // so the inset differs by axis rather than being one margin.
            // Filling the cell made both buttons visibly chunkier than
            // the art beside them.
            // Inset by half the stroke, which straddles the edge it is
            // drawn on: without this the button measures two pixels wider
            // and taller than the art it replaces, in both families at
            // once — which reads as "the sizing is wrong" rather than as
            // a stroke problem.
            rect {
                x: "{vw * box_x + edge / 2.0}", y: "{vh * box_y + edge / 2.0}",
                width: "{vw * box_w - edge}", height: "{vh * box_h - edge}",
                rx: "{vw.min(vh) * 0.16}",
                // The two families are not the same fill at two
                // brightnesses. The mixer's panel is an opaque #464646 —
                // a touch lighter than plain hardware grey. The track
                // panel's is **black at 35% alpha**, a scrim that lets the
                // track colour through, which is why it looks near-black
                // on a dark strip and tinted on a coloured one. Painting
                // both opaque made the track buttons sit on the strip
                // instead of in it.
                fill: "{panel}",
                fill_opacity: "{panel_alpha}",
                stroke: "{k.border.css()}", stroke_width: "{edge}",
            }
            rect {
                x: "{vw * box_x + vw * 0.08}", y: "{vh * box_y + edge}",
                width: "{vw * box_w - vw * 0.16}", height: "{vh * 0.04}",
                fill: "#ffffff", fill_opacity: "0.07",
            }
            for (i, colour) in [out, send, recv].iter().enumerate() {
                rect {
                    key: "{i}",
                    x: if horizontal { "{lane_l + lane_t * i as f32}" } else { "{cross}" },
                    y: if horizontal { "{cross}" } else { "{lane_l + lane_t * i as f32}" },
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
    /// The cell this replaces: `mcp_monitor_*` is 21x20, `track_*` 16x24.
    #[props(default = (21.0, 20.0))]
    pub cell: (f32, f32),
    /// Mixer radiates downward; the track panel radiates right.
    #[props(default)]
    pub axis: Axis,
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
    let (vw, vh) = props.cell;

    // The original is a source radiating away from a filled dot: dot at
    // the top with arcs opening upward beneath it in the mixer, dot at the
    // left with arcs opening leftward in the track panel. This first drew
    // arcs *above* a dot at the bottom, which is the same parts assembled
    // into a different icon — it read as wifi, not as a monitored input.
    // Neutral: the lit icon is #a6a6a6 in the source, and `chrome.text` is
    // the panel blue-white — on a mixer strip that reads as backlit.
    let colour = match props.state {
        Monitoring::Off => t.chrome.hardware_mark.shade(-0.3),
        Monitoring::On => t.chrome.hardware_mark,
        Monitoring::Auto => t.signal.rec,
    };
    let colour = match props.at {
        Interaction::Normal => colour,
        Interaction::Hover => colour.shade(0.18),
        Interaction::Pressed => colour.shade(-0.12),
    };

    // One drawing, turned. `origin` is where the dot sits and `deg` the
    // direction the waves travel, so the arc maths below is written once.
    let horizontal = props.axis == Axis::Horizontal;
    let unit = vw.min(vh);
    let (cx, cy, deg) = if horizontal {
        (vw * 0.22, vh * 0.5, 0.0f32)
    } else {
        (vw * 0.5, vh * 0.24, 90.0f32)
    };
    let sw = unit * 0.10;
    let dot = unit * 0.09;

    // Arc endpoints at ±55° either side of the travel direction.
    let arc = |r: f32| {
        let (a, b) = ((deg - 55.0).to_radians(), (deg + 55.0).to_radians());
        format!(
            "M {} {} A {r} {r} 0 0 1 {} {}",
            cx + a.cos() * r,
            cy + a.sin() * r,
            cx + b.cos() * r,
            cy + b.sin() * r,
        )
    };

    rsx! {
        svg {
            width: "{props.width.unwrap_or(vw as u32)}",
            height: "{props.height.unwrap_or(vh as u32)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            circle { cx: "{cx}", cy: "{cy}", r: "{dot}", fill: "{colour.css()}" }
            for (i, rad) in [unit * 0.26, unit * 0.44].iter().enumerate() {
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
    // 0.40 of the width, not 0.44: the disc measured two pixels wider
    // than the source's at the small size.
    let r = vw * (if props.large { 0.46 } else { 0.40 });

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

    // Traced off `mcp_volthumb`, row by row down the centre column:
    //
    //     y5      #0e0e0e   top border
    //     y6-7    #696969   bevel, catching the light
    //     y8-12   #414141   body above the grip
    //     y13-39  ribs, alternating light and dark every row and
    //             brightening downward from #9d/#50 to #d9/#64
    //     y40-46  #2b2b2b   body below
    //     y47     #0b0b0b   bottom border
    //     y48-52  a soft drop shadow, fading to nothing
    //
    // The previous version drew two floating ribbed blocks with no body,
    // no border and no bevel — the cap read as a pair of grilles rather
    // than a moulded thumb, because those three are most of what makes it
    // look like an object at all.
    let grip = props.accent.unwrap_or(t.chrome.hardware_mark);
    let body = t.chrome.hardware;
    let edge = t.chrome.hardware_edge.shade(-0.35);

    // Fractions of the cell, all measured.
    let (x0, x1) = (vw * 2.0 / 27.0, vw * 21.0 / 27.0);
    let (top, bot) = (vh * 5.0 / 53.0, vh * 48.0 / 53.0);
    let (gx0, gw) = (vw * 7.0 / 27.0, vw * 11.0 / 27.0);
    let (gy0, gy1) = (vh * 13.0 / 53.0, vh * 40.0 / 53.0);

    rsx! {
        svg {
            width: "{props.width.unwrap_or(27)}",
            height: "{props.height.unwrap_or(53)}",
            view_box: "0 0 {vw} {vh}",
            xmlns: "http://www.w3.org/2000/svg",
            defs {
                linearGradient { id: "capbody", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{body.shade(0.06).css()}" }
                    stop { offset: "0.65", stop_color: "{body.shade(-0.02).css()}" }
                    stop { offset: "1", stop_color: "{body.shade(-0.32).css()}" }
                }
            }
            // The shadow it casts, below the body.
            rect {
                x: "{x0 + 1.0}", y: "{bot - 1.0}",
                width: "{x1 - x0 - 2.0}", height: "{vh - bot + 3.0}",
                rx: "{vw * 0.1}",
                fill: "#000000", fill_opacity: "0.20",
            }
            // Body, with a one-pixel border drawn as fill beneath it so
            // the face cannot bleed past the frame.
            rect {
                x: "{x0}", y: "{top}",
                width: "{x1 - x0}", height: "{bot - top}",
                rx: "{vw * 0.16}",
                fill: "{edge.css()}",
            }
            rect {
                x: "{x0 + 1.0}", y: "{top + 1.0}",
                width: "{x1 - x0 - 2.0}", height: "{bot - top - 2.0}",
                rx: "{vw * 0.13}",
                fill: "url(#capbody)",
            }
            // The lit bevel across the top — two rows, the brighter above.
            rect {
                x: "{x0 + 2.0}", y: "{top + 1.0}",
                width: "{x1 - x0 - 4.0}", height: "1.0",
                fill: "{body.shade(0.28).css()}",
            }
            rect {
                x: "{x0 + 2.0}", y: "{top + 2.0}",
                width: "{x1 - x0 - 4.0}", height: "1.0",
                fill: "{body.shade(0.16).css()}",
            }
            // The silver grip.
            //
            // Mapping it row by row shows it is not a comb of full-width
            // bands, which is what this drew:
            //
            //     y15  .....###########.....
            //     y16  .....###+++++###.....
            //     y26  .....................
            //
            // A light panel spanning x7..x17, and the grooves are *short
            // centre notches* — x10..x14 — so three columns of silver run
            // unbroken down each side. y26 is different again: a full
            // width dark row, the seam between the two halves. Drawing
            // every groove full width flattened the panel into a grille
            // and lost both the side rails and the seam.
            defs {
                linearGradient { id: "capgrip", x1: "0", y1: "0", x2: "0", y2: "1",
                    stop { offset: "0", stop_color: "{grip.shade(-0.03).css()}" }
                    stop { offset: "1", stop_color: "{grip.shade(0.34).css()}" }
                }
            }
            rect {
                x: "{gx0}", y: "{gy0}",
                width: "{gw}", height: "{gy1 - gy0}",
                rx: "{vw * 0.055}",
                fill: "url(#capgrip)",
            }
            // Five notches, the seam, five more: eleven rows on alternate
            // lines from y16 to y36, with y26 the full-width split. This
            // ran to thirteen with the seam at the seventh, which put the
            // split a row late and carried two phantom notches past the
            // bottom of the panel.
            for i in 0..11i32 {
                {
                    let y = gy0 + vh * (3.0 + 2.0 * i as f32) / 53.0;
                    let seam = i == 5;
                    let down = i as f32 / 10.0;
                    rsx! {
                        rect {
                            key: "{i}",
                            x: if seam { "{gx0}" } else { "{gx0 + gw * 0.27}" },
                            y: "{y}",
                            width: if seam { "{gw}" } else { "{gw * 0.46}" },
                            height: "{vh / 53.0}",
                            fill: "{grip.shade(-0.52 + 0.12 * down).css()}",
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
            ("mcp_recarm_on", render_svg(RecordArmButton, RecordArmProps { cell: (36.0, 24.0), housing: true, state: RecordArm::On, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_mute_on", render_svg(MuteButton, ToggleProps { hover: 0.35, sinks: true, depth: 0.15, legend: None, cell: (21.0, 20.0), body: (0.0, 1.0), on: true, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_solo_on", render_svg(SoloButton, SoloProps { hover: 0.35, sinks: true, depth: 0.11, legend: None, cell: (21.0, 20.0), body: (0.0, 1.0), state: Solo::On, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_fx_norm", render_svg(FxButton, FxProps { family: Default::default(), state: FxChain::Active, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_io_s_r", render_svg(RoutingButton, RoutingProps { cell: (23.0, 32.0), axis: Default::default(), has_sends: true, has_receives: true, disabled: false, width: n.0, height: n.1, at: Interaction::Normal })),
            ("mcp_monitor_on", render_svg(InputMonitorIndicator, MonitoringProps { cell: (21.0, 20.0), axis: Default::default(), state: Monitoring::On, width: n.0, height: n.1, at: Interaction::Normal })),
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
                        hover: 0.35,
                        sinks: true,
                        depth: 0.15,
                        legend: None,
                        body: (0.0, 1.0),
                        cell: (21.0, 20.0),
                        on: true,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    SoloButton,
                    SoloProps {
                        hover: 0.35,
                        sinks: true,
                        depth: 0.11,
                        legend: None,
                        body: (0.0, 1.0),
                        cell: (21.0, 20.0),
                        state: Solo::Defeat,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    FxButton,
                    FxProps {
                        family: Default::default(),
                        state: FxChain::Active,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    RecordArmButton,
                    RecordArmProps {
                        cell: (36.0, 24.0),
                        housing: true,
                        state: RecordArm::NoRecord,
                        width: w,
                        height: h,
                        at: Interaction::Normal,
                    },
                ),
                render_svg(
                    RoutingButton,
                    RoutingProps {
                        cell: (23.0, 32.0),
                        axis: Default::default(),
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
                        cell: (21.0, 20.0),
                        axis: Default::default(),
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
                hover: 0.35,
                sinks: true,
                depth: 0.15,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
                on: false,
                width: Some(21),
                height: Some(20),
                at: Interaction::Normal,
            },
        );
        let large = render_svg(
            MuteButton,
            ToggleProps {
                hover: 0.35,
                sinks: true,
                depth: 0.15,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
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
                hover: 0.35,
                sinks: true,
                depth: 0.15,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
                on: false,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        let h = render_svg(
            MuteButton,
            ToggleProps {
                hover: 0.35,
                sinks: true,
                depth: 0.15,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
                on: false,
                width: None,
                height: None,
                at: Interaction::Hover,
            },
        );
        let p = render_svg(
            MuteButton,
            ToggleProps {
                hover: 0.35,
                sinks: true,
                depth: 0.15,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
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
                hover: 0.35,
                sinks: true,
                depth: 0.11,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
                state: Solo::Off,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        let on = render_svg(
            SoloButton,
            SoloProps {
                hover: 0.35,
                sinks: true,
                depth: 0.11,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
                state: Solo::On,
                width: None,
                height: None,
                at: Interaction::Normal,
            },
        );
        let defeat = render_svg(
            SoloButton,
            SoloProps {
                hover: 0.35,
                sinks: true,
                depth: 0.11,
                legend: None,
                body: (0.0, 1.0),
                cell: (21.0, 20.0),
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
        // The grip is fourteen ribs of shaded variants, so the accent
        // never appears verbatim — asserting on its exact hex only passed
        // while the panel was a flat fill, and pinning any *particular*
        // shade just re-broke every time the ramp was retuned. What the
        // caller actually promises is that the accent reaches the grip
        // and nothing else, so check the hue instead of the value.
        // Counting rects was a proxy for "it has ribs" and broke the
        // moment the grip stopped being one rect per row — which was a
        // correction, not a regression. Only the hue is the promise.
        let greenish = svg
            .match_indices("fill=\"#")
            .filter_map(|(i, _)| svg.get(i + 7..i + 13))
            .filter_map(|h| Color::hex(&format!("#{h}")))
            .filter(|c| c.g > c.r && c.g > c.b)
            .count();
        assert!(greenish > 10, "the accent did not reach the grip");

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
