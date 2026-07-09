//! REAPER resource-path service.

use std::path::PathBuf;

#[architect::rpc]
pub trait ResourcePaths {
    /// REAPER resource directory (presets, templates, themes, …).
    fn resource_path(&self) -> PathBuf;

    /// Path to REAPER's `reaper.ini` configuration file.
    fn ini_file_path(&self) -> PathBuf;

    /// Path to the currently loaded color theme file. `None` when the
    /// default theme is active.
    fn color_theme_path(&self) -> Option<PathBuf>;
}
