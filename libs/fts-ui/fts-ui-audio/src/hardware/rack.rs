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
            Ring::None => Vec::new(),
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
    pub knob: KnobStyle,
    pub items: &'static [RackItem],
}
