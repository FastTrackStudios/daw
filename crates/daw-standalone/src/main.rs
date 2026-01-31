//! Standalone DAW Implementation
//!
//! This is a minimal DAW implementation that runs standalone without any external DAW.
//! It serves as both the reference implementation and the mock for testing.
//!
//! daw[impl daw.protocol]
//! This crate implements the DAW Protocol for a standalone (non-DAW) environment.

#![deny(unsafe_code)]

use daw_proto::{Transport, TransportState, TransportStateUpdate, TimePosition};
use roam::session::Tx;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

/// daw[impl transport.state.internal]
/// Internal state of the standalone transport.
///
/// Maintains the current transport state, position, and tempo.
#[derive(Debug, Clone)]
struct TransportStateInternal {
    /// daw[impl transport.state.stored]
    state: TransportState,
    /// daw[impl transport.position]
    position: TimePosition,
    /// daw[impl transport.tempo]
    tempo: f64,
}

impl Default for TransportStateInternal {
    fn default() -> Self {
        Self {
            state: TransportState::Stopped,
            position: TimePosition::from_seconds(0.0),
            tempo: 120.0,
        }
    }
}

/// daw[impl transport.service]
/// Standalone DAW transport implementation.
///
/// This implementation provides a reference for how the Transport service
/// should behave according to the DAW Protocol specification.
#[derive(Clone)]
pub struct StandaloneTransport {
    state: Arc<RwLock<TransportStateInternal>>,
    /// daw[impl transport.state.subscribers]
    update_tx: Arc<Mutex<Option<Tx<TransportStateUpdate>>>>,
}

impl StandaloneTransport {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(TransportStateInternal::default())),
            update_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// daw[impl transport.state.broadcast]
    /// Broadcast state updates to all subscribers.
    async fn broadcast_update(&self) {
        let state = self.state.read().await;
        let update = TransportStateUpdate {
            state: state.state,
            position: state.position,
            tempo: state.tempo,
        };
        
        if let Some(tx) = self.update_tx.lock().await.as_ref() {
            if let Err(e) = tx.send(&update).await {
                warn!("Failed to send state update: {}", e);
            }
        }
    }
}

impl Transport for StandaloneTransport {
    /// daw[impl transport.play]
    /// Start playback from the current position.
    ///
    /// Implements:
    /// - daw[transport.play.start]
    /// - daw[transport.play.from-position]
    /// - daw[transport.play.already-playing]
    #[tracing::instrument(skip(self, _cx))]
    async fn play(&self, _cx: &roam::Context) {
        let mut state = self.state.write().await;
        
        match state.state {
            TransportState::Playing => {
                // daw[impl transport.play.already-playing]
                warn!("Transport is already playing");
            }
            _ => {
                info!("Starting playback");
                // daw[impl transport.state.playing]
                state.state = TransportState::Playing;
                drop(state); // Release lock before broadcasting
                // daw[impl transport.state.broadcast]
                self.broadcast_update().await;
            }
        }
    }

    /// daw[impl transport.stop]
    /// Stop playback and maintain cursor position.
    ///
    /// Implements:
    /// - daw[transport.stop]
    /// - daw[transport.stop.maintain-position]
    /// - daw[transport.stop.already-stopped]
    #[tracing::instrument(skip(self, _cx))]
    async fn stop(&self, _cx: &roam::Context) {
        let mut state = self.state.write().await;
        
        match state.state {
            TransportState::Stopped => {
                // daw[impl transport.stop.already-stopped]
                warn!("Transport is already stopped");
            }
            _ => {
                info!("Stopping playback");
                // daw[impl transport.state.stopped]
                state.state = TransportState::Stopped;
                // daw[impl transport.stop.maintain-position]
                // Position is maintained (not modified)
                drop(state); // Release lock before broadcasting
                // daw[impl transport.state.broadcast]
                self.broadcast_update().await;
            }
        }
    }

    /// daw[impl transport.state.subscribe]
    /// Subscribe to transport state updates.
    ///
    /// Implements:
    /// - daw[transport.state.subscribe]
    /// - daw[transport.state.broadcast]
    /// - daw[transport.state.initial]
    /// - daw[transport.state.streaming]
    #[tracing::instrument(skip(self, _cx, updates))]
    async fn subscribe_state(&self, _cx: &roam::Context, updates: Tx<TransportStateUpdate>) {
        info!("New state subscription");
        // daw[impl transport.state.subscribers]
        *self.update_tx.lock().await = Some(updates);
        
        // daw[impl transport.state.initial]
        // Send current state immediately upon subscription
        self.broadcast_update().await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("DAW Standalone Cell starting...");

    // For now, just run the transport service implementation
    // In the full implementation, this would:
    // 1. Parse spawn args from the host
    // 2. Establish SHM connection
    // 3. Run the event loop
    
    let _transport = StandaloneTransport::new();
    
    info!("Standalone transport initialized");
    info!("Waiting for connections...");
    
    // Keep running
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    
    Ok(())
}