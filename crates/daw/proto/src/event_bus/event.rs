//! Unified event types for the cross-domain `EventBus`.
//!
//! `DawEvent` wraps every per-domain stream event variant onto one
//! channel so consumers like OSC/MIDI bridges, the daw-cli inspector,
//! and web UIs can subscribe once and receive everything they're
//! interested in. `BusFilter` toggles each domain on or off so a
//! subscriber doesn't pay decode/transit cost for events it'll throw
//! away.
//!
//! The per-domain `Tracks::subscribe` / `Markers::subscribe` / etc.
//! traits remain — they're the right surface when a consumer cares
//! about exactly one domain.

use crate::fx::FxStreamEvent;
use crate::marker::MarkerStreamEvent;
use crate::project::ProjectStreamEvent;
use crate::region::RegionStreamEvent;
use crate::tempo_map::TempoMapStreamEvent;
use crate::track::TrackStreamEvent;
use crate::transport::{PositionTick, TransportEvent};
use facet::Facet;

/// One event from any subscribed domain. Each variant wraps the
/// existing per-domain stream type so no information is lost
/// compared to subscribing per-domain.
#[derive(Debug, Clone, Facet)]
#[repr(u8)]
pub enum DawEvent {
    Track(TrackStreamEvent),
    Fx(FxStreamEvent),
    Marker(MarkerStreamEvent),
    Region(RegionStreamEvent),
    TempoMap(TempoMapStreamEvent),
    TransportState(TransportEvent),
    TransportPosition(PositionTick),
    Project(ProjectStreamEvent),
}

/// Per-subscriber filter. Every flag defaults to off; callers opt in
/// to the domains they care about. `all()` / `everything_but_position()`
/// cover the common shapes — OSC bridges typically want everything,
/// UI inspectors typically skip the 30Hz position firehose.
///
/// `project_guid`, when set, scopes per-project domains (track,
/// marker, region, tempo_map, transport state + position) to a single
/// project at the publisher. Single-project clients save the wire
/// traffic of every other open tab. `Project` events always pass
/// through regardless — clients need them to know when to swap the
/// scoped guid.
#[derive(Clone, Debug, Default, Facet)]
pub struct BusFilter {
    pub tracks: bool,
    pub fx: bool,
    pub markers: bool,
    pub regions: bool,
    pub tempo_map: bool,
    pub transport_state: bool,
    pub transport_position: bool,
    pub projects: bool,
    pub project_guid: Option<String>,
}

impl BusFilter {
    pub const fn all() -> Self {
        Self {
            tracks: true,
            fx: true,
            markers: true,
            regions: true,
            tempo_map: true,
            transport_state: true,
            transport_position: true,
            projects: true,
            project_guid: None,
        }
    }

    pub const fn everything_but_position() -> Self {
        Self {
            tracks: true,
            fx: true,
            markers: true,
            regions: true,
            tempo_map: true,
            transport_state: true,
            transport_position: false,
            projects: true,
            project_guid: None,
        }
    }

    /// Narrow this filter to a single project — `track`, `marker`,
    /// `region`, `tempo_map`, transport state and position events
    /// outside that project are dropped at the publisher.
    pub fn for_project(mut self, guid: impl Into<String>) -> Self {
        self.project_guid = Some(guid.into());
        self
    }

    /// True when this filter is project-scoped and `guid` doesn't
    /// match. Per-domain forwarders call this to drop ticks for tabs
    /// the subscriber doesn't care about.
    pub fn project_rejects(&self, guid: &str) -> bool {
        self.project_guid
            .as_deref()
            .is_some_and(|want| want != guid)
    }

    /// True when at least one domain is enabled.
    pub fn any(&self) -> bool {
        self.tracks
            || self.fx
            || self.markers
            || self.regions
            || self.tempo_map
            || self.transport_state
            || self.transport_position
            || self.projects
    }
}

#[cfg(feature = "vox")]
#[allow(unsafe_code)]
mod reborrow_impls {
    use super::{BusFilter, DawEvent};
    unsafe impl vox_types::Reborrow for DawEvent {
        type Ref<'a> = DawEvent;
    }
    unsafe impl vox_types::Reborrow for BusFilter {
        type Ref<'a> = BusFilter;
    }
}
