//! Resource path service
//!
//! Provides access to REAPER resource directories and configuration files.

use std::path::PathBuf;
use vox::service;

/// Service for accessing REAPER resource paths and configuration locations
#[service]
pub trait ResourceService {
    /// Get the REAPER resource directory path
    ///
    /// This is the main REAPER resource directory containing presets, templates, etc.
    async fn get_resource_path(&self) -> PathBuf;

    /// Get the path to the REAPER INI configuration file
    async fn get_ini_file_path(&self) -> PathBuf;

    /// Get the path to the currently loaded color theme file
    ///
    /// Returns None if no custom theme is loaded (using default theme)
    async fn get_color_theme_path(&self) -> Option<PathBuf>;
}
