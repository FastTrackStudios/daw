//! Project service definitions
//!
//! The Project service manages project lifecycle operations including creation,
//! opening, saving, and closing projects. It is responsible for:
//! - Creating new projects
//! - Opening existing projects
//! - Saving project state
//! - Closing projects
//! - Retrieving project metadata
//!
//! All DAWs MUST implement the core Project service behaviors.

use facet::Facet;
use roam::{Tx, service};

/// Context specifying which project to operate on
///
/// Used by all DAW services to target operations at a specific project
/// or the currently active project.
#[repr(u8)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Facet)]
pub enum ProjectContext {
    /// The currently active/focused project (default)
    #[default]
    Current,
    /// A specific project identified by its GUID
    Project(String),
}

impl ProjectContext {
    /// Create a context for the current project
    pub fn current() -> Self {
        Self::Current
    }

    /// Create a context for a specific project by GUID
    pub fn project(guid: impl Into<String>) -> Self {
        Self::Project(guid.into())
    }
}

/// Project metadata
///
/// Globally unique project identifier
///
/// Human-readable project name
///
/// Filesystem path to the project file
#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct ProjectInfo {
    pub guid: String,
    pub name: String,
    pub path: String,
}

/// Events emitted when project state changes
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum ProjectEvent {
    /// A project was opened/added
    Opened(ProjectInfo),
    /// A project was closed (contains the GUID)
    Closed(String),
    /// The active/current project changed (contains new current project GUID, or None)
    CurrentChanged(Option<String>),
    /// A project's metadata was modified
    Changed(ProjectInfo),
    /// Full project list refresh (e.g., after reconnection)
    ProjectsChanged(Vec<ProjectInfo>),
}

/// Service for managing projects
///
/// ### Project Lifecycle
///
/// The service MUST support creating a new project with a specified name.
///
/// The service MUST support opening an existing project from a file path.
///
/// The service MUST support saving the current project state.
///
/// The service MUST support closing the current project.
///
/// If the project has unsaved changes, closing SHOULD prompt for save
/// (implementation-specific).
///
/// ### Project Metadata
///
/// The service MUST provide the current project name.
///
/// The service MUST provide the current project file path.
///
/// The service MUST indicate whether the project has unsaved changes.
///
/// The service MUST provide a list of tracks in the project.
///
/// ### Error Handling
///
/// Opening a non-existent project MUST return an appropriate error.
///
/// Permission errors (read/write) MUST be reported clearly.
///
/// Invalid project format errors MUST be reported clearly.
#[service]
pub trait ProjectService {
    /// Get the currently active/focused project
    async fn get_current(&self) -> Option<ProjectInfo>;

    /// Get a specific project by GUID
    async fn get(&self, project_id: String) -> Option<ProjectInfo>;

    /// List all open projects
    async fn list(&self) -> Vec<ProjectInfo>;

    /// Select/switch to a specific project by GUID
    ///
    /// Makes the specified project the currently active/focused project.
    /// This is equivalent to switching tabs in a DAW that supports multiple
    /// open projects.
    ///
    /// Returns true if the project was successfully selected, false if the
    /// project was not found.
    async fn select(&self, project_id: String) -> bool;

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to project state changes
    ///
    /// Streams events when:
    /// - A project is opened or closed
    /// - The current/active project changes
    /// - Project metadata changes (name, path)
    ///
    /// Initially sends a `ProjectsChanged` event with all current projects,
    /// followed by a `CurrentChanged` event with the current project GUID.
    async fn subscribe(&self, tx: Tx<ProjectEvent>);
}
