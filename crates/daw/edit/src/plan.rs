//! The types of edit planning (`daw.edit`'s `PLAN`).
//!
//! A kit's played hits, the part as written, the bars and the song's
//! sections in; a target for every hit out, with why, and the few a
//! person should look at.
//!
//! The answer is in the shape an apply takes: `targets` (where each hit
//! that cuts goes), `remove` (markers that are no hit of their own: their
//! sound stays where it is), `delete` (hits whose sound goes) and `rides`
//! (a hit with no cut of its own, moved with the one it rides).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A hit as it plays now, before the plan moves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayedHit {
    pub id: String,
    /// The drum: `Kick`, `Snare`, `Tom 1`, …
    pub piece: String,
    /// The take it is from (the hit id's code after `~`, or the comp's
    /// name for it).
    #[serde(default)]
    pub take: String,
    /// Where it plays now, project seconds.
    pub time: f64,
    /// Its peak, dBFS.
    #[serde(default)]
    pub peak_db: f64,
    /// What heard it.
    #[serde(default)]
    pub heard: Heard,
    /// `ghost`, `flam`, `bleed`, `unsure`, `false-double`.
    #[serde(default)]
    pub flags: Vec<String>,
}

impl PlayedHit {
    #[must_use]
    pub fn flagged(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// Which of a piece's inputs found a hit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Heard {
    Both,
    Trigger,
    Mic,
    /// Placed by hand, or not said.
    #[default]
    Unknown,
}

/// A written note of the part, already mapped to its piece.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Project seconds.
    pub time: f64,
    pub piece: String,
    #[serde(default)]
    pub velocity: u8,
}

/// One bar: where it starts and its meter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BarLine {
    /// Its number (bar 1 is the song's first).
    pub bar: i32,
    /// Where it starts, project seconds.
    pub at: f64,
    /// The meter: `beats` of `unit` (4/4, 6/8, …).
    pub beats: u32,
    pub unit: u32,
    /// How long one beat (one `unit`) lasts, seconds.
    pub beat: f64,
}

/// A section of the song (`VS 1`, `CH 2`), project seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub from: f64,
    pub to: f64,
}

/// What a zone asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoneKind {
    /// The drummer leaves the groove: left as played, unless it has a
    /// grid of its own.
    Fill,
    /// Not to be planned.
    Exception,
}

/// A zone: a stretch the plan treats apart, project seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub start: f64,
    pub end: f64,
    pub kind: ZoneKind,
    /// A fill quantized on a grid of its own (a whole note's division:
    /// 12, 16, 24, …). `None`: left as played.
    #[serde(default)]
    pub grid: Option<u32>,
}

/// Which the plan trusts more where they disagree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Prefer {
    /// The written note, when it is near enough.
    #[default]
    Midi,
    /// The nearest line, the notes only breaking ties.
    Grid,
}

/// A piece's strokes that say where the beat is: the backbeat snare.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub piece: String,
    /// The beats of the bar it plays on, 1-based (`[3]` in half time,
    /// `[2, 4]` otherwise).
    pub beats: Vec<f64>,
    /// Only in this section (all when none).
    #[serde(default)]
    pub section: Option<String>,
}

/// How the plan is made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// A section's grid (a whole note's division) instead of choosing
    /// straight or triplet per beat: `{"CH 2": 16}`.
    #[serde(default)]
    pub grid_hint: BTreeMap<String, u32>,
    /// The backbeat, where it is not found (half time is found per
    /// section).
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    /// Straight runs (double kicks) go to consecutive lines, in order.
    #[serde(default = "yes")]
    pub lock_runs: bool,
    #[serde(default)]
    pub prefer: Prefer,
}

const fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            grid_hint: BTreeMap::new(),
            anchors: Vec::new(),
            lock_runs: true,
            prefer: Prefer::Midi,
        }
    }
}

/// Why a hit goes where it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// To its own drum's written note.
    Midi,
    /// To the nearest line, after the lean.
    Grid,
    /// A stroke of a straight run, to its line in order.
    DoubleRun,
    /// No cut of its own: moves with this hit (a unison or a flam's
    /// second stroke).
    Rides(String),
    /// A grace note before this main stroke: rides it.
    FlamGrace(String),
    /// In a fill quantized on its own grid (a whole note's division).
    FillGrid(u32),
    /// Left as played (a fill, an exception).
    Kept,
    /// A second detection of one stroke: no hit of its own.
    FalseDouble,
    /// The old take sounding at a comp seam: its sound goes.
    SeamDouble,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Midi => f.write_str("midi"),
            Self::Grid => f.write_str("grid"),
            Self::DoubleRun => f.write_str("double-run"),
            Self::Rides(id) => write!(f, "rides {id}"),
            Self::FlamGrace(id) => write!(f, "flam-grace {id}"),
            Self::FillGrid(n) => write!(f, "fill-grid {n}"),
            Self::Kept => f.write_str("kept"),
            Self::FalseDouble => f.write_str("false-double"),
            Self::SeamDouble => f.write_str("seam-double"),
        }
    }
}

impl FromStr for Reason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (word, rest) = s.split_once(' ').unwrap_or((s, ""));
        Ok(match (word, rest) {
            ("midi", "") => Self::Midi,
            ("grid", "") => Self::Grid,
            ("double-run", "") => Self::DoubleRun,
            ("rides", id) if !id.is_empty() => Self::Rides(id.to_owned()),
            ("flam-grace", id) if !id.is_empty() => Self::FlamGrace(id.to_owned()),
            ("fill-grid", n) => Self::FillGrid(n.parse().map_err(|_| format!("bad reason: {s}"))?),
            ("kept", "") => Self::Kept,
            ("false-double", "") => Self::FalseDouble,
            ("seam-double", "") => Self::SeamDouble,
            _ => return Err(format!("bad reason: {s}")),
        })
    }
}

impl Serialize for Reason {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Reason {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

/// Where one hit goes, and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Planned {
    pub id: String,
    /// Where it plays once applied, project seconds (where it rides to,
    /// for a rider; where it was, for one removed or deleted).
    pub target: f64,
    pub reason: Reason,
    /// 0..1: how clear the choice was.
    pub confidence: f64,
}

/// A hit a person should look at: the two places it could go.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Doubt {
    pub id: String,
    pub piece: String,
    /// Where it plays now, project seconds.
    pub time: f64,
    /// The planned target first, then the other.
    pub candidates: [f64; 2],
    pub why: String,
}

/// How early (negative) or late the drummer played over half a bar,
/// measured from its anchors.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lean {
    pub bar: i32,
    /// 0: the first half, 1: the second.
    pub half: u8,
    /// Seconds (played minus written).
    pub lean: f64,
    /// How many strokes measured it (0: carried from a neighbour).
    pub anchors: u32,
}

/// A plan for a kit's hits: [`PlanRequest`](crate::PlanRequest)'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// Every hit asked about, in time order.
    pub hits: Vec<Planned>,
    /// Where each hit that cuts goes (riders, removed and deleted hits
    /// are not here): an apply's `targets`.
    pub targets: BTreeMap<String, f64>,
    /// Markers that are no hit of their own (false doubles): no cut, the
    /// sound stays.
    pub remove: Vec<String>,
    /// Hits whose sound goes (the old take at a comp seam).
    pub delete: Vec<String>,
    /// A hit with no cut of its own → the hit it moves with.
    pub rides: BTreeMap<String, String>,
    /// Only the hits worth a look.
    pub doubts: Vec<Doubt>,
    /// The lean per half bar.
    pub leans: Vec<Lean>,
}
