//! Standalone UI Service Implementation (Mock)

use daw_proto::{UiService, UserInputResult};
use std::path::PathBuf;

/// Standalone UI service (returns mock/empty results for testing)
#[derive(Clone)]
pub struct StandaloneUi;

impl StandaloneUi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StandaloneUi {
    fn default() -> Self {
        Self::new()
    }
}

impl UiService for StandaloneUi {
    async fn get_user_inputs(
        &self,
        _title: String,
        _prompts: Vec<String>,
        defaults: Vec<String>,
    ) -> Option<UserInputResult> {
        // In standalone mode, return defaults as if user clicked OK
        Some(UserInputResult {
            ok: true,
            values: defaults,
        })
    }

    async fn browse_for_file(
        &self,
        _title: String,
        _initial_dir: Option<PathBuf>,
        _filter: Option<String>,
    ) -> Option<PathBuf> {
        // Return a mock file path
        Some(PathBuf::from("/mock/file.wav"))
    }

    async fn browse_for_save_file(
        &self,
        _title: String,
        _initial_dir: Option<PathBuf>,
        default_name: String,
        _filter: Option<String>,
    ) -> Option<PathBuf> {
        // Return mock path with the default name
        Some(PathBuf::from(format!("/mock/{}", default_name)))
    }

    async fn browse_for_directory(
        &self,
        _title: String,
        _initial_dir: Option<PathBuf>,
    ) -> Option<PathBuf> {
        // Return a mock directory path
        Some(PathBuf::from("/mock/directory"))
    }

    async fn set_prevent_ui_refresh(&self, _prevent: bool) {
        // No-op in standalone mode
    }
}
