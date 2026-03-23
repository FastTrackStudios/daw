//! Routing service trait

use super::{RouteRef, RouteType, RoutingEvent, SendMode, TrackRoute};
use crate::project::ProjectContext;
use crate::track::TrackRef;
use vox::{Tx, service};

/// Specifies a route location (track + route type + route reference)
#[derive(Clone, Debug, facet::Facet)]
pub struct RouteLocation {
    /// The track containing the route
    pub track: TrackRef,
    /// Type of route (send, receive, hardware output)
    pub route_type: RouteType,
    /// Reference to the specific route
    pub route: RouteRef,
}

impl RouteLocation {
    /// Create a new route location
    pub fn new(track: TrackRef, route_type: RouteType, route: RouteRef) -> Self {
        Self {
            track,
            route_type,
            route,
        }
    }

    /// Create a send route location
    pub fn send(track: TrackRef, route: RouteRef) -> Self {
        Self::new(track, RouteType::Send, route)
    }

    /// Create a receive route location
    pub fn receive(track: TrackRef, route: RouteRef) -> Self {
        Self::new(track, RouteType::Receive, route)
    }

    /// Create a hardware output route location
    pub fn hardware_output(track: TrackRef, route: RouteRef) -> Self {
        Self::new(track, RouteType::HardwareOutput, route)
    }
}

/// Service for managing track routing (sends, receives, hardware outputs)
#[service]
pub trait RoutingService {
    // === Queries ===

    /// Get all sends from a track
    async fn get_sends(&self, project: ProjectContext, track: TrackRef) -> Vec<TrackRoute>;

    /// Get all receives to a track
    async fn get_receives(&self, project: ProjectContext, track: TrackRef) -> Vec<TrackRoute>;

    /// Get all hardware outputs from a track
    async fn get_hardware_outputs(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute>;

    /// Get a specific route
    async fn get_route(
        &self,
        project: ProjectContext,
        location: RouteLocation,
    ) -> Option<TrackRoute>;

    // === CRUD ===

    /// Add a send from source track to destination track
    /// Returns the index of the new send
    async fn add_send(
        &self,
        project: ProjectContext,
        source: TrackRef,
        dest: TrackRef,
    ) -> Option<u32>;

    /// Add a hardware output to a track
    /// Returns the index of the new hardware output
    async fn add_hardware_output(
        &self,
        project: ProjectContext,
        track: TrackRef,
        hw_output: u32,
    ) -> Option<u32>;

    /// Remove a route
    async fn remove_route(&self, project: ProjectContext, location: RouteLocation);

    // === Levels ===

    /// Set route volume
    async fn set_volume(&self, project: ProjectContext, location: RouteLocation, volume: f64);

    /// Set route pan
    async fn set_pan(&self, project: ProjectContext, location: RouteLocation, pan: f64);

    // === State ===

    /// Set route mute state
    async fn set_muted(&self, project: ProjectContext, location: RouteLocation, muted: bool);

    /// Set route mono state
    async fn set_mono(&self, project: ProjectContext, location: RouteLocation, mono: bool);

    /// Set route phase inversion
    async fn set_phase(&self, project: ProjectContext, location: RouteLocation, inverted: bool);

    // === Mode ===

    /// Set send mode (pre-fx, post-fx, post-fader)
    async fn set_send_mode(
        &self,
        project: ProjectContext,
        track: TrackRef,
        route: RouteRef,
        mode: SendMode,
    );

    // === Channel Mapping ===

    /// Set source audio channels for a route (0-indexed start channel, num channels)
    async fn set_source_channels(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        start_channel: u32,
        num_channels: u32,
    );

    /// Set destination audio channels for a route (0-indexed start channel, num channels)
    async fn set_dest_channels(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        start_channel: u32,
        num_channels: u32,
    );

    // === Parent Send (Folder routing) ===

    /// Get whether parent send is enabled (for folder track routing)
    async fn get_parent_send_enabled(&self, project: ProjectContext, track: TrackRef) -> bool;

    /// Set whether parent send is enabled
    async fn set_parent_send_enabled(
        &self,
        project: ProjectContext,
        track: TrackRef,
        enabled: bool,
    );

    // === Subscriptions ===

    /// Subscribe to routing change events for a project.
    async fn subscribe_routing(&self, project: ProjectContext, tx: Tx<RoutingEvent>);
}
