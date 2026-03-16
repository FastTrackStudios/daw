//! Standalone project implementation

use crate::transport::SharedProjectState;
use daw_proto::{ProjectEvent, ProjectInfo, ProjectService};
use roam::Tx;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Well-known project GUIDs for testing
pub mod project_guids {
    pub const SONG1: &str = "project-song1-how-great-is-our-god";
    pub const SONG2: &str = "project-song2-holy-holy-holy";
    pub const SONG3: &str = "project-song3-amazing-grace";
}

/// Standalone DAW project implementation.
///
/// This is a minimal in-memory project manager that provides mock projects
/// for testing. It implements `ProjectService` and can be used in tests
/// or as a reference.
///
/// Returns 3 mock projects representing songs in a setlist:
/// - "How Great is Our God"
/// - "Holy, Holy, Holy"
/// - "Amazing Grace"
#[derive(Clone)]
pub struct StandaloneProject {
    projects: Arc<Vec<ProjectInfo>>,
    /// Shared state with transport service
    shared_state: SharedProjectState,
}

impl Default for StandaloneProject {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneProject {
    pub fn new() -> Self {
        let projects = vec![
            ProjectInfo {
                guid: project_guids::SONG1.to_string(),
                name: "How Great is Our God".to_string(),
                path: "/projects/how-great-is-our-god.rpp".to_string(),
            },
            ProjectInfo {
                guid: project_guids::SONG2.to_string(),
                name: "Holy, Holy, Holy".to_string(),
                path: "/projects/holy-holy-holy.rpp".to_string(),
            },
            ProjectInfo {
                guid: project_guids::SONG3.to_string(),
                name: "Amazing Grace".to_string(),
                path: "/projects/amazing-grace.rpp".to_string(),
            },
        ];

        let project_guids: Vec<String> = projects.iter().map(|p| p.guid.clone()).collect();
        let shared_state = SharedProjectState {
            project_guids: Arc::new(project_guids),
            current_index: Arc::new(RwLock::new(0)),
        };

        Self {
            projects: Arc::new(projects),
            shared_state,
        }
    }

    /// Create with specific projects (useful for tests)
    pub fn with_projects(projects: Vec<ProjectInfo>) -> Self {
        let project_guids: Vec<String> = projects.iter().map(|p| p.guid.clone()).collect();
        let shared_state = SharedProjectState {
            project_guids: Arc::new(project_guids),
            current_index: Arc::new(RwLock::new(0)),
        };

        Self {
            projects: Arc::new(projects),
            shared_state,
        }
    }

    /// Get the shared project state for use by the transport service
    pub fn shared_state(&self) -> SharedProjectState {
        self.shared_state.clone()
    }

    /// Get a project by index (for testing assertions)
    pub fn project_info(&self, index: usize) -> Option<&ProjectInfo> {
        self.projects.get(index)
    }

    /// Get the current project index (for testing)
    pub async fn current_index(&self) -> usize {
        *self.shared_state.current_index.read().await
    }
}

impl ProjectService for StandaloneProject {
    async fn get_current(&self) -> Option<ProjectInfo> {
        let index = *self.shared_state.current_index.read().await;
        info!(
            "ProjectService::get_current() called - returning project at index {}",
            index
        );
        self.projects.get(index).cloned()
    }

    async fn get(&self, project_id: String) -> Option<ProjectInfo> {
        info!(
            "ProjectService::get() called with project_id: {}",
            project_id
        );
        self.projects.iter().find(|p| p.guid == project_id).cloned()
    }

    async fn list(&self) -> Vec<ProjectInfo> {
        info!(
            "ProjectService::list() called - returning {} projects",
            self.projects.len()
        );
        self.projects.as_ref().clone()
    }

    async fn select(&self, project_id: String) -> bool {
        tracing::debug!(
            "ProjectService::select() called with project_id: {}",
            project_id
        );

        // Find the project index by GUID
        if let Some(index) = self.projects.iter().position(|p| p.guid == project_id) {
            let prev_index = *self.shared_state.current_index.read().await;
            let mut current = self.shared_state.current_index.write().await;
            *current = index;
            info!(
                "ProjectService::select() - switched from project {} to project {} (index {})",
                prev_index, project_id, index
            );
            true
        } else {
            tracing::warn!(
                "ProjectService::select() - project {} not found in {} projects",
                project_id,
                self.projects.len()
            );
            false
        }
    }

    async fn create(&self) -> Option<ProjectInfo> {
        info!("ProjectService::create() called (standalone stub)");
        let guid = format!(
            "project-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        Some(ProjectInfo {
            guid,
            name: "New Project".to_string(),
            path: String::new(),
        })
    }

    async fn open(&self, path: String) -> Option<ProjectInfo> {
        info!("ProjectService::open({}) called (standalone stub)", path);
        None
    }

    async fn close(&self, project_id: String) -> bool {
        info!(
            "ProjectService::close({}) called (standalone stub)",
            project_id
        );
        true
    }

    async fn get_by_slot(&self, slot: u32) -> Option<ProjectInfo> {
        info!("ProjectService::get_by_slot({}) called", slot);
        self.projects.get(slot as usize).cloned()
    }

    // Undo stubs (no-op for standalone mock)

    async fn begin_undo_block(
        &self,
        _project: daw_proto::ProjectContext,
        _label: String,
    ) {
    }

    async fn end_undo_block(
        &self,
        _project: daw_proto::ProjectContext,
        _label: String,
        _scope: Option<daw_proto::UndoScope>,
    ) {
    }

    async fn undo(&self, _project: daw_proto::ProjectContext) -> bool {
        false
    }

    async fn redo(&self, _project: daw_proto::ProjectContext) -> bool {
        false
    }

    async fn last_undo_label(
        &self,
        _project: daw_proto::ProjectContext,
    ) -> Option<String> {
        None
    }

    async fn last_redo_label(
        &self,
        _project: daw_proto::ProjectContext,
    ) -> Option<String> {
        None
    }

    async fn run_command(&self, _project: daw_proto::ProjectContext, _command: String) -> bool {
        false
    }

    async fn save(&self, _project: daw_proto::ProjectContext) {}

    async fn save_all(&self) {}

    async fn set_ruler_lane_name(
        &self,
        _project: daw_proto::ProjectContext,
        _lane_index: u32,
        _name: String,
    ) {
    }

    async fn get_ruler_lane_name(
        &self,
        _project: daw_proto::ProjectContext,
        _lane_index: u32,
    ) -> String {
        String::new()
    }

    async fn ruler_lane_count(&self, _project: daw_proto::ProjectContext) -> u32 {
        0
    }

    async fn subscribe(&self, tx: Tx<ProjectEvent>) {
        info!("ProjectService::subscribe() - starting project stream");

        // Clone self for the spawned task
        let this = self.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        moire::task::spawn(async move {
            // Send initial state: all projects
            let projects = this.projects.as_ref().clone();
            if tx
                .send(ProjectEvent::ProjectsChanged(projects))
                .await
                .is_err()
            {
                debug!("ProjectService::subscribe() - client disconnected during initial send");
                return;
            }

            // Send current project
            let current_guid = {
                let index = *this.shared_state.current_index.read().await;
                this.projects.get(index).map(|p| p.guid.clone())
            };
            if tx
                .send(ProjectEvent::CurrentChanged(current_guid.clone()))
                .await
                .is_err()
            {
                debug!("ProjectService::subscribe() - client disconnected");
                return;
            }

            // Poll for changes at 2Hz (current project rarely changes, no need for 60Hz)
            let mut last_index = *this.shared_state.current_index.read().await;

            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Check for current project change
                let current_index = *this.shared_state.current_index.read().await;
                if current_index != last_index {
                    last_index = current_index;
                    let new_guid = this.projects.get(current_index).map(|p| p.guid.clone());
                    if tx
                        .send(ProjectEvent::CurrentChanged(new_guid))
                        .await
                        .is_err()
                    {
                        debug!("ProjectService::subscribe() - client disconnected");
                        break;
                    }
                }
            }

            info!("ProjectService::subscribe() - stream ended");
        });
    }
}
