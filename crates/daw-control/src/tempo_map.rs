//! TempoMap handle and operations

use std::sync::Arc;

use crate::DawClients;
use daw_proto::{ProjectContext, TempoMapEvent, TempoPoint};
use crate::Result;
use roam::Rx;

/// TempoMap handle for a specific project
///
/// This handle provides access to tempo map operations (query tempo points,
/// time conversion, add/remove tempo changes) for a specific project.
/// Like reaper-rs, it's lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> crate::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let tempo_map = project.tempo_map();
///
/// // Query tempo
/// let bpm = tempo_map.tempo_at(10.5).await?;
/// let (num, denom) = tempo_map.time_signature_at(10.5).await?;
///
/// // Time conversion
/// let (measure, beat, frac) = tempo_map.time_to_musical(30.0).await?;
/// let seconds = tempo_map.musical_to_time(4, 1, 0.0).await?;
///
/// // Add tempo changes
/// let idx = tempo_map.add_point(60.0, 140.0).await?;
/// tempo_map.set_time_signature_at(idx, 3, 4).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TempoMap {
    project_id: String,
    clients: Arc<DawClients>,
}

impl TempoMap {
    /// Create a new tempo map handle for a project
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

    /// Get all tempo points in the project
    pub async fn points(&self) -> Result<Vec<TempoPoint>> {
        let points = self
            .clients
            .tempo_map
            .get_tempo_points(self.context())
            .await?;
        Ok(points)
    }

    /// Get tempo point at a specific index
    pub async fn point(&self, index: u32) -> Result<Option<TempoPoint>> {
        let point = self
            .clients
            .tempo_map
            .get_tempo_point(self.context(), index)
            .await?;
        Ok(point)
    }

    /// Get the number of tempo points
    pub async fn count(&self) -> Result<usize> {
        let count = self
            .clients
            .tempo_map
            .tempo_point_count(self.context())
            .await?;
        Ok(count)
    }

    /// Get the tempo at a specific time position (interpolated if between points)
    pub async fn tempo_at(&self, seconds: f64) -> Result<f64> {
        let tempo = self
            .clients
            .tempo_map
            .get_tempo_at(self.context(), seconds)
            .await?;
        Ok(tempo)
    }

    /// Get the time signature at a specific time position
    pub async fn time_signature_at(&self, seconds: f64) -> Result<(i32, i32)> {
        let ts = self
            .clients
            .tempo_map
            .get_time_signature_at(self.context(), seconds)
            .await?;
        Ok(ts)
    }

    // =========================================================================
    // Time Conversion
    // =========================================================================

    /// Convert time position (seconds) to musical position (measure, beat, fraction)
    pub async fn time_to_musical(&self, seconds: f64) -> Result<(i32, i32, f64)> {
        let pos = self
            .clients
            .tempo_map
            .time_to_musical(self.context(), seconds)
            .await?;
        Ok(pos)
    }

    /// Convert musical position to time position in seconds
    pub async fn musical_to_time(&self, measure: i32, beat: i32, fraction: f64) -> Result<f64> {
        let seconds = self
            .clients
            .tempo_map
            .musical_to_time(self.context(), measure, beat, fraction)
            .await?;
        Ok(seconds)
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Add a tempo point at the given position
    ///
    /// Returns the index of the newly created tempo point.
    pub async fn add_point(&self, seconds: f64, bpm: f64) -> Result<u32> {
        let index = self
            .clients
            .tempo_map
            .add_tempo_point(self.context(), seconds, bpm)
            .await?;
        Ok(index)
    }

    /// Remove a tempo point by index
    pub async fn remove_point(&self, index: u32) -> Result<()> {
        self.clients
            .tempo_map
            .remove_tempo_point(self.context(), index)
            .await?;
        Ok(())
    }

    /// Set tempo at a specific point
    pub async fn set_tempo_at(&self, index: u32, bpm: f64) -> Result<()> {
        self.clients
            .tempo_map
            .set_tempo_at_point(self.context(), index, bpm)
            .await?;
        Ok(())
    }

    /// Set time signature at a specific point
    pub async fn set_time_signature_at(
        &self,
        index: u32,
        numerator: i32,
        denominator: i32,
    ) -> Result<()> {
        self.clients
            .tempo_map
            .set_time_signature_at_point(self.context(), index, numerator, denominator)
            .await?;
        Ok(())
    }

    /// Move a tempo point to a new position
    pub async fn move_point(&self, index: u32, seconds: f64) -> Result<()> {
        self.clients
            .tempo_map
            .move_tempo_point(self.context(), index, seconds)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Project Defaults
    // =========================================================================

    /// Get the project's default tempo (at position 0)
    pub async fn default_tempo(&self) -> Result<f64> {
        let tempo = self
            .clients
            .tempo_map
            .get_default_tempo(self.context())
            .await?;
        Ok(tempo)
    }

    /// Set the project's default tempo
    pub async fn set_default_tempo(&self, bpm: f64) -> Result<()> {
        self.clients
            .tempo_map
            .set_default_tempo(self.context(), bpm)
            .await?;
        Ok(())
    }

    /// Get the project's default time signature
    pub async fn default_time_signature(&self) -> Result<(i32, i32)> {
        let ts = self
            .clients
            .tempo_map
            .get_default_time_signature(self.context())
            .await?;
        Ok(ts)
    }

    /// Set the project's default time signature
    pub async fn set_default_time_signature(&self, numerator: i32, denominator: i32) -> Result<()> {
        self.clients
            .tempo_map
            .set_default_time_signature(self.context(), numerator, denominator)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to tempo map events (tempo points added, removed, changed, etc.)
    ///
    /// Returns a receiver that streams granular tempo map events for this project.
    /// The stream continues until the returned `Rx` is dropped.
    pub async fn subscribe(&self) -> Result<Rx<TempoMapEvent>> {
        let (tx, rx) = roam::channel::<TempoMapEvent>();
        self.clients
            .tempo_map
            .subscribe_tempo_map(self.context(), tx)
            .await?;
        Ok(rx)
    }
}

impl std::fmt::Debug for TempoMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TempoMap")
            .field("project_id", &self.project_id)
            .finish()
    }
}
