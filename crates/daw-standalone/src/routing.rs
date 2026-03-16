//! Standalone routing implementation

use daw_proto::{
    ProjectContext,
    primitives::AutomationMode,
    routing::{
        ChannelMapping, RouteLocation, RouteRef, RouteType, RoutingEvent, RoutingService, SendMode,
        TrackRoute,
    },
    track::TrackRef,
};
use roam::Tx;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Internal route state
#[derive(Clone)]
struct RouteState {
    index: u32,
    route_type: RouteType,
    source_track_guid: String,
    dest_track_guid: Option<String>,
    volume: f64,
    pan: f64,
    muted: bool,
    mono: bool,
    phase_inverted: bool,
    send_mode: SendMode,
}

impl RouteState {
    fn to_route(&self) -> TrackRoute {
        TrackRoute {
            index: self.index,
            route_type: self.route_type,
            source_track_guid: self.source_track_guid.clone(),
            dest_track_guid: self.dest_track_guid.clone(),
            dest_track_name: None,
            hw_output_index: None,
            hw_output_name: None,
            volume: self.volume,
            pan: self.pan,
            muted: self.muted,
            mono: self.mono,
            phase_inverted: self.phase_inverted,
            send_mode: self.send_mode,
            automation_mode: AutomationMode::TrimRead,
            source_channels: ChannelMapping::default(),
            dest_channels: ChannelMapping::default(),
            midi_channel_mapping: None,
        }
    }
}

/// Standalone routing service implementation
#[derive(Clone, Default)]
pub struct StandaloneRouting {
    routes: Arc<RwLock<Vec<RouteState>>>,
}

impl StandaloneRouting {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl RoutingService for StandaloneRouting {
    async fn get_sends(
        &self,
        _project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute> {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return vec![],
        };
        let routes = self.routes.read().await;
        routes
            .iter()
            .filter(|r| r.source_track_guid == track_guid && r.route_type == RouteType::Send)
            .map(|r| r.to_route())
            .collect()
    }

    async fn get_receives(
        &self,
        _project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute> {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return vec![],
        };
        let routes = self.routes.read().await;
        routes
            .iter()
            .filter(|r| {
                r.dest_track_guid.as_ref() == Some(&track_guid)
                    && r.route_type == RouteType::Receive
            })
            .map(|r| r.to_route())
            .collect()
    }

    async fn get_hardware_outputs(
        &self,
        _project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute> {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return vec![],
        };
        let routes = self.routes.read().await;
        routes
            .iter()
            .filter(|r| {
                r.source_track_guid == track_guid && r.route_type == RouteType::HardwareOutput
            })
            .map(|r| r.to_route())
            .collect()
    }

    async fn get_route(
        &self,
        _project: ProjectContext,
        location: RouteLocation,
    ) -> Option<TrackRoute> {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return None,
        };
        let routes = self.routes.read().await;
        routes
            .iter()
            .find(|r| {
                r.source_track_guid == track_guid
                    && r.route_type == location.route_type
                    && match &location.route {
                        RouteRef::Index(i) => r.index == *i,
                        RouteRef::ByDestination(dest) => match dest {
                            TrackRef::Guid(g) => r.dest_track_guid.as_ref() == Some(g),
                            _ => false,
                        },
                    }
            })
            .map(|r| r.to_route())
    }

    async fn add_send(
        &self,
        _project: ProjectContext,
        source: TrackRef,
        dest: TrackRef,
    ) -> Option<u32> {
        let source_guid = match source {
            TrackRef::Guid(g) => g,
            _ => return None,
        };
        let dest_guid = match dest {
            TrackRef::Guid(g) => g,
            _ => return None,
        };
        let mut routes = self.routes.write().await;
        let index = routes.len() as u32;
        routes.push(RouteState {
            index,
            route_type: RouteType::Send,
            source_track_guid: source_guid,
            dest_track_guid: Some(dest_guid),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            mono: false,
            phase_inverted: false,
            send_mode: SendMode::PostFader,
        });
        Some(index)
    }

    async fn add_hardware_output(
        &self,
        _project: ProjectContext,
        track: TrackRef,
        _hw_output: u32,
    ) -> Option<u32> {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return None,
        };
        let mut routes = self.routes.write().await;
        let index = routes.len() as u32;
        routes.push(RouteState {
            index,
            route_type: RouteType::HardwareOutput,
            source_track_guid: track_guid,
            dest_track_guid: None,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            mono: false,
            phase_inverted: false,
            send_mode: SendMode::PostFader,
        });
        Some(index)
    }

    async fn remove_route(&self, _project: ProjectContext, location: RouteLocation) {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return,
        };
        let mut routes = self.routes.write().await;
        routes.retain(|r| {
            !(r.source_track_guid == track_guid
                && r.route_type == location.route_type
                && match &location.route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                })
        });
    }

    async fn set_volume(
        &self,
        _project: ProjectContext,
        location: RouteLocation,
        volume: f64,
    ) {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return,
        };
        let mut routes = self.routes.write().await;
        if let Some(r) = routes.iter_mut().find(|r| {
            r.source_track_guid == track_guid
                && r.route_type == location.route_type
                && match &location.route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                }
        }) {
            r.volume = volume;
        }
    }

    async fn set_pan(
        &self,
        _project: ProjectContext,
        location: RouteLocation,
        pan: f64,
    ) {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return,
        };
        let mut routes = self.routes.write().await;
        if let Some(r) = routes.iter_mut().find(|r| {
            r.source_track_guid == track_guid
                && r.route_type == location.route_type
                && match &location.route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                }
        }) {
            r.pan = pan;
        }
    }

    async fn set_muted(
        &self,
        _project: ProjectContext,
        location: RouteLocation,
        muted: bool,
    ) {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return,
        };
        let mut routes = self.routes.write().await;
        if let Some(r) = routes.iter_mut().find(|r| {
            r.source_track_guid == track_guid
                && r.route_type == location.route_type
                && match &location.route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                }
        }) {
            r.muted = muted;
        }
    }

    async fn set_mono(
        &self,
        _project: ProjectContext,
        location: RouteLocation,
        mono: bool,
    ) {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return,
        };
        let mut routes = self.routes.write().await;
        if let Some(r) = routes.iter_mut().find(|r| {
            r.source_track_guid == track_guid
                && r.route_type == location.route_type
                && match &location.route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                }
        }) {
            r.mono = mono;
        }
    }

    async fn set_phase(
        &self,
        _project: ProjectContext,
        location: RouteLocation,
        inverted: bool,
    ) {
        let track_guid = match &location.track {
            TrackRef::Guid(g) => g.clone(),
            _ => return,
        };
        let mut routes = self.routes.write().await;
        if let Some(r) = routes.iter_mut().find(|r| {
            r.source_track_guid == track_guid
                && r.route_type == location.route_type
                && match &location.route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                }
        }) {
            r.phase_inverted = inverted;
        }
    }

    async fn set_send_mode(
        &self,
        _project: ProjectContext,
        track: TrackRef,
        route: RouteRef,
        mode: SendMode,
    ) {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return,
        };
        let mut routes = self.routes.write().await;
        if let Some(r) = routes.iter_mut().find(|r| {
            r.source_track_guid == track_guid
                && r.route_type == RouteType::Send
                && match &route {
                    RouteRef::Index(i) => r.index == *i,
                    RouteRef::ByDestination(_) => false,
                }
        }) {
            r.send_mode = mode;
        }
    }

    async fn set_source_channels(
        &self,
        _project: ProjectContext,
        _location: RouteLocation,
        _start_channel: u32,
        _num_channels: u32,
    ) {
        // Standalone: no-op
    }

    async fn set_dest_channels(
        &self,
        _project: ProjectContext,
        _location: RouteLocation,
        _start_channel: u32,
        _num_channels: u32,
    ) {
        // Standalone: no-op
    }

    async fn get_parent_send_enabled(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
    ) -> bool {
        true // Default to enabled
    }

    async fn set_parent_send_enabled(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _enabled: bool,
    ) {
        // Stub - no-op
    }

    async fn subscribe_routing(&self, _project: ProjectContext, _tx: Tx<RoutingEvent>) {}
}
