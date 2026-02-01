//! DAW Reaper Implementation
//!
//! This crate provides a REAPER-specific implementation of the DAW Protocol.

#![deny(unsafe_code)]

use daw_proto::{TransportService, PlayState};
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::info;

/// Reaper DAW transport implementation
pub struct ReaperTransport {
    state: Arc<RwLock<PlayState>>,
}

impl ReaperTransport {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(PlayState::Stopped)),
        }
    }
}

impl TransportService for ReaperTransport {
    async fn play(&self, _cx: &roam::Context, _project_id: Option<String>) {
        let mut state = self.state.write().await;
        info!("Starting playback in REAPER");
        *state = PlayState::Playing;
    }

    async fn stop(&self, _cx: &roam::Context, _project_id: Option<String>) {
        let mut state = self.state.write().await;
        info!("Stopping playback in REAPER");
        *state = PlayState::Stopped;
    }
}

impl Default for ReaperTransport {
    fn default() -> Self {
        Self::new()
    }
}
