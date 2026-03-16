//! Marker data type
//!
//! A marker represents a named point in time within a project.

use crate::{Position, TimePosition};
use facet::Facet;

/// A marker at a specific position in the project timeline
///
/// Markers are named reference points that can be used for navigation,
/// synchronization, or structural organization of a project.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct Marker {
    /// Unique identifier for the marker (assigned by the DAW)
    pub id: Option<u32>,
    /// Position of the marker in the timeline
    pub position: Position,
    /// Display name of the marker
    pub name: String,
    /// Color in native format (0xRRGGBB, or None for default)
    pub color: Option<u32>,
    /// GUID for stable identification across sessions
    pub guid: Option<String>,
    /// Ruler lane index (v7.62+). None = default lane.
    pub lane: Option<u32>,
}

impl Marker {
    /// Create a new marker at the given position with a name
    pub fn new(position: Position, name: String) -> Self {
        Self {
            id: None,
            position,
            name,
            color: None,
            guid: None,
            lane: None,
        }
    }

    /// Create a marker from a time position in seconds
    pub fn from_seconds(seconds: f64, name: String) -> Self {
        Self::new(
            Position::from_time(TimePosition::from_seconds(seconds)),
            name,
        )
    }

    /// Create a marker with all metadata
    pub fn new_full(
        id: Option<u32>,
        position: Position,
        name: String,
        color: Option<u32>,
        guid: Option<String>,
    ) -> Self {
        Self {
            id,
            position,
            name,
            color,
            guid,
            lane: None,
        }
    }

    /// Get the position in seconds
    pub fn position_seconds(&self) -> f64 {
        self.position
            .time
            .as_ref()
            .map(|t| t.as_seconds())
            .unwrap_or(0.0)
    }

    /// Check if the marker is within a time range (inclusive)
    pub fn is_in_range(&self, start: f64, end: f64) -> bool {
        let pos = self.position_seconds();
        pos >= start && pos <= end
    }

    /// Check if the marker is at a specific position (within tolerance)
    pub fn is_at_position(&self, seconds: f64, tolerance: f64) -> bool {
        (self.position_seconds() - seconds).abs() <= tolerance
    }
}

impl Default for Marker {
    fn default() -> Self {
        Self::new(Position::start(), String::new())
    }
}
