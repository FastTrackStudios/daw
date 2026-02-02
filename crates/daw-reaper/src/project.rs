//! REAPER Project Implementation
//!
//! Implements ProjectService by queuing commands for main thread execution.
//! REAPER APIs can only be called from the main thread, so we use a callback
//! mechanism to dispatch commands.

use daw_proto::{ProjectInfo, ProjectService};
use roam::Context;
use std::sync::OnceLock;
use tracing::info;

/// Callback type for getting current project info from the main thread
pub type GetCurrentProjectCallback =
    Box<dyn Fn() -> tokio::sync::oneshot::Receiver<Option<ProjectInfo>> + Send + Sync>;

/// Global callback for getting current project
static GET_CURRENT_PROJECT_CALLBACK: OnceLock<GetCurrentProjectCallback> = OnceLock::new();

/// Set the callback for getting current project info from the main thread.
/// This should be called once during extension initialization.
pub fn set_get_current_project_callback<F>(callback: F)
where
    F: Fn() -> tokio::sync::oneshot::Receiver<Option<ProjectInfo>> + Send + Sync + 'static,
{
    if GET_CURRENT_PROJECT_CALLBACK
        .set(Box::new(callback))
        .is_err()
    {
        tracing::warn!("GetCurrentProject callback already set");
    }
}

/// REAPER project implementation that queues commands for main thread execution.
#[derive(Clone)]
pub struct ReaperProject;

impl ReaperProject {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperProject {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectService for ReaperProject {
    async fn get_current(&self, _cx: &Context) -> Option<ProjectInfo> {
        info!("REAPER Project: Queuing get_current command");

        if let Some(callback) = GET_CURRENT_PROJECT_CALLBACK.get() {
            let rx = callback();
            match rx.await {
                Ok(result) => result,
                Err(_) => {
                    tracing::error!("Failed to receive project info from main thread");
                    None
                }
            }
        } else {
            tracing::error!("GetCurrentProject callback not set");
            None
        }
    }

    async fn get(&self, cx: &Context, project_id: String) -> Option<ProjectInfo> {
        info!("REAPER Project: Getting project by ID: {}", project_id);

        // For now, just return current project if ID matches
        let current = self.get_current(cx).await?;
        if current.guid == project_id {
            Some(current)
        } else {
            None
        }
    }

    async fn list(&self, cx: &Context) -> Vec<ProjectInfo> {
        info!("REAPER Project: Listing projects");

        // For now, just return the current project
        if let Some(current) = self.get_current(cx).await {
            vec![current]
        } else {
            vec![]
        }
    }
}
