//! The quantize surface.
//!
//! The engine was complete and tested, and there was **no way to reach
//! any of it from the editor** — the feature was done and unusable. This
//! is the surface, and because quantize is a tool on the seam it works
//! for MIDI notes as well as audio transients rather than only the
//! latter.
//!
//! State and geometry live here so they are assertable without a
//! renderer; the `rsx!` chrome belongs with the rest of the canvas.

use expression_editor_tools::event::Timed;
use expression_editor_tools::quantize::{Plan, QuantizeConfig, plan};

/// How the quantize is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WriteMode {
    /// Cut the audio and move the pieces. Phase-coherent across a mic
    /// group, which is why drum editing is done this way.
    #[default]
    Split,
    /// Bend time between anchors, keeping the material continuous.
    Warp,
}

/// Which tracks are edited, and which one the hits are detected from.
///
/// Separate on purpose: a kit is quantized from the snare or a trigger
/// track, and every mic is cut in the same places. Detecting per-mic
/// would smear the source at every join.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// Track guids being edited.
    pub members: Vec<String>,
    /// The guid hits are detected from. Must be one of `members`, or the
    /// group is editing to a reference nobody can hear.
    pub trigger: Option<String>,
}

impl Group {
    pub fn is_valid(&self) -> bool {
        match &self.trigger {
            Some(t) => self.members.contains(t),
            None => false,
        }
    }
}

/// Everything the panel holds.
#[derive(Clone, Debug, Default)]
pub struct QuantizePanel {
    pub config: QuantizeConfig,
    pub mode: WriteMode,
    /// Leading pad before each cut, in the events' unit. SPLIT only —
    /// a cut exactly on a transient clips its attack.
    pub pad: f64,
    /// Crossfade length at each join. SPLIT only.
    pub crossfade: f64,
    pub group: Group,
}

/// A vertical line over the waveform: where a hit is, and where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriggerLine {
    pub x: f64,
    /// Where it will move to, in pixels. Equal to `x` when it does not
    /// move.
    pub to_x: f64,
    /// True when the planner deliberately left it alone.
    ///
    /// Surfaced rather than hidden: `Plan::unmatched` exists precisely
    /// so "why did that hit not move" is answerable, and a line that is
    /// simply absent answers nothing.
    pub unmatched: bool,
}

impl TriggerLine {
    pub fn moves(&self) -> bool {
        (self.to_x - self.x).abs() > f64::EPSILON
    }
}

/// The panel's preview of a plan, in pixels.
pub struct Preview {
    pub lines: Vec<TriggerLine>,
    /// Grid divisions in view, so the target is visible and not implied.
    pub divisions: Vec<f64>,
}

/// Plan and lay out, in one step.
pub fn preview<E: Timed>(
    events: &[E],
    panel: &QuantizePanel,
    width: f64,
    span: (f64, f64),
) -> (Plan, Preview) {
    let p = plan(events, panel.config);
    let view = lay_out(&p, width, span, panel.config);
    (p, view)
}

fn x_of(t: f64, width: f64, span: (f64, f64)) -> f64 {
    let (from, to) = span;
    let len = (to - from).max(1e-9);
    ((t - from) / len).clamp(0.0, 1.0) * width
}

/// Turn a plan into lines and divisions.
pub fn lay_out(p: &Plan, width: f64, span: (f64, f64), cfg: QuantizeConfig) -> Preview {
    let mut lines: Vec<TriggerLine> = p
        .moves
        .iter()
        .map(|m| TriggerLine {
            x: x_of(m.from, width, span),
            to_x: x_of(m.to, width, span),
            unmatched: false,
        })
        .collect();

    lines.extend(p.unmatched.iter().map(|&at| {
        let x = x_of(at, width, span);
        TriggerLine {
            x,
            to_x: x,
            unmatched: true,
        }
    }));
    lines.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(core::cmp::Ordering::Equal));

    let mut divisions = Vec::new();
    if cfg.grid > 0.0 {
        let (from, to) = span;
        let first = ((from - cfg.grid_offset) / cfg.grid).ceil();
        let mut d = cfg.grid_offset + first * cfg.grid;
        while d <= to {
            divisions.push(x_of(d, width, span));
            d += cfg.grid;
        }
    }

    Preview { lines, divisions }
}

/// One bar of the sensitivity histogram.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bin {
    /// Lower edge of the bin, in event strength.
    pub from: f64,
    pub to: f64,
    pub count: usize,
}

/// Distribution of event strengths.
///
/// Perfect Timing has one, and it is the difference between guessing at
/// a slider and seeing where the hits actually sit. It also shows the
/// threshold against the material's own noise floor, which is what makes
/// the crest reading below threshold interpretable rather than
/// mysterious.
pub fn histogram<E: Timed>(events: &[E], bins: usize) -> Vec<Bin> {
    let bins = bins.max(1);
    let mut out: Vec<Bin> = (0..bins)
        .map(|i| Bin {
            from: i as f64 / bins as f64,
            to: (i + 1) as f64 / bins as f64,
            count: 0,
        })
        .collect();

    for e in events {
        let s = e.strength().clamp(0.0, 1.0);
        // The top edge belongs to the last bin rather than falling off
        // the end.
        let idx = ((s * bins as f64) as usize).min(bins - 1);
        out[idx].count += 1;
    }
    out
}

/// Where the sensitivity threshold sits on the histogram, 0..1.
pub fn threshold_position(cfg: &QuantizeConfig) -> f64 {
    cfg.min_strength.clamp(0.0, 1.0)
}

/// How many events the filter is currently excluding.
///
/// The number a user actually wants next to the slider: "how much am I
/// about to leave alone".
pub fn excluded_count<E: Timed>(events: &[E], cfg: &QuantizeConfig) -> usize {
    events
        .iter()
        .filter(|e| e.strength() < cfg.min_strength)
        .count()
}
