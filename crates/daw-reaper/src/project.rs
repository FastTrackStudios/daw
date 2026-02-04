//! REAPER Project Implementation
//!
//! Implements ProjectService by dispatching REAPER API calls to the main thread
//! using TaskSupport from reaper-high.

use daw_proto::{ProjectEvent, ProjectInfo, ProjectService};
use reaper_high::{Project, Reaper};
use reaper_medium::{CommandId, ProjectContext, ProjectRef};
use roam::{Context, Tx};
use std::time::Duration;
use tracing::{debug, info};

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

/// Get a project by tab index using medium_reaper's enum_projects
fn project_by_tab(reaper: &Reaper, tab_index: u32) -> Option<Project> {
    reaper
        .medium_reaper()
        .enum_projects(ProjectRef::Tab(tab_index), 0)
        .map(|result| Project::new(result.project))
}

/// Extract project info from a REAPER project
fn project_to_info(project: &Project) -> ProjectInfo {
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

    ProjectInfo { guid, name, path }
}

impl ProjectService for ReaperProject {
    async fn get_current(&self, _cx: &Context) -> Option<ProjectInfo> {
        debug!("ReaperProject: get_current");

        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let project = reaper.current_project();
                Some(project_to_info(&project))
            })
            .await
            .unwrap_or(None)
        } else {
            tracing::error!("TaskSupport not set");
            None
        }
    }

    async fn get(&self, _cx: &Context, project_id: String) -> Option<ProjectInfo> {
        debug!("ReaperProject: get({})", project_id);

        if let Some(ts) = task_support() {
            ts.main_thread_future(move || {
                let reaper = Reaper::get();

                // Iterate through all open project tabs
                for i in 0..128 {
                    if let Some(project) = project_by_tab(reaper, i) {
                        let info = project_to_info(&project);
                        if info.guid == project_id {
                            return Some(info);
                        }
                    } else {
                        // No more tabs
                        break;
                    }
                }
                None
            })
            .await
            .unwrap_or(None)
        } else {
            tracing::error!("TaskSupport not set");
            None
        }
    }

    async fn list(&self, _cx: &Context) -> Vec<ProjectInfo> {
        debug!("ReaperProject: list");

        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let mut projects = Vec::new();

                // Iterate through all open project tabs (max 128)
                for i in 0..128 {
                    if let Some(project) = project_by_tab(reaper, i) {
                        let info = project_to_info(&project);
                        // Skip routing/utility projects
                        if !info.name.to_uppercase().contains("FTS-ROUTING") {
                            projects.push(info);
                        }
                    } else {
                        // No more tabs
                        break;
                    }
                }

                info!("ReaperProject: list - found {} projects", projects.len());
                projects
            })
            .await
            .unwrap_or_else(|_| vec![])
        } else {
            tracing::error!("TaskSupport not set");
            vec![]
        }
    }

    async fn select(&self, _cx: &Context, project_id: String) -> bool {
        info!("ReaperProject: select({})", project_id);

        if let Some(ts) = task_support() {
            ts.main_thread_future(move || {
                let reaper = Reaper::get();

                // Find the tab index for the project with matching GUID
                let mut target_tab: Option<u32> = None;
                for i in 0..128u32 {
                    if let Some(project) = project_by_tab(reaper, i) {
                        let info = project_to_info(&project);
                        if info.guid == project_id {
                            target_tab = Some(i);
                            break;
                        }
                    } else {
                        break;
                    }
                }

                let Some(target) = target_tab else {
                    info!("ReaperProject: select - project {} not found", project_id);
                    return false;
                };

                // Get current tab index
                let current_project = reaper.current_project();
                let mut current_tab: Option<u32> = None;
                for i in 0..128u32 {
                    if let Some(project) = project_by_tab(reaper, i) {
                        if project == current_project {
                            current_tab = Some(i);
                            break;
                        }
                    } else {
                        break;
                    }
                }

                let Some(current) = current_tab else {
                    return false;
                };

                if current == target {
                    // Already on the correct tab
                    return true;
                }

                // Calculate shortest path (forward or backward)
                // Count total tabs first
                let mut total_tabs = 0u32;
                for i in 0..128u32 {
                    if project_by_tab(reaper, i).is_some() {
                        total_tabs = i + 1;
                    } else {
                        break;
                    }
                }

                let forward_distance = if target > current {
                    target - current
                } else {
                    total_tabs - current + target
                };

                let backward_distance = if current > target {
                    current - target
                } else {
                    current + total_tabs - target
                };

                // REAPER actions for tab switching
                let action_next_tab = CommandId::new(40861);
                let action_prev_tab = CommandId::new(40862);

                if forward_distance <= backward_distance {
                    // Go forward
                    for _ in 0..forward_distance {
                        reaper.medium_reaper().main_on_command_ex(
                            action_next_tab,
                            0,
                            ProjectContext::CurrentProject,
                        );
                    }
                } else {
                    // Go backward
                    for _ in 0..backward_distance {
                        reaper.medium_reaper().main_on_command_ex(
                            action_prev_tab,
                            0,
                            ProjectContext::CurrentProject,
                        );
                    }
                }

                // Verify we ended up at the right project
                let final_project = reaper.current_project();
                let final_info = project_to_info(&final_project);
                let success = final_info.guid == project_id;

                if success {
                    info!(
                        "ReaperProject: select - successfully switched to {}",
                        final_info.name
                    );
                } else {
                    tracing::warn!(
                        "ReaperProject: select - ended at {} instead of expected project",
                        final_info.name
                    );
                }

                success
            })
            .await
            .unwrap_or(false)
        } else {
            tracing::error!("TaskSupport not set");
            false
        }
    }

    async fn subscribe(&self, _cx: &Context, tx: Tx<ProjectEvent>) {
        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        let this = self.clone();
        tokio::spawn(async move {
            this.subscribe_impl(tx).await;
        });
    }
}

impl ReaperProject {
    /// Helper to get list of projects (used by subscribe)
    async fn get_project_list(&self) -> Vec<ProjectInfo> {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let mut projects = Vec::new();

                for i in 0..128 {
                    if let Some(project) = project_by_tab(reaper, i) {
                        let info = project_to_info(&project);
                        if !info.name.to_uppercase().contains("FTS-ROUTING") {
                            projects.push(info);
                        }
                    } else {
                        break;
                    }
                }
                projects
            })
            .await
            .unwrap_or_else(|_| vec![])
        } else {
            vec![]
        }
    }

    /// Helper to get current project GUID
    async fn get_current_guid(&self) -> Option<String> {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let project = reaper.current_project();
                Some(project_to_info(&project).guid)
            })
            .await
            .unwrap_or(None)
        } else {
            None
        }
    }

    async fn subscribe_impl(&self, tx: Tx<ProjectEvent>) {
        info!("ReaperProject::subscribe() - starting project stream");

        // Send initial state: all projects
        let projects = self.get_project_list().await;
        if tx
            .send(&ProjectEvent::ProjectsChanged(projects.clone()))
            .await
            .is_err()
        {
            debug!("ReaperProject::subscribe() - client disconnected during initial send");
            return;
        }

        // Send current project
        let current_guid = self.get_current_guid().await;
        if tx
            .send(&ProjectEvent::CurrentChanged(current_guid.clone()))
            .await
            .is_err()
        {
            debug!("ReaperProject::subscribe() - client disconnected");
            return;
        }

        // Poll for changes at 60Hz
        let mut last_guid = current_guid;
        let mut last_projects = projects;

        loop {
            tokio::time::sleep(Duration::from_micros(16667)).await;

            // Check for project list changes
            let current_projects = self.get_project_list().await;
            if current_projects != last_projects {
                if tx
                    .send(&ProjectEvent::ProjectsChanged(current_projects.clone()))
                    .await
                    .is_err()
                {
                    debug!("ReaperProject::subscribe() - client disconnected");
                    break;
                }
                last_projects = current_projects;
            }

            // Check for current project change
            let current_guid = self.get_current_guid().await;
            if current_guid != last_guid {
                if tx
                    .send(&ProjectEvent::CurrentChanged(current_guid.clone()))
                    .await
                    .is_err()
                {
                    debug!("ReaperProject::subscribe() - client disconnected");
                    break;
                }
                last_guid = current_guid;
            }
        }

        info!("ReaperProject::subscribe() - stream ended");
    }
}

#[allow(dead_code)]
fn hash_string(input: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}
