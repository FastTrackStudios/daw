//! `impl ResourcePaths for Standalone` — post-architect::rpc port.
//!
//! The standalone mock returns deterministic placeholder paths.

use std::path::PathBuf;

use daw_proto::ResourcePaths;

use crate::sync::Standalone;

impl ResourcePaths for Standalone {
    fn resource_path(&self) -> PathBuf {
        PathBuf::from("/mock/resource")
    }

    fn ini_file_path(&self) -> PathBuf {
        PathBuf::from("/mock/reaper.ini")
    }

    fn color_theme_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/mock/theme.ReaperTheme"))
    }
}
