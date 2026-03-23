//! Marker service trait
//!
//! Defines the RPC interface for marker operations.

use super::{Marker, MarkerEvent};
use crate::ProjectContext;
use vox::{Tx, service};

/// Service for managing markers in a DAW project
///
/// Markers are named reference points in the timeline that can be used
/// for navigation, synchronization, or structural organization.
#[service]
pub trait MarkerService {
    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all markers in the project
    async fn get_markers(&self, project: ProjectContext) -> Vec<Marker>;

    /// Get a specific marker by ID
    async fn get_marker(&self, project: ProjectContext, id: u32) -> Option<Marker>;

    /// Get all markers within a time range (inclusive)
    async fn get_markers_in_range(
        &self,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Marker>;

    /// Get the next marker after the given position
    async fn get_next_marker(&self, project: ProjectContext, after: f64) -> Option<Marker>;

    /// Get the previous marker before the given position
    async fn get_previous_marker(&self, project: ProjectContext, before: f64) -> Option<Marker>;

    /// Get the total number of markers
    async fn marker_count(&self, project: ProjectContext) -> usize;

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Add a new marker at the given position (returns marker ID)
    async fn add_marker(&self, project: ProjectContext, position: f64, name: String) -> u32;

    /// Remove a marker by ID
    async fn remove_marker(&self, project: ProjectContext, id: u32);

    /// Move a marker to a new position
    async fn move_marker(&self, project: ProjectContext, id: u32, position: f64);

    /// Rename a marker
    async fn rename_marker(&self, project: ProjectContext, id: u32, name: String);

    /// Set the color of a marker (0 for default color)
    async fn set_marker_color(&self, project: ProjectContext, id: u32, color: u32);

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    /// Navigate to the next marker from current position
    async fn goto_next_marker(&self, project: ProjectContext);

    /// Navigate to the previous marker from current position
    async fn goto_previous_marker(&self, project: ProjectContext);

    /// Navigate to a specific marker by ID
    async fn goto_marker(&self, project: ProjectContext, id: u32);

    // =========================================================================
    // Lane Methods (v7.62+)
    // =========================================================================

    /// Add a new marker at the given position in a specific ruler lane.
    /// Returns the marker ID.
    async fn add_marker_in_lane(
        &self,
        project: ProjectContext,
        position: f64,
        name: String,
        lane: u32,
    ) -> u32;

    /// Set the ruler lane for a marker (None to move to default lane)
    async fn set_marker_lane(&self, project: ProjectContext, id: u32, lane: Option<u32>);

    /// Get all markers in a specific ruler lane
    async fn get_markers_in_lane(&self, project: ProjectContext, lane: u32) -> Vec<Marker>;

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to marker changes for a project
    ///
    /// Streams events when:
    /// - A marker is added, removed, or modified
    /// - Project markers are bulk-updated (e.g., project reload)
    ///
    /// Initially sends a `MarkersChanged` event with all current markers.
    async fn subscribe(&self, project: ProjectContext, tx: Tx<MarkerEvent>);
}
