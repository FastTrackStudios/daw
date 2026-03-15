//! Track module
//!
//! This module provides track types and the TrackService trait
//! for managing audio/MIDI tracks in a DAW mixer.

mod error;
mod event;
mod hierarchy;
mod hierarchy_builder;
mod service;
mod test_utils;
#[allow(clippy::module_inception)]
mod track;

pub use error::TrackError;
pub use event::TrackEvent;
pub use hierarchy::{FolderDepthChange, TrackHierarchy, TrackNode};
pub use hierarchy_builder::{AddChildren, TrackHierarchyBuilder};
pub use service::{
    TrackService, TrackServiceClient, TrackServiceDispatcher, track_service_service_descriptor,
};
pub use test_utils::{
    TrackGroup, TrackStructureBuilder, assert_tracks_equal, display_tracklist, format_tracklist,
};
pub use service::TrackExtStateRequest;
pub use track::{InputMonitoringMode, RecordInput, Track, TrackRef};
