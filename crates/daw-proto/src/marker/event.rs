//! Marker event types
//!
//! Events are emitted when marker state changes.

use super::Marker;
use facet::Facet;

/// Events emitted when markers change
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum MarkerEvent {
    /// A new marker was added
    Added(Marker),
    /// A marker was removed (contains the ID)
    Removed(u32),
    /// A marker was modified
    Changed(Marker),
    /// Multiple markers changed (e.g., project reload)
    MarkersChanged(Vec<Marker>),
}
