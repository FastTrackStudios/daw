//! Markers handle and operations

use std::sync::Arc;

use crate::DawClients;
use daw_proto::{Marker, MarkerEvent, ProjectContext};
use crate::Result;
use roam::Rx;

/// Markers handle for a specific project
///
/// This handle provides access to marker operations (query, add, remove, navigate)
/// for a specific project. Like reaper-rs, it's lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::ErasedCaller) -> daw_control::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let markers = project.markers();
///
/// // Query markers
/// let all = markers.all().await?;
/// let count = markers.count().await?;
///
/// // Add and manipulate markers
/// let id = markers.add(10.5, "Verse 1").await?;
/// markers.rename(id, "Chorus").await?;
/// markers.set_color(id, 0xFF0000).await?;
///
/// // Navigation
/// markers.goto_next().await?;
/// markers.goto(id).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Markers {
    project_id: String,
    clients: Arc<DawClients>,
}

impl Markers {
    /// Create a new markers handle for a project
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all markers in the project
    pub async fn all(&self) -> Result<Vec<Marker>> {
        let markers = self.clients.marker.get_markers(self.context()).await?;
        Ok(markers)
    }

    /// Get a specific marker by ID
    pub async fn get(&self, id: u32) -> Result<Option<Marker>> {
        let marker = self.clients.marker.get_marker(self.context(), id).await?;
        Ok(marker)
    }

    /// Get all markers within a time range (inclusive)
    pub async fn in_range(&self, start: f64, end: f64) -> Result<Vec<Marker>> {
        let markers = self
            .clients
            .marker
            .get_markers_in_range(self.context(), start, end)
            .await?;
        Ok(markers)
    }

    /// Get the next marker after the given position
    pub async fn next_after(&self, position: f64) -> Result<Option<Marker>> {
        let marker = self
            .clients
            .marker
            .get_next_marker(self.context(), position)
            .await?;
        Ok(marker)
    }

    /// Get the previous marker before the given position
    pub async fn previous_before(&self, position: f64) -> Result<Option<Marker>> {
        let marker = self
            .clients
            .marker
            .get_previous_marker(self.context(), position)
            .await?;
        Ok(marker)
    }

    /// Get the total number of markers
    pub async fn count(&self) -> Result<usize> {
        let count = self.clients.marker.marker_count(self.context()).await?;
        Ok(count)
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Add a new marker at the given position
    ///
    /// Returns the ID of the newly created marker.
    pub async fn add(&self, position: f64, name: &str) -> Result<u32> {
        let id = self
            .clients
            .marker
            .add_marker(self.context(), position, name.to_string())
            .await?;
        Ok(id)
    }

    /// Add a new marker at the given position in a specific ruler lane.
    ///
    /// Returns the ID of the newly created marker.
    pub async fn add_in_lane(&self, position: f64, name: &str, lane: u32) -> Result<u32> {
        let id = self
            .clients
            .marker
            .add_marker_in_lane(self.context(), position, name.to_string(), lane)
            .await?;
        Ok(id)
    }

    /// Set the ruler lane for a marker. Pass None to move to the default lane.
    pub async fn set_lane(&self, id: u32, lane: Option<u32>) -> Result<()> {
        self.clients
            .marker
            .set_marker_lane(self.context(), id, lane)
            .await?;
        Ok(())
    }

    /// Remove a marker by ID
    pub async fn remove(&self, id: u32) -> Result<()> {
        self.clients
            .marker
            .remove_marker(self.context(), id)
            .await?;
        Ok(())
    }

    /// Move a marker to a new position
    pub async fn move_to(&self, id: u32, position: f64) -> Result<()> {
        self.clients
            .marker
            .move_marker(self.context(), id, position)
            .await?;
        Ok(())
    }

    /// Rename a marker
    pub async fn rename(&self, id: u32, name: &str) -> Result<()> {
        self.clients
            .marker
            .rename_marker(self.context(), id, name.to_string())
            .await?;
        Ok(())
    }

    /// Set the color of a marker (0 for default color)
    pub async fn set_color(&self, id: u32, color: u32) -> Result<()> {
        self.clients
            .marker
            .set_marker_color(self.context(), id, color)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    /// Navigate to the next marker from current position
    pub async fn goto_next(&self) -> Result<()> {
        self.clients.marker.goto_next_marker(self.context()).await?;
        Ok(())
    }

    /// Navigate to the previous marker from current position
    pub async fn goto_previous(&self) -> Result<()> {
        self.clients
            .marker
            .goto_previous_marker(self.context())
            .await?;
        Ok(())
    }

    /// Navigate to a specific marker by ID
    pub async fn goto(&self, id: u32) -> Result<()> {
        self.clients.marker.goto_marker(self.context(), id).await?;
        Ok(())
    }

    // =========================================================================
    // Subscriptions
    // =========================================================================

    /// Subscribe to marker change events for this project.
    pub async fn subscribe(&self) -> Result<Rx<MarkerEvent>> {
        let (tx, rx) = roam::channel::<MarkerEvent>();
        self.clients.marker.subscribe(self.context(), tx).await?;
        Ok(rx)
    }
}

impl std::fmt::Debug for Markers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Markers")
            .field("project_id", &self.project_id)
            .finish()
    }
}
