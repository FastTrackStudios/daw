//! Standalone ExtState implementation (in-memory mock)

use daw_proto::{ExtStateService, ProjectContext};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

type GlobalStateMap = Arc<RwLock<HashMap<(String, String), String>>>;
type ProjectStateMap = Arc<RwLock<HashMap<(String, String, String), String>>>;

/// In-memory ext state for testing.
///
/// Stores global values in `global_state: HashMap<(section, key), value>`.
/// Stores per-project values in `project_state: HashMap<(project_guid, section, key), value>`.
/// The `persist` flag is accepted but ignored — all values are transient in the standalone mock.
#[derive(Clone)]
pub struct StandaloneExtState {
    global_state: GlobalStateMap,
    project_state: ProjectStateMap,
}

impl StandaloneExtState {
    pub fn new() -> Self {
        Self {
            global_state: Arc::new(RwLock::new(HashMap::new())),
            project_state: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for StandaloneExtState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtStateService for StandaloneExtState {
    async fn get_ext_state(&self, section: String, key: String) -> Option<String> {
        let state = self.global_state.read().unwrap();
        state.get(&(section, key)).cloned()
    }

    async fn set_ext_state(
        &self,
        section: String,
        key: String,
        value: String,
        _persist: bool,
    ) {
        let mut state = self.global_state.write().unwrap();
        state.insert((section, key), value);
    }

    async fn delete_ext_state(&self, section: String, key: String, _persist: bool) {
        let mut state = self.global_state.write().unwrap();
        state.remove(&(section, key));
    }

    async fn has_ext_state(&self, section: String, key: String) -> bool {
        let state = self.global_state.read().unwrap();
        state.contains_key(&(section, key))
    }

    // === Project-Scoped ExtState ===

    async fn get_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> Option<String> {
        let project_guid = match project {
            ProjectContext::Current => "current".to_string(),
            ProjectContext::Project(guid) => guid,
        };
        let state = self.project_state.read().unwrap();
        state.get(&(project_guid, section, key)).cloned()
    }

    async fn set_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
        value: String,
    ) {
        let project_guid = match project {
            ProjectContext::Current => "current".to_string(),
            ProjectContext::Project(guid) => guid,
        };
        let mut state = self.project_state.write().unwrap();
        state.insert((project_guid, section, key), value);
    }

    async fn delete_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) {
        let project_guid = match project {
            ProjectContext::Current => "current".to_string(),
            ProjectContext::Project(guid) => guid,
        };
        let mut state = self.project_state.write().unwrap();
        state.remove(&(project_guid, section, key));
    }

    async fn has_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> bool {
        let project_guid = match project {
            ProjectContext::Current => "current".to_string(),
            ProjectContext::Project(guid) => guid,
        };
        let state = self.project_state.read().unwrap();
        state.contains_key(&(project_guid, section, key))
    }
}
