//! Automation service trait

use super::{Envelope, EnvelopeLocation, EnvelopePoint, EnvelopeShape};
use crate::primitives::{AutomationMode, PositionInSeconds};
use crate::project::ProjectContext;
use crate::track::TrackRef;
use facet::Facet;
use vox::service;

/// Parameters for adding an envelope point
#[derive(Clone, Debug, Facet)]
pub struct AddPointParams {
    /// Time position
    pub time: PositionInSeconds,
    /// Value (0.0-1.0)
    pub value: f64,
    /// Curve shape
    pub shape: EnvelopeShape,
}

impl AddPointParams {
    /// Create new point parameters
    pub fn new(time: PositionInSeconds, value: f64, shape: EnvelopeShape) -> Self {
        Self { time, value, shape }
    }

    /// Create with default linear shape
    pub fn linear(time: PositionInSeconds, value: f64) -> Self {
        Self::new(time, value, EnvelopeShape::Linear)
    }
}

/// Parameters for setting an envelope point
#[derive(Clone, Debug, Facet)]
pub struct SetPointParams {
    /// Point index
    pub index: u32,
    /// Time position
    pub time: PositionInSeconds,
    /// Value (0.0-1.0)
    pub value: f64,
    /// Curve shape
    pub shape: EnvelopeShape,
}

/// Time range for envelope operations
#[derive(Clone, Debug, Facet)]
pub struct TimeRangeParams {
    /// Start time
    pub start: PositionInSeconds,
    /// End time
    pub end: PositionInSeconds,
}

impl TimeRangeParams {
    /// Create a new time range
    pub fn new(start: PositionInSeconds, end: PositionInSeconds) -> Self {
        Self { start, end }
    }
}

/// Service for managing automation envelopes
#[service]
pub trait AutomationService {
    // === Envelope Queries ===

    /// Get all envelopes for a track
    async fn get_envelopes(&self, project: ProjectContext, track: TrackRef) -> Vec<Envelope>;

    /// Get a specific envelope
    async fn get_envelope(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
    ) -> Option<Envelope>;

    // === Envelope State ===

    /// Set envelope visibility
    async fn set_visible(&self, project: ProjectContext, location: EnvelopeLocation, visible: bool);

    /// Set envelope armed state
    async fn set_armed(&self, project: ProjectContext, location: EnvelopeLocation, armed: bool);

    /// Set envelope automation mode
    async fn set_automation_mode(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        mode: AutomationMode,
    );

    // === Point Queries ===

    /// Get all points in an envelope
    async fn get_points(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
    ) -> Vec<EnvelopePoint>;

    /// Get points within a time range
    async fn get_points_in_range(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    ) -> Vec<EnvelopePoint>;

    /// Get the interpolated value at a specific time
    async fn get_value_at(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        time: PositionInSeconds,
    ) -> f64;

    // === Point CRUD ===

    /// Add a point to an envelope, returns the point index
    async fn add_point(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        params: AddPointParams,
    ) -> u32;

    /// Delete a point by index
    async fn delete_point(&self, project: ProjectContext, location: EnvelopeLocation, index: u32);

    /// Set/update a point
    async fn set_point(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        params: SetPointParams,
    );

    /// Delete all points within a time range
    async fn delete_points_in_range(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    );

    // === Global ===

    /// Get global automation override (None if not overridden)
    async fn get_global_automation_override(
        &self,
        project: ProjectContext,
    ) -> Option<AutomationMode>;

    /// Set global automation override (None to clear)
    async fn set_global_automation_override(
        &self,
        project: ProjectContext,
        mode: Option<AutomationMode>,
    );
}
