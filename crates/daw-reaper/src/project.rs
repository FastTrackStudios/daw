//! REAPER Project Implementation
//!
//! Implements ProjectService by dispatching REAPER API calls to the main thread
//! using TaskSupport from reaper-high.

use daw_proto::{ProjectInfo, ProjectService};
use reaper_high::Reaper;
use roam::Context;
use tracing::debug;

use crate::transport::task_support;

/// REAPER project implementation that dispatches to the main thread via TaskSupport.
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
        debug!("ReaperProject: get_current");

        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let project = reaper.current_project();

                let path = project.file().map(|p| p.to_string()).unwrap_or_default();
                let name = if path.is_empty() {
                    "Untitled".to_string()
                } else {
                    std::path::Path::new(&path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Untitled".to_string())
                };

                // Generate a stable GUID from the path
                let guid = format!("{:x}", hash_string(&path));

                Some(ProjectInfo { guid, name, path })
            })
            .await
            .unwrap_or(None)
        } else {
            tracing::error!("TaskSupport not set");
            None
        }
    }

    async fn get(&self, cx: &Context, project_id: String) -> Option<ProjectInfo> {
        debug!("ReaperProject: get({})", project_id);

        // For now, just return current project if ID matches
        let current = self.get_current(cx).await?;
        if current.guid == project_id {
            Some(current)
        } else {
            None
        }
    }

    async fn list(&self, cx: &Context) -> Vec<ProjectInfo> {
        debug!("ReaperProject: list");

        // For now, just return the current project
        if let Some(current) = self.get_current(cx).await {
            vec![current]
        } else {
            vec![]
        }
    }
}

fn hash_string(input: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}
