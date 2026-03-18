//! Automation handles for envelopes

use std::sync::Arc;

use crate::Result;
use crate::{DawClients, Error};
use daw_proto::{
    ProjectContext,
    automation::{
        AddPointParams, Envelope, EnvelopeLocation, EnvelopePoint, EnvelopeRef, EnvelopeShape,
        EnvelopeType, SetPointParams, TimeRangeParams,
    },
    primitives::{AutomationMode, PositionInSeconds},
    track::TrackRef,
};

/// Envelopes accessor for a track
#[derive(Clone)]
pub struct Envelopes {
    track_guid: String,
    project_id: String,
    clients: Arc<DawClients>,
}

impl Envelopes {
    /// Create a new envelopes handle
    pub(crate) fn new(track_guid: String, project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            track_guid,
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create track reference
    fn track_ref(&self) -> TrackRef {
        TrackRef::Guid(self.track_guid.clone())
    }

    /// Get all envelopes for this track
    pub async fn all(&self) -> Result<Vec<Envelope>> {
        let envelopes = self
            .clients
            .automation
            .get_envelopes(self.context(), self.track_ref())
            .await?;
        Ok(envelopes)
    }

    /// Get envelope by type
    pub async fn by_type(&self, envelope_type: EnvelopeType) -> Result<Option<EnvelopeHandle>> {
        let location = EnvelopeLocation::new(self.track_ref(), EnvelopeRef::Type(envelope_type));
        let envelope = self
            .clients
            .automation
            .get_envelope(self.context(), location)
            .await?;

        Ok(envelope.map(|_| {
            EnvelopeHandle::new(
                self.track_guid.clone(),
                EnvelopeRef::Type(envelope_type),
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get volume envelope
    pub fn volume(&self) -> EnvelopeHandle {
        EnvelopeHandle::new(
            self.track_guid.clone(),
            EnvelopeRef::Type(EnvelopeType::Volume),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get pan envelope
    pub fn pan(&self) -> EnvelopeHandle {
        EnvelopeHandle::new(
            self.track_guid.clone(),
            EnvelopeRef::Type(EnvelopeType::Pan),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get FX parameter envelope
    pub fn fx_param(&self, fx_guid: &str, param_index: u32) -> EnvelopeHandle {
        EnvelopeHandle::new(
            self.track_guid.clone(),
            EnvelopeRef::FxParam {
                fx_guid: fx_guid.to_string(),
                param_index,
            },
            self.project_id.clone(),
            self.clients.clone(),
        )
    }
}

impl std::fmt::Debug for Envelopes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelopes")
            .field("track_guid", &self.track_guid)
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// EnvelopeHandle
// =============================================================================

/// Handle to a single automation envelope
#[derive(Clone)]
pub struct EnvelopeHandle {
    track_guid: String,
    envelope_ref: EnvelopeRef,
    project_id: String,
    clients: Arc<DawClients>,
}

impl EnvelopeHandle {
    /// Create a new envelope handle
    pub(crate) fn new(
        track_guid: String,
        envelope_ref: EnvelopeRef,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            track_guid,
            envelope_ref,
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create envelope location
    fn location(&self) -> EnvelopeLocation {
        EnvelopeLocation::new(
            TrackRef::Guid(self.track_guid.clone()),
            self.envelope_ref.clone(),
        )
    }

    // =========================================================================
    // Info
    // =========================================================================

    /// Get full envelope state
    pub async fn info(&self) -> Result<Envelope> {
        self.clients
            .automation
            .get_envelope(self.context(), self.location())
            .await?
            .ok_or_else(|| Error::Other("Envelope not found".to_string()))
    }

    // =========================================================================
    // State
    // =========================================================================

    /// Set envelope visibility
    pub async fn set_visible(&self, visible: bool) -> Result<()> {
        self.clients
            .automation
            .set_visible(self.context(), self.location(), visible)
            .await?;
        Ok(())
    }

    /// Show the envelope
    pub async fn show(&self) -> Result<()> {
        self.set_visible(true).await
    }

    /// Hide the envelope
    pub async fn hide(&self) -> Result<()> {
        self.set_visible(false).await
    }

    /// Set envelope armed state
    pub async fn set_armed(&self, armed: bool) -> Result<()> {
        self.clients
            .automation
            .set_armed(self.context(), self.location(), armed)
            .await?;
        Ok(())
    }

    /// Arm the envelope for recording
    pub async fn arm(&self) -> Result<()> {
        self.set_armed(true).await
    }

    /// Disarm the envelope
    pub async fn disarm(&self) -> Result<()> {
        self.set_armed(false).await
    }

    /// Set automation mode
    pub async fn set_automation_mode(&self, mode: AutomationMode) -> Result<()> {
        self.clients
            .automation
            .set_automation_mode(self.context(), self.location(), mode)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Points
    // =========================================================================

    /// Get all points
    pub async fn points(&self) -> Result<Vec<EnvelopePoint>> {
        let points = self
            .clients
            .automation
            .get_points(self.context(), self.location())
            .await?;
        Ok(points)
    }

    /// Get points in a time range
    pub async fn points_in_range(
        &self,
        start: PositionInSeconds,
        end: PositionInSeconds,
    ) -> Result<Vec<EnvelopePoint>> {
        let points = self
            .clients
            .automation
            .get_points_in_range(
                self.context(),
                self.location(),
                TimeRangeParams::new(start, end),
            )
            .await?;
        Ok(points)
    }

    /// Get interpolated value at a time
    pub async fn value_at(&self, time: PositionInSeconds) -> Result<f64> {
        let value = self
            .clients
            .automation
            .get_value_at(self.context(), self.location(), time)
            .await?;
        Ok(value)
    }

    /// Add a point
    pub async fn add_point(
        &self,
        time: PositionInSeconds,
        value: f64,
        shape: EnvelopeShape,
    ) -> Result<u32> {
        let index = self
            .clients
            .automation
            .add_point(
                self.context(),
                self.location(),
                AddPointParams::new(time, value, shape),
            )
            .await?;
        Ok(index)
    }

    /// Add a point with linear shape
    pub async fn add_point_linear(&self, time: PositionInSeconds, value: f64) -> Result<u32> {
        self.add_point(time, value, EnvelopeShape::Linear).await
    }

    /// Delete a point
    pub async fn delete_point(&self, index: u32) -> Result<()> {
        self.clients
            .automation
            .delete_point(self.context(), self.location(), index)
            .await?;
        Ok(())
    }

    /// Set/update a point
    pub async fn set_point(
        &self,
        index: u32,
        time: PositionInSeconds,
        value: f64,
        shape: EnvelopeShape,
    ) -> Result<()> {
        self.clients
            .automation
            .set_point(
                self.context(),
                self.location(),
                SetPointParams {
                    index,
                    time,
                    value,
                    shape,
                },
            )
            .await?;
        Ok(())
    }

    /// Delete all points in a time range
    pub async fn delete_points_in_range(
        &self,
        start: PositionInSeconds,
        end: PositionInSeconds,
    ) -> Result<()> {
        self.clients
            .automation
            .delete_points_in_range(
                self.context(),
                self.location(),
                TimeRangeParams::new(start, end),
            )
            .await?;
        Ok(())
    }

    /// Clear all points
    pub async fn clear(&self) -> Result<()> {
        // Delete from 0 to a very large time
        self.delete_points_in_range(
            PositionInSeconds::ZERO,
            PositionInSeconds::from_seconds(86400.0 * 365.0), // ~1 year
        )
        .await
    }
}

impl std::fmt::Debug for EnvelopeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvelopeHandle")
            .field("track_guid", &self.track_guid)
            .field("envelope_ref", &self.envelope_ref)
            .field("project_id", &self.project_id)
            .finish()
    }
}
