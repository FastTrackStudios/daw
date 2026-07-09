//! Marker event types
//!
//! Events are emitted when marker state changes. Wire shape mirrors
//! the helgobox occasional-channel pattern: every transition is
//! delivered, subscribers shouldn't miss any.

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

/// Streaming envelope — pairs a [`MarkerEvent`] with the project it
/// applies to. The streaming hub broadcasts these across all open
/// projects on a single channel; subscribers filter by `project_guid`
/// to scope to a specific project.
#[derive(Debug, Clone, Facet)]
pub struct MarkerStreamEvent {
    pub project_guid: String,
    pub event: MarkerEvent,
}

#[cfg(feature = "vox")]
#[allow(unsafe_code)]
mod reborrow_impls {
    use super::{MarkerEvent, MarkerStreamEvent};
    unsafe impl vox_types::Reborrow for MarkerEvent {
        type Ref<'a> = MarkerEvent;
    }
    unsafe impl vox_types::Reborrow for MarkerStreamEvent {
        type Ref<'a> = MarkerStreamEvent;
    }
}
