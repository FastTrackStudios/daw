//! A track's own regions and markers, as a window draws them (the marks
//! the engine sends with a song). Reading, writing and proposing them is
//! the engine's (`session_engine::track_regions`).

use facet::Facet;

/// Where on a track they are kept.
pub const KEY: &str = "fts.marks";

/// What a region says about its stretch.
#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RegionKind {
    /// Its hits on a grid of its own (`division`).
    Grid,
    /// Left as played: no quantize moves a hit in it.
    NoQuantize,
    /// A fill: left as played unless a quantize is asked to edit fills.
    Fill,
    /// Only a note.
    Note,
}

/// Who put it there.
#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Origin {
    /// By hand.
    Manual,
    /// Proposed (from the MIDI, from the hits), not yet decided.
    Auto,
    /// Proposed and approved.
    Approved,
    /// Proposed and turned down: kept, so it is not proposed again.
    Rejected,
}

/// A stretch of a track.
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct TrackRegion {
    pub id: String,
    /// Project seconds.
    pub start: f64,
    pub end: f64,
    pub kind: RegionKind,
    /// A grid region's grid: a whole note's division (16 sixteenths, 3
    /// half-note triplets, 12 eighth triplets, 24 sixteenth triplets).
    pub division: Option<u32>,
    pub origin: Origin,
    pub note: String,
}

impl TrackRegion {
    /// What it is called where it is drawn: `Grid 1/2T`, `No quantize`.
    #[must_use]
    pub fn label(&self) -> String {
        let what = match self.kind {
            RegionKind::Grid => {
                format!("Grid {}", self.division.map_or_else(String::new, grid_name))
            }
            RegionKind::NoQuantize => "No quantize".to_owned(),
            RegionKind::Fill => self
                .division
                .map_or_else(|| "Fill".to_owned(), |d| format!("Fill {}", grid_name(d))),
            RegionKind::Note => String::new(),
        };
        let who = match self.origin {
            Origin::Manual => "",
            Origin::Auto => " (auto)",
            Origin::Approved => " (approved)",
            Origin::Rejected => " (rejected)",
        };
        let label = format!("{what}{who}");
        match (label.is_empty(), self.note.is_empty()) {
            (_, true) => label,
            (true, false) => self.note.clone(),
            (false, false) => format!("{label} — {}", self.note),
        }
    }

    /// Whether it holds hits — anything but one turned down.
    #[must_use]
    pub fn counts(&self) -> bool {
        self.origin != Origin::Rejected
    }
}

/// A named moment on a track.
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct TrackMarker {
    pub id: String,
    /// Project seconds.
    pub at: f64,
    pub name: String,
}

/// Everything a track keeps: its regions and markers, in time order.
#[derive(Facet, Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub regions: Vec<TrackRegion>,
    pub markers: Vec<TrackMarker>,
}

/// A grid written as a note value: 16 → `1/16`, 3 → `1/2T`, 12 → `1/8T`.
#[must_use]
pub fn grid_name(division: u32) -> String {
    if division.is_multiple_of(3) {
        let straight = division.checked_div(3).unwrap_or(0).saturating_mul(2);
        format!("1/{straight}T")
    } else {
        format!("1/{division}")
    }
}
