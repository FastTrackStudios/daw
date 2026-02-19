//! UI service for dialogs and refresh control
//!
//! Provides access to REAPER's UI utilities like file pickers and input dialogs.

use facet::Facet;
use roam::service;
use std::path::PathBuf;

/// Result from a user input dialog
#[derive(Clone, Debug, Facet)]
#[repr(C)]
pub struct UserInputResult {
    /// Whether the user clicked OK (true) or Cancel (false)
    pub ok: bool,
    /// The values entered by the user, corresponding to the prompts
    pub values: Vec<String>,
}

/// Service for UI dialogs and display control
#[service]
pub trait UiService {
    /// Show a dialog prompting the user for multiple text inputs
    ///
    /// # Arguments
    /// * `title` - Dialog window title
    /// * `prompts` - Labels for each input field
    /// * `defaults` - Default values for each field (must match prompts length)
    ///
    /// Returns the user's inputs if they clicked OK, or None if they cancelled.
    async fn get_user_inputs(
        &self,
        title: String,
        prompts: Vec<String>,
        defaults: Vec<String>,
    ) -> Option<UserInputResult>;

    /// Show a file browser dialog to select an existing file
    ///
    /// # Arguments
    /// * `title` - Dialog title
    /// * `initial_dir` - Starting directory (None for current)
    /// * `filter` - File filter pattern (e.g., "*.wav;*.mp3")
    ///
    /// Returns the selected file path, or None if cancelled.
    async fn browse_for_file(
        &self,
        title: String,
        initial_dir: Option<PathBuf>,
        filter: Option<String>,
    ) -> Option<PathBuf>;

    /// Show a file browser dialog to save a file
    ///
    /// # Arguments
    /// * `title` - Dialog title
    /// * `initial_dir` - Starting directory (None for current)
    /// * `default_name` - Default filename
    /// * `filter` - File filter pattern (e.g., "*.wav")
    ///
    /// Returns the selected file path, or None if cancelled.
    async fn browse_for_save_file(
        &self,
        title: String,
        initial_dir: Option<PathBuf>,
        default_name: String,
        filter: Option<String>,
    ) -> Option<PathBuf>;

    /// Show a directory browser dialog
    ///
    /// # Arguments
    /// * `title` - Dialog title
    /// * `initial_dir` - Starting directory (None for current)
    ///
    /// Returns the selected directory path, or None if cancelled.
    async fn browse_for_directory(
        &self,
        title: String,
        initial_dir: Option<PathBuf>,
    ) -> Option<PathBuf>;

    /// Prevent UI refresh temporarily for batch operations
    ///
    /// When true, REAPER won't redraw the UI until set to false.
    /// Useful for performance when making many changes.
    async fn set_prevent_ui_refresh(&self, prevent: bool);
}
