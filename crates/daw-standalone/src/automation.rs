//! Standalone automation implementation

use daw_proto::{
    ProjectContext,
    automation::{
        AddPointParams, AutomationService, Envelope, EnvelopeLocation, EnvelopePoint, EnvelopeRef,
        EnvelopeShape, EnvelopeType, SetPointParams, TimeRangeParams,
    },
    primitives::{AutomationMode, PositionInSeconds},
    track::TrackRef,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Internal envelope state
#[derive(Clone)]
struct EnvelopeState {
    track_guid: String,
    envelope_type: EnvelopeType,
    name: String,
    visible: bool,
    armed: bool,
    automation_mode: AutomationMode,
    points: Vec<PointState>,
}

/// Internal point state
#[derive(Clone)]
struct PointState {
    index: u32,
    time: PositionInSeconds,
    value: f64,
    shape: EnvelopeShape,
    tension: f64,
    selected: bool,
}

impl EnvelopeState {
    fn to_envelope(&self) -> Envelope {
        Envelope {
            track_guid: self.track_guid.clone(),
            envelope_type: self.envelope_type,
            name: self.name.clone(),
            fx_guid: None,
            param_index: None,
            visible: self.visible,
            armed: self.armed,
            automation_mode: self.automation_mode,
            point_count: self.points.len() as u32,
        }
    }
}

impl PointState {
    fn to_point(&self) -> EnvelopePoint {
        EnvelopePoint {
            index: self.index,
            time: self.time,
            value: self.value,
            shape: self.shape,
            tension: self.tension,
            selected: self.selected,
        }
    }
}

/// Standalone automation service implementation
#[derive(Clone, Default)]
pub struct StandaloneAutomation {
    envelopes: Arc<RwLock<Vec<EnvelopeState>>>,
}

impl StandaloneAutomation {
    pub fn new() -> Self {
        Self {
            envelopes: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn find_envelope<'a>(
        envelopes: &'a mut [EnvelopeState],
        location: &EnvelopeLocation,
    ) -> Option<&'a mut EnvelopeState> {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return None,
        };
        envelopes.iter_mut().find(|e| {
            e.track_guid == track_guid
                && match &location.envelope {
                    EnvelopeRef::Type(t) => e.envelope_type == *t,
                    EnvelopeRef::ByName(n) => &e.name == n,
                    EnvelopeRef::FxParam { .. } => false,
                }
        })
    }
}

impl AutomationService for StandaloneAutomation {
    async fn get_envelopes(
        &self,
        _project: ProjectContext,
        track: TrackRef,
    ) -> Vec<Envelope> {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return vec![],
        };
        let envelopes = self.envelopes.read().await;
        envelopes
            .iter()
            .filter(|e| e.track_guid == track_guid)
            .map(|e| e.to_envelope())
            .collect()
    }

    async fn get_envelope(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
    ) -> Option<Envelope> {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return None,
        };
        let envelopes = self.envelopes.read().await;
        envelopes
            .iter()
            .find(|e| {
                e.track_guid == track_guid
                    && match &location.envelope {
                        EnvelopeRef::Type(t) => e.envelope_type == *t,
                        EnvelopeRef::ByName(n) => &e.name == n,
                        EnvelopeRef::FxParam { .. } => false,
                    }
            })
            .map(|e| e.to_envelope())
    }

    async fn set_visible(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        visible: bool,
    ) {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location) {
            e.visible = visible;
        }
    }

    async fn set_armed(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        armed: bool,
    ) {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location) {
            e.armed = armed;
        }
    }

    async fn set_automation_mode(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        mode: AutomationMode,
    ) {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location) {
            e.automation_mode = mode;
        }
    }

    async fn get_points(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
    ) -> Vec<EnvelopePoint> {
        let envelopes = self.envelopes.read().await;
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return vec![],
        };
        envelopes
            .iter()
            .find(|e| {
                e.track_guid == track_guid
                    && match &location.envelope {
                        EnvelopeRef::Type(t) => e.envelope_type == *t,
                        EnvelopeRef::ByName(n) => &e.name == n,
                        EnvelopeRef::FxParam { .. } => false,
                    }
            })
            .map(|e| e.points.iter().map(|p| p.to_point()).collect())
            .unwrap_or_default()
    }

    async fn get_points_in_range(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    ) -> Vec<EnvelopePoint> {
        let envelopes = self.envelopes.read().await;
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return vec![],
        };
        envelopes
            .iter()
            .find(|e| {
                e.track_guid == track_guid
                    && match &location.envelope {
                        EnvelopeRef::Type(t) => e.envelope_type == *t,
                        EnvelopeRef::ByName(n) => &e.name == n,
                        EnvelopeRef::FxParam { .. } => false,
                    }
            })
            .map(|e| {
                e.points
                    .iter()
                    .filter(|p| {
                        p.time.as_seconds() >= range.start.as_seconds()
                            && p.time.as_seconds() <= range.end.as_seconds()
                    })
                    .map(|p| p.to_point())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn get_value_at(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        time: PositionInSeconds,
    ) -> f64 {
        let envelopes = self.envelopes.read().await;
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return 0.0,
        };
        envelopes
            .iter()
            .find(|e| {
                e.track_guid == track_guid
                    && match &location.envelope {
                        EnvelopeRef::Type(t) => e.envelope_type == *t,
                        EnvelopeRef::ByName(n) => &e.name == n,
                        EnvelopeRef::FxParam { .. } => false,
                    }
            })
            .map(|e| {
                // Simple linear interpolation
                let t = time.as_seconds();
                let mut prev: Option<&PointState> = None;
                for p in &e.points {
                    if p.time.as_seconds() > t {
                        if let Some(prev) = prev {
                            let ratio = (t - prev.time.as_seconds())
                                / (p.time.as_seconds() - prev.time.as_seconds());
                            return prev.value + (p.value - prev.value) * ratio;
                        }
                        return p.value;
                    }
                    prev = Some(p);
                }
                prev.map(|p| p.value).unwrap_or(0.0)
            })
            .unwrap_or(0.0)
    }

    async fn add_point(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        params: AddPointParams,
    ) -> u32 {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location) {
            let index = e.points.len() as u32;
            e.points.push(PointState {
                index,
                time: params.time,
                value: params.value,
                shape: params.shape,
                tension: 0.0,
                selected: false,
            });
            // Sort by time
            e.points.sort_by(|a, b| {
                a.time
                    .as_seconds()
                    .partial_cmp(&b.time.as_seconds())
                    .unwrap()
            });
            // Re-index
            for (i, p) in e.points.iter_mut().enumerate() {
                p.index = i as u32;
            }
            return index;
        }
        0
    }

    async fn delete_point(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        index: u32,
    ) {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location) {
            e.points.retain(|p| p.index != index);
            // Re-index
            for (i, p) in e.points.iter_mut().enumerate() {
                p.index = i as u32;
            }
        }
    }

    async fn set_point(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        params: SetPointParams,
    ) {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location)
            && let Some(p) = e.points.iter_mut().find(|p| p.index == params.index)
        {
            p.time = params.time;
            p.value = params.value;
            p.shape = params.shape;
        }
    }

    async fn delete_points_in_range(
        &self,
        _project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    ) {
        let mut envelopes = self.envelopes.write().await;
        if let Some(e) = Self::find_envelope(&mut envelopes, &location) {
            e.points.retain(|p| {
                p.time.as_seconds() < range.start.as_seconds()
                    || p.time.as_seconds() > range.end.as_seconds()
            });
            // Re-index
            for (i, p) in e.points.iter_mut().enumerate() {
                p.index = i as u32;
            }
        }
    }

    async fn get_global_automation_override(
        &self,
        _project: ProjectContext,
    ) -> Option<AutomationMode> {
        None
    }

    async fn set_global_automation_override(
        &self,
        _project: ProjectContext,
        _mode: Option<AutomationMode>,
    ) {
        // Stub - no-op
    }
}
