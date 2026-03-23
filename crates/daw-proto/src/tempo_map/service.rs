//! Tempo map service trait
//!
//! Defines the RPC interface for tempo and time signature management.

use super::{TempoMapEvent, TempoPoint};
use crate::ProjectContext;
use vox::{Tx, service};

/// Service for managing the tempo map in a DAW project
///
/// The tempo map defines how tempo and time signature change over time.
/// This enables conversion between time (seconds) and musical position
/// (measures/beats).
#[service]
pub trait TempoMapService {
    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all tempo points in the project
    async fn get_tempo_points(&self, project: ProjectContext) -> Vec<TempoPoint>;

    /// Get tempo point at a specific index
    async fn get_tempo_point(&self, project: ProjectContext, index: u32) -> Option<TempoPoint>;

    /// Get the number of tempo points
    async fn tempo_point_count(&self, project: ProjectContext) -> usize;

    /// Get the tempo at a specific time position (interpolated if between points)
    async fn get_tempo_at(&self, project: ProjectContext, seconds: f64) -> f64;

    /// Get the time signature at a specific time position
    async fn get_time_signature_at(&self, project: ProjectContext, seconds: f64) -> (i32, i32);

    /// Convert a time position (seconds) to quarter-note position
    async fn time_to_qn(&self, project: ProjectContext, seconds: f64) -> f64;

    /// Convert a quarter-note position to time position (seconds)
    async fn qn_to_time(&self, project: ProjectContext, qn: f64) -> f64;

    /// Convert time position to musical position (measure, beat, fraction)
    async fn time_to_musical(&self, project: ProjectContext, seconds: f64) -> (i32, i32, f64);

    /// Convert musical position to time position in seconds
    async fn musical_to_time(
        &self,
        project: ProjectContext,
        measure: i32,
        beat: i32,
        fraction: f64,
    ) -> f64;

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Add a tempo point at the given position
    ///
    /// To also set a time signature change, call `set_time_signature_at_point`
    /// after adding the tempo point.
    async fn add_tempo_point(&self, project: ProjectContext, seconds: f64, bpm: f64) -> u32;

    /// Remove a tempo point by index
    async fn remove_tempo_point(&self, project: ProjectContext, index: u32);

    /// Set tempo at a specific point
    async fn set_tempo_at_point(&self, project: ProjectContext, index: u32, bpm: f64);

    /// Set time signature at a specific point
    async fn set_time_signature_at_point(
        &self,
        project: ProjectContext,
        index: u32,
        numerator: i32,
        denominator: i32,
    );

    /// Move a tempo point to a new position
    async fn move_tempo_point(&self, project: ProjectContext, index: u32, seconds: f64);

    // =========================================================================
    // Project Defaults
    // =========================================================================

    /// Get the project's default tempo (at position 0)
    async fn get_default_tempo(&self, project: ProjectContext) -> f64;

    /// Set the project's default tempo
    async fn set_default_tempo(&self, project: ProjectContext, bpm: f64);

    /// Get the project's default time signature
    async fn get_default_time_signature(&self, project: ProjectContext) -> (i32, i32);

    /// Set the project's default time signature
    async fn set_default_time_signature(
        &self,
        project: ProjectContext,
        numerator: i32,
        denominator: i32,
    );

    // =========================================================================
    // Subscriptions
    // =========================================================================

    /// Subscribe to tempo map change events for a project.
    async fn subscribe_tempo_map(&self, project: ProjectContext, tx: Tx<TempoMapEvent>);
}
