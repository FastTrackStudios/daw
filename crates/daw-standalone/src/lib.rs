//! Standalone DAW Implementation
//!
//! This is a minimal DAW implementation that runs standalone without any external DAW.
//! It serves as both the reference implementation and the mock for testing.
//!
//! The implementations in this module (`StandaloneTransport`, `StandaloneProject`) can be
//! used directly in tests without spawning a separate cell process.

#![deny(unsafe_code)]

use daw_proto::{PlayState, ProjectInfo, ProjectService, TransportService};
use roam::Context;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

/// Standalone DAW transport implementation.
///
/// This is a minimal in-memory transport that tracks play/stop state.
/// It implements `TransportService` and can be used in tests or as a reference.
#[derive(Clone)]
pub struct StandaloneTransport {
    state: Arc<RwLock<PlayState>>,
}

impl Default for StandaloneTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneTransport {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(PlayState::Stopped)),
        }
    }

    /// Get the current play state (for testing assertions)
    pub async fn get_state(&self) -> PlayState {
        *self.state.read().await
    }

    /// Check if currently playing (convenience method for tests)
    pub async fn is_playing(&self) -> bool {
        *self.state.read().await == PlayState::Playing
    }
}

impl TransportService for StandaloneTransport {
    async fn play(&self, _cx: &Context, _project_id: Option<String>) {
        info!("Starting playback");
        let mut state = self.state.write().await;
        *state = PlayState::Playing;
    }

    async fn stop(&self, _cx: &Context, _project_id: Option<String>) {
        let mut state = self.state.write().await;
        *state = PlayState::Stopped;
        info!("Stopping playback");
    }
}

/// Standalone DAW project implementation.
///
/// This is a minimal in-memory project manager that provides a default project.
/// It implements `ProjectService` and can be used in tests or as a reference.
#[derive(Clone)]
pub struct StandaloneProject {
    default_project: Arc<ProjectInfo>,
}

impl Default for StandaloneProject {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneProject {
    pub fn new() -> Self {
        let default_project = ProjectInfo {
            guid: Uuid::new_v4().to_string(),
            name: "Untitled Project".to_string(),
            path: "/tmp/untitled.daw".to_string(),
        };

        Self {
            default_project: Arc::new(default_project),
        }
    }

    /// Create with a specific project (useful for tests that need predictable GUIDs)
    pub fn with_project(project: ProjectInfo) -> Self {
        Self {
            default_project: Arc::new(project),
        }
    }

    /// Get the default project info (for testing assertions)
    pub fn project_info(&self) -> &ProjectInfo {
        &self.default_project
    }
}

impl ProjectService for StandaloneProject {
    async fn get_current(&self, _cx: &Context) -> Option<ProjectInfo> {
        info!("ProjectService::get_current() called");
        Some(self.default_project.as_ref().clone())
    }

    async fn get(&self, _cx: &Context, project_id: String) -> Option<ProjectInfo> {
        info!(
            "ProjectService::get() called with project_id: {}",
            project_id
        );
        if project_id == self.default_project.guid {
            Some(self.default_project.as_ref().clone())
        } else {
            None
        }
    }

    async fn list(&self, _cx: &Context) -> Vec<ProjectInfo> {
        info!("ProjectService::list() called");
        vec![self.default_project.as_ref().clone()]
    }
}
