//! Project service definitions
//!
//! The Project service manages project lifecycle operations including creation,
//! opening, saving, and closing projects. It is responsible for:
//! - Creating new projects
//! - Opening existing projects
//! - Saving project state
//! - Closing projects
//! - Retrieving project metadata
//! //!
//! All DAWs MUST implement the core Project service behaviors.

use facet::Facet;
use roam::service;

/// Project metadata
///
/// Globally unique project identifier
///
/// Human-readable project name
///
/// Filesystem path to the project file
#[derive(Clone, Debug, Facet)]
pub struct ProjectInfo {
    pub guid: String,
    pub name: String,
    pub path: String,
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
}
