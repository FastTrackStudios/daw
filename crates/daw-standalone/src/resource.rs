//! Standalone Resource Path Implementation (Mock)

use daw_proto::resource::ResourceService;
use roam::Context;
use std::path::PathBuf;

/// Standalone resource service (returns mock paths for testing)
#[derive(Clone)]
pub struct StandaloneResource;

impl StandaloneResource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StandaloneResource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceService for StandaloneResource {
    async fn get_resource_path(&self, _cx: &Context) -> PathBuf {
        PathBuf::from("/mock/resource")
    }

    async fn get_ini_file_path(&self, _cx: &Context) -> PathBuf {
        PathBuf::from("/mock/reaper.ini")
    }

    async fn get_color_theme_path(&self, _cx: &Context) -> Option<PathBuf> {
        Some(PathBuf::from("/mock/theme.ReaperTheme"))
    }
}
