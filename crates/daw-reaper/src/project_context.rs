//! Project Context Resolution
//!
//! Utilities for converting between daw-proto's ProjectContext and REAPER's project context.

use daw_proto::ProjectContext;
use reaper_high::{Project, Reaper};
use reaper_medium::{ProjectContext as ReaperProjectContext, ProjectRef, ReaProject};

/// Get the GUID for a project using its raw pointer (unique per tab).
fn project_guid(project: &Project) -> String {
    format!("reaper-ptr-{:x}", project.raw().as_ptr() as usize)
}

/// Find a REAPER project by its GUID
///
/// Iterates through all open project tabs to find the one matching the given GUID.
/// Returns the raw ReaProject pointer that can be used with REAPER APIs.
pub fn find_project_by_guid_raw(guid: &str) -> Option<ReaProject> {
    let reaper = Reaper::get();

    // Iterate through all project tabs (max 128 is reasonable)
    for tab_index in 0..128u32 {
        if let Some(result) = reaper
            .medium_reaper()
            .enum_projects(ProjectRef::Tab(tab_index), 0)
        {
            let project = Project::new(result.project);
            if project_guid(&project) == guid {
                return Some(result.project);
            }
        } else {
            // No more projects
            break;
        }
    }

    None
}

/// Find a REAPER project by its GUID
///
/// Iterates through all open project tabs to find the one matching the given GUID.
/// Returns the high-level Project wrapper.
pub fn find_project_by_guid(guid: &str) -> Option<Project> {
    find_project_by_guid_raw(guid).map(Project::new)
}

/// Convert a daw-proto ProjectContext to a REAPER ProjectContext
///
/// If the project is found by GUID, returns a ProjectContext::Proj with the raw pointer.
/// Otherwise falls back to CurrentProject.
pub fn resolve_project_context(ctx: &ProjectContext) -> ReaperProjectContext {
    match ctx {
        ProjectContext::Current => ReaperProjectContext::CurrentProject,
        ProjectContext::Project(guid) => {
            if let Some(rea_project) = find_project_by_guid_raw(guid) {
                ReaperProjectContext::Proj(rea_project)
            } else {
                // Fallback to current project if not found
                tracing::warn!(
                    "Project with GUID {} not found, using current project",
                    guid
                );
                ReaperProjectContext::CurrentProject
            }
        }
    }
}
