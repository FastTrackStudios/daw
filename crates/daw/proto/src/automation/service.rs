//! Automation service — envelopes and points.

use super::{Envelope, EnvelopeLocation, EnvelopePoint, EnvelopeShape};
use crate::DawResult;
use crate::primitives::{AutomationMode, PositionInSeconds};
use crate::project::ProjectContext;
use crate::track::TrackRef;
use facet::Facet;

/// Parameters for adding an envelope point.
#[derive(Clone, Debug, Facet)]
pub struct AddPointParams {
    pub time: PositionInSeconds,
    /// Normalized value (0.0–1.0).
    pub value: f64,
    pub shape: EnvelopeShape,
}

impl AddPointParams {
    pub fn new(time: PositionInSeconds, value: f64, shape: EnvelopeShape) -> Self {
        Self { time, value, shape }
    }

    pub fn linear(time: PositionInSeconds, value: f64) -> Self {
        Self::new(time, value, EnvelopeShape::Linear)
    }
}

/// Parameters for setting an envelope point.
#[derive(Clone, Debug, Facet)]
pub struct SetPointParams {
    pub index: u32,
    pub time: PositionInSeconds,
    pub value: f64,
    pub shape: EnvelopeShape,
}

/// Time range for envelope operations.
#[derive(Clone, Debug, Facet)]
pub struct TimeRangeParams {
    pub start: PositionInSeconds,
    pub end: PositionInSeconds,
}

impl TimeRangeParams {
    pub fn new(start: PositionInSeconds, end: PositionInSeconds) -> Self {
        Self { start, end }
    }
}

#[architect::rpc]
pub trait Automation {
    // ── Envelopes ──────────────────────────────────────────────────

    fn envelopes(&self, project: ProjectContext, track: TrackRef) -> Vec<Envelope>;

    fn envelope(&self, project: ProjectContext, location: EnvelopeLocation) -> Option<Envelope>;

    fn set_visible(&self, project: ProjectContext, location: EnvelopeLocation, visible: bool);
    fn set_armed(&self, project: ProjectContext, location: EnvelopeLocation, armed: bool);
    fn set_automation_mode(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        mode: AutomationMode,
    );

    // ── Points ─────────────────────────────────────────────────────

    fn points(&self, project: ProjectContext, location: EnvelopeLocation) -> Vec<EnvelopePoint>;

    fn points_in_range(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    ) -> Vec<EnvelopePoint>;

    /// Interpolated value at a specific time.
    fn value_at(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        time: PositionInSeconds,
    ) -> f64;

    /// Add a point. Returns the point index.
    /// Mark a parameter's control as touched (Touch/Latch automation
    /// gating). Surfaces call this from fader-touch sensors.
    fn touch_param(&self, project: ProjectContext, location: EnvelopeLocation) -> DawResult<()>;

    /// Release a touched parameter.
    fn release_param(&self, project: ProjectContext, location: EnvelopeLocation) -> DawResult<()>;

    /// Write a parameter value through the automation engine: updates
    /// the static value AND records an envelope point when the mode +
    /// touch state + transport allow (REAPER's touch/latch/write).
    fn write_param(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        value: f64,
    ) -> DawResult<()>;

    fn add_point(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        params: AddPointParams,
    ) -> u32;

    fn delete_point(&self, project: ProjectContext, location: EnvelopeLocation, index: u32);

    fn set_point(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        params: SetPointParams,
    );

    fn delete_points_in_range(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    );

    // ── Global ─────────────────────────────────────────────────────

    /// Global automation override (`None` if not overridden).
    fn global_automation_override(&self, project: ProjectContext) -> Option<AutomationMode>;

    /// Set global automation override (`None` to clear).
    fn set_global_automation_override(&self, project: ProjectContext, mode: Option<AutomationMode>);
}
