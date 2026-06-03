//! Shared view-model for the top-level DAW panels.
//!
//! [`TrackView`] is a single track as the three panels see it — a reusable,
//! self-contained model (no `daw-proto` dependency) so the panels can drive
//! any host's data. Mutable per-track state lives in `Signal`s (so a fader
//! moved in the mixer also moves in the TCP), while layout/metadata is owned.

use dioxus::prelude::*;

/// One clip/item on a track's arrangement lane.
#[derive(Clone, PartialEq)]
pub struct ClipView {
    /// Start position, in seconds on the timeline.
    pub start: f64,
    /// Length, in seconds.
    pub length: f64,
    pub name: String,
    /// `#rrggbb`; falls back to the track colour when `None`.
    pub color: Option<String>,
}

/// A track as the TrackControlPanel, MixerControlPanel, and ArrangeView all
/// see it. Shared `Signal`s keep the three panels in sync.
#[derive(Clone, PartialEq)]
pub struct TrackView {
    pub id: usize,
    pub name: String,
    /// Track colour (`#rrggbb`). Tints the strip, TCP row, and lane.
    pub color: Option<String>,

    // ── live, shared state ──
    /// Normalized fader position (0–1).
    pub fader: Signal<f32>,
    /// Pan (bipolar; 0.5 = centre).
    pub pan: Signal<f32>,
    pub mute: Signal<bool>,
    pub solo: Signal<bool>,
    pub record_arm: Signal<bool>,

    // ── metering (read-only inputs) ──
    pub level: f32,
    pub level_right: Option<f32>,
    pub peak: Option<f32>,

    // ── routing flags (for the mixer routing button) ──
    pub sends: bool,
    pub receives: bool,

    // ── hierarchy + layout ──
    /// Absolute folder depth: 0 = top level, 1 = inside one folder, etc.
    pub depth: u32,
    /// Whether this track is a folder parent (renders a folder header).
    pub is_folder: bool,
    /// Lane height in px (TCP rows + arrange lanes share this so they align).
    pub height: u32,

    // ── arrangement ──
    pub clips: Vec<ClipView>,
}

impl TrackView {
    /// Convenience constructor for showcase/sample data. Signals are created
    /// from the given initial values; the caller owns them thereafter.
    #[allow(clippy::too_many_arguments)]
    pub fn new(id: usize, name: impl Into<String>, color: Option<&str>) -> Self {
        Self {
            id,
            name: name.into(),
            color: color.map(|c| c.to_string()),
            fader: Signal::new(0.75),
            pan: Signal::new(0.5),
            mute: Signal::new(false),
            solo: Signal::new(false),
            record_arm: Signal::new(false),
            level: 0.0,
            level_right: None,
            peak: None,
            sends: false,
            receives: false,
            depth: 0,
            is_folder: false,
            height: DEFAULT_LANE_HEIGHT,
            clips: Vec::new(),
        }
    }

    /// Builder: set the initial fader position.
    pub fn fader(mut self, v: f32) -> Self {
        self.fader = Signal::new(v);
        self
    }
    /// Builder: set folder depth (0 = top level).
    pub fn depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }
    /// Builder: mark as a folder parent.
    pub fn folder(mut self) -> Self {
        self.is_folder = true;
        self
    }
    /// Builder: set lane/row height in px.
    pub fn height(mut self, h: u32) -> Self {
        self.height = h;
        self
    }
    /// Builder: set stereo metering levels (+ peak hold).
    pub fn levels(mut self, left: f32, right: f32, peak: f32) -> Self {
        self.level = left;
        self.level_right = Some(right);
        self.peak = Some(peak);
        self
    }
    /// Builder: attach arrangement clips.
    pub fn clips(mut self, clips: Vec<ClipView>) -> Self {
        self.clips = clips;
        self
    }
    /// Builder: routing flags.
    pub fn routing(mut self, sends: bool, receives: bool) -> Self {
        self.sends = sends;
        self.receives = receives;
        self
    }

    /// Resolved track colour (`#rrggbb`), with a neutral fallback.
    pub fn hex(&self) -> String {
        self.color.clone().unwrap_or_else(|| NEUTRAL.to_string())
    }
}

/// Default lane / TCP-row height in px.
pub const DEFAULT_LANE_HEIGHT: u32 = 54;
/// Neutral accent when a track has no colour.
pub const NEUTRAL: &str = "#a1a1aa";
