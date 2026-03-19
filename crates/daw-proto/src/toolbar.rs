//! Toolbar Service — Dynamic toolbar management for DAW extensions.
//!
//! Extensions can add, update, and remove toolbar buttons in the host DAW.
//! Operations are deferred and applied on the main thread to avoid re-entrancy
//! issues inside DAW callbacks.

use facet::Facet;
use roam::service;

/// Target toolbar for button placement.
#[repr(u8)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub enum ToolbarTarget {
    /// Main toolbar.
    #[default]
    Main,
    /// Floating toolbar (1–32).
    Floating(u8),
}

/// A toolbar button to add or update.
#[derive(Debug, Clone, Facet)]
pub struct ToolbarButton {
    /// REAPER command name (e.g., `_FTS_SIGNAL_OPEN_BROWSER`).
    pub command_name: String,
    /// Display label shown on the button.
    pub label: String,
    /// Optional icon path or name.
    pub icon: Option<String>,
    /// Which toolbar to place the button on.
    pub target: ToolbarTarget,
    /// Toolbar button flags (bitmask).
    pub flags: u32,
}

/// Result of a toolbar operation.
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum ToolbarResult {
    /// Button was added/updated successfully with this command ID.
    Ok(u32),
    /// Operation failed.
    Error(String),
}

/// Service for managing toolbar buttons in the host DAW.
///
/// Operations are queued and applied from the host's timer callback
/// to avoid re-entrancy issues.
#[service]
pub trait ToolbarService {
    /// Add a toolbar button. Returns the resolved command ID.
    ///
    /// If the button already exists, this is a no-op (returns existing ID).
    /// The `workflow_id` groups buttons for batch removal.
    async fn add_button(&self, button: ToolbarButton, workflow_id: String) -> ToolbarResult;

    /// Update an existing toolbar button (or add if not present).
    async fn update_button(&self, button: ToolbarButton, workflow_id: String) -> ToolbarResult;

    /// Remove a single toolbar button by command name and target.
    async fn remove_button(&self, target: ToolbarTarget, command_name: String) -> ToolbarResult;

    /// Remove all toolbar buttons belonging to a workflow.
    async fn remove_workflow_buttons(&self, workflow_id: String) -> ToolbarResult;

    /// Check if the dynamic toolbar API is available in the host.
    async fn is_available(&self) -> bool;

    /// List all tracked buttons: (toolbar_name, command_name, workflow_id).
    async fn get_tracked_buttons(&self) -> Vec<TrackedButton>;
}

/// A tracked toolbar button entry.
#[derive(Debug, Clone, Facet)]
pub struct TrackedButton {
    /// Toolbar name (e.g., "Main toolbar", "Floating toolbar 1").
    pub toolbar_name: String,
    /// REAPER command name.
    pub command_name: String,
    /// Workflow that owns this button.
    pub workflow_id: String,
}
