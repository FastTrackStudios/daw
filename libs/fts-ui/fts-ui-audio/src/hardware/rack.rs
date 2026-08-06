//! A rack unit's front panel, as data.
//!
//! Every emulated unit shares one anatomy: a painted panel with rack ears, a
//! meter somewhere on the left, a row of knobs with silkscreened legends, and
//! the occasional switch or bank of buttons. Hand-writing each one as its own
//! component produces near-identical files that drift apart in spacing and
//! colour, so a face is a [`RackDesign`] — a table of placements.
//!
//! The *drawing* of a design lives in the plugin, because that is where the
//! controls' handles come from: a design names controls by id, and only the
//! plugin knows how to turn an id into a [`ParamHandle`](crate::ParamHandle)
//! through its own profile data. The description lives here so the compressor
//! and the EQ describe their panels in the same vocabulary.

use crate::hardware::knob::KnobStyle;
use crate::hardware::knob_svg::{detent_ring, linear_scale_label, scale_ring, ScaleMark};
use crate::hardware::vu::{VuFace, VuMode};

/// What a knob's printed scale ring says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ring {
    /// Numbers running `from`..`to` across the sweep, with `majors` of them.
    Linear {
        from: f64,
        to: f64,
        majors: usize,
    },
    /// One numbered mark per detent — for rotary switches.
    Detents(&'static [&'static str]),
    /// Tick marks with no numbers.
    Plain { majors: usize },
    /// Numerals only, no tick marks, printed close in around the knob — the
    /// Pultec look, where the panel prints 0–10 hugging the skirt and nothing
    /// else.
    Numerals(&'static [&'static str]),
    /// Dots rather than tick marks, with a numeral at each entry that has
    /// one — the SSL's printed ring, where the dots mark the detent-less
    /// travel of a sweepable band.
    Dots(&'static [&'static str]),
    /// No printed ring at all.
    None,
}

impl Ring {
    /// The printed marks this ring resolves to.
    pub fn marks(self) -> Vec<ScaleMark> {
        match self {
            Ring::Linear { from, to, majors } => {
                scale_ring(majors, 1, linear_scale_label(from, to))
            }
            Ring::Detents(labels) => detent_ring(labels),
            Ring::Plain { majors } => scale_ring(majors, 1, |_| None),
            Ring::Numerals(labels) | Ring::Dots(labels) => detent_ring(labels),
            Ring::None => Vec::new(),
        }
    }

    /// Where this ring is printed: `(tick radius, numeral radius)` in the
    /// knob's own viewBox units, and whether tick marks are drawn at all.
    ///
    /// [`Ring::Numerals`] sits close in with no ticks; everything else keeps
    /// the default ring the compressor faces are drawn against.
    pub fn geometry(self) -> (f64, f64, bool) {
        match self {
            Ring::Numerals(_) => (31.0, 37.0, false),
            // Dots print where ticks would, and the numerals just outside.
            Ring::Dots(_) => (38.0, 48.0, false),
            // Nothing printed at all — not even the faint band the ticks sit
            // on, which is what "None" has to mean for a bypass knob.
            Ring::None => (41.0, 50.0, false),
            _ => (41.0, 50.0, true),
        }
    }
}

/// One thing placed on a panel, in design-space coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RackItem {
    /// The meter movement.
    Vu {
        x: f64,
        y: f64,
        w: f64,
        mode: VuMode,
        legend: &'static str,
    },
    /// A knob, with its legend silkscreened underneath.
    Knob {
        /// Control id on the unit's profile.
        id: &'static str,
        legend: &'static str,
        x: f64,
        y: f64,
        d: f64,
        ring: Ring,
        /// Body colour, when the unit colour-codes its controls. `None` takes
        /// the design's [`KnobStyle`].
        tint: Option<&'static str>,
        /// Override the design's knob for this one control. A panel is not
        /// always one kit: the Pultec's five big boost/atten knobs and its
        /// three small pointer knobs are different parts.
        style: Option<KnobStyle>,
    },
    /// A vertical bank of radio-like buttons (the 1176's ratios).
    Buttons {
        id: &'static str,
        legend: &'static str,
        x: f64,
        y: f64,
        labels: &'static [&'static str],
    },
    /// A two-position bat switch.
    Switch {
        id: &'static str,
        legend: &'static str,
        x: f64,
        y: f64,
        labels: [&'static str; 2],
    },
    /// A lever switch — the Pultec's frequency selectors: a paddle that swings
    /// between detents with the legends printed in an arc above it.
    Lever {
        id: &'static str,
        legend: &'static str,
        /// Small caption printed above the arc ("CPS", "KCS").
        unit: &'static str,
        x: f64,
        y: f64,
        labels: &'static [&'static str],
    },
    /// A numeric readout of a control's position on its printed 0–10 scale.
    ///
    /// The Pultec prints the value above each big knob; it reads the *panel's*
    /// scale rather than the engine's units, which is what the numbers around
    /// the knob mean too.
    Readout { id: &'static str, x: f64, y: f64 },
    /// A panel indicator lamp.
    Lamp { x: f64, y: f64, color: &'static str },
    /// A latching panel button — the console's FLTR IN, EQ IN, the phase
    /// invert, the ÷3 and ×3 range switches.
    ///
    /// `id` may be empty, which means "this control exists on the panel but
    /// has no parameter behind it yet": it draws, it does not move. Better
    /// than omitting it, because the panel is the specification for what the
    /// DSP still owes.
    Button {
        id: &'static str,
        label: &'static str,
        x: f64,
        y: f64,
        /// Face colour — cream for a function switch, red for phase.
        color: &'static str,
        /// Text colour.
        ink: &'static str,
        /// Colour of the indicator below it, if it has one.
        led: &'static str,
    },
    /// A segmented LED level meter, as the console's output metering.
    LedMeter {
        x: f64,
        y: f64,
        h: f64,
        /// Which channel, so a pair reads left and right.
        right: bool,
    },
    /// A hairline dividing two sections of a panel.
    Divider { x: f64, y: f64, h: f64 },
    /// A rounded outline around a section — the Distressor's two boxes.
    Frame { x: f64, y: f64, w: f64, h: f64 },
    /// A horizontal LED ladder with its dB scale printed above it.
    LedBar {
        x: f64,
        y: f64,
        /// The printed scale, left (deepest reduction) to right.
        steps: &'static [f64],
        pitch: f64,
    },
    /// A row of labelled LEDs that selects a stepped control.
    LedSelect {
        id: &'static str,
        x: f64,
        y: f64,
        labels: &'static [&'static str],
        pitch: f64,
    },
    /// Silkscreened panel text in a stated colour — a channel label picked out
    /// in red, a warning, anything the panel's two inks do not cover.
    TintedText {
        x: f64,
        y: f64,
        text: &'static str,
        size: f64,
        color: &'static str,
    },
    /// Silkscreened panel text.
    Text {
        x: f64,
        y: f64,
        text: &'static str,
        size: f64,
        /// `true` for the model line, `false` for the smaller subtitle.
        strong: bool,
    },
}

/// A unit's front panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RackDesign {
    /// The profile id whose controls the items name.
    pub id: &'static str,
    /// Drawing size in design-space px.
    pub w: f64,
    pub h: f64,
    /// The paint, as a CSS background.
    pub paint: &'static str,
    /// Silkscreen colour.
    pub ink: &'static str,
    /// Secondary silkscreen colour (subtitles).
    pub dim_ink: &'static str,
    /// Rack ears and screws.
    pub chrome: &'static str,
    pub vu: VuFace,
    /// Whether the movements are mounted through the panel in a bezel.
    pub vu_bezel: bool,
    /// What the movement's card is printed with — the VU standard, or a plain
    /// decibel scale.
    pub vu_card: crate::hardware::vu_svg::VuScale,
    /// How the panel is finished at its edges.
    pub ends: crate::hardware::panel::PanelEnds,
    pub knob: KnobStyle,
    pub items: &'static [RackItem],
}
