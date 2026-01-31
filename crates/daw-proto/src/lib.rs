//! DAW Protocol Definitions
//!
//! This crate defines the shared types and service interfaces for DAW cells.
//!
//! daw[define daw.protocol]
//! The DAW Protocol provides a standardized interface for controlling
//! Digital Audio Workstations via the Roam RPC framework.

#![deny(unsafe_code)]

use facet::Facet;
use roam::service;
use roam::session::Tx;

/// daw[define transport.state]
/// Current state of the transport playback.
///
/// States:
/// - Stopped: Playback is stopped
/// - Playing: Playback is active
/// - Paused: Playback is paused (optional)
/// - Recording: Recording is active (optional)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum TransportState {
    /// daw[define transport.state.stopped]
    Stopped = 0,
    /// daw[define transport.state.playing]
    Playing = 1,
    /// daw[define transport.state.paused]
    Paused = 2,
    /// daw[define transport.state.recording]
    Recording = 3,
}

/// daw[define transport.position]
/// Time position in the project timeline.
///
/// Position is measured in seconds from the project start.
#[derive(Debug, Clone, Copy, PartialEq, Facet)]
pub struct TimePosition {
    /// daw[define transport.position.seconds]
    /// Position in seconds from project start
    pub seconds: f64,
}

impl TimePosition {
    /// daw[define transport.position.from-seconds]
    /// Create a TimePosition from a seconds value.
    pub fn from_seconds(seconds: f64) -> Self {
        Self { seconds }
    }
}

/// daw[define transport.update]
/// Transport state update broadcast to subscribers.
///
/// Contains current state, position, and tempo information.
#[derive(Debug, Clone, Facet)]
pub struct TransportStateUpdate {
    /// daw[define transport.update.state]
    /// Current transport state
    pub state: TransportState,
    /// daw[define transport.update.position]
    /// Current playback position
    pub position: TimePosition,
    /// daw[define transport.update.tempo]
    /// Current tempo in BPM
    pub tempo: f64,
}

/// daw[spec transport]
/// Transport service for controlling playback state.
///
/// The Transport service provides:
/// - Playback control (play, stop)
/// - State streaming for real-time updates
/// - Position and tempo information
#[service]
pub trait Transport {
    /// daw[spec transport.play]
    /// Start playback from the current position.
    ///
    /// daw[impl transport.play.start]
    /// daw[impl transport.play.from-position]
    async fn play(&self);
    
    /// daw[spec transport.stop]
    /// Stop playback and maintain cursor position.
    ///
    /// daw[impl transport.stop]
    /// daw[impl transport.stop.maintain-position]
    async fn stop(&self);
    
    /// daw[spec transport.state.subscribe]
    /// Subscribe to transport state updates.
    ///
    /// Clients receive updates whenever the transport state changes.
    ///
    /// daw[impl transport.state.subscribe]
    /// daw[impl transport.state.broadcast]
    /// daw[impl transport.state.initial]
    /// daw[impl transport.state.streaming]
    async fn subscribe_state(&self, updates: Tx<TransportStateUpdate>);
}