//! REAPER Project Implementation
//!
//! Implements ProjectService by dispatching REAPER API calls to the main thread
//! using `crate::main_thread`.

use daw_proto::{ProjectEvent, ProjectInfo, ProjectService};
use reaper_high::{Project, Reaper};
use reaper_medium::{CommandId, ProjectContext, ProjectPart, ProjectRef, UndoScope};
use roam::Tx;
use std::time::Duration;
use tracing::{debug, info};

use crate::main_thread;
use crate::project_context::{find_project_by_guid, project_guid};

// Thread-local storage for the undo block label.
//
// `begin_undo_block` and `end_undo_block` arrive as separate RPC calls, but
// REAPER's `Undo_EndBlock2` needs the label at end-time. We stash the label
// from `begin` and retrieve it in `end` as a fallback.
thread_local! {
    static UNDO_LABEL: std::cell::Cell<Option<String>> = const { std::cell::Cell::new(None) };
}

/// REAPER project implementation that dispatches to the main thread via `main_thread`.
#[derive(Clone)]
pub struct ReaperProject;

impl ReaperProject {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperProject {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve a daw_proto::ProjectContext to a reaper_high::Project
fn resolve_project(ctx: &daw_proto::ProjectContext) -> Option<Project> {
    match ctx {
        daw_proto::ProjectContext::Current => Some(Reaper::get().current_project()),
        daw_proto::ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

/// Get a project by tab index using medium_reaper's enum_projects
fn project_by_tab(reaper: &Reaper, tab_index: u32) -> Option<Project> {
    reaper
        .medium_reaper()
        .enum_projects(ProjectRef::Tab(tab_index), 0)
        .map(|result| Project::new(result.project))
}

/// Extract project info from a REAPER project
fn project_to_info(project: &Project) -> ProjectInfo {
    let path = project.file().map(|p| p.to_string()).unwrap_or_default();
    let name = if path.is_empty() {
        "Untitled".to_string()
    } else {
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    };

    let guid = project_guid(project);

    ProjectInfo { guid, name, path }
}

/// Convert daw_proto::UndoScope to reaper_medium::UndoScope
fn convert_undo_scope(scope: &daw_proto::UndoScope) -> UndoScope {
    use enumflags2::BitFlags;

    match scope {
        daw_proto::UndoScope::All => UndoScope::All,
        daw_proto::UndoScope::Scoped(parts) => {
            let mut flags = BitFlags::empty();
            for part in parts {
                let reaper_part = match part {
                    daw_proto::ProjectPart::Freeze => ProjectPart::Freeze,
                    daw_proto::ProjectPart::Fx => ProjectPart::Fx,
                    daw_proto::ProjectPart::Items => ProjectPart::Items,
                    daw_proto::ProjectPart::MiscCfg => ProjectPart::MiscCfg,
                    daw_proto::ProjectPart::TrackCfg => ProjectPart::TrackCfg,
                };
                flags |= reaper_part;
            }
            UndoScope::Scoped(flags)
        }
    }
}

impl ProjectService for ReaperProject {
    async fn get_current(&self) -> Option<ProjectInfo> {
        debug!("ReaperProject: get_current");

        main_thread::query(|| {
            let reaper = Reaper::get();
            let project = reaper.current_project();
            Some(project_to_info(&project))
        })
        .await
        .flatten()
    }

    async fn get(&self, project_id: String) -> Option<ProjectInfo> {
        debug!("ReaperProject: get({})", project_id);

        main_thread::query(move || {
            let reaper = Reaper::get();

            // Iterate through all open project tabs
            for i in 0..128 {
                if let Some(project) = project_by_tab(reaper, i) {
                    let info = project_to_info(&project);
                    if info.guid == project_id {
                        return Some(info);
                    }
                } else {
                    // No more tabs
                    break;
                }
            }
            None
        })
        .await
        .flatten()
    }

    async fn list(&self) -> Vec<ProjectInfo> {
        debug!("ReaperProject: list");

        main_thread::query(|| {
            let reaper = Reaper::get();
            let mut projects = Vec::new();

            // Iterate through all open project tabs (max 128)
            for i in 0..128 {
                if let Some(project) = project_by_tab(reaper, i) {
                    let info = project_to_info(&project);
                    // Skip routing/utility projects
                    if !info.name.to_uppercase().contains("FTS-ROUTING") {
                        projects.push(info);
                    }
                } else {
                    // No more tabs
                    break;
                }
            }

            info!("ReaperProject: list - found {} projects", projects.len());
            projects
        })
        .await
        .unwrap_or_default()
    }

    async fn select(&self, project_id: String) -> bool {
        info!("ReaperProject: select({})", project_id);

        main_thread::query(move || {
            let reaper = Reaper::get();

            // Find the tab index for the project with matching GUID
            let mut target_tab: Option<u32> = None;
            for i in 0..128u32 {
                if let Some(project) = project_by_tab(reaper, i) {
                    let info = project_to_info(&project);
                    if info.guid == project_id {
                        target_tab = Some(i);
                        break;
                    }
                } else {
                    break;
                }
            }

            let Some(target) = target_tab else {
                info!("ReaperProject: select - project {} not found", project_id);
                return false;
            };

            // Get current tab index
            let current_project = reaper.current_project();
            let mut current_tab: Option<u32> = None;
            for i in 0..128u32 {
                if let Some(project) = project_by_tab(reaper, i) {
                    if project == current_project {
                        current_tab = Some(i);
                        break;
                    }
                } else {
                    break;
                }
            }

            let Some(current) = current_tab else {
                return false;
            };

            if current == target {
                // Already on the correct tab
                return true;
            }

            // Calculate shortest path (forward or backward)
            // Count total tabs first
            let mut total_tabs = 0u32;
            for i in 0..128u32 {
                if project_by_tab(reaper, i).is_some() {
                    total_tabs = i + 1;
                } else {
                    break;
                }
            }

            let forward_distance = if target > current {
                target - current
            } else {
                total_tabs - current + target
            };

            let backward_distance = if current > target {
                current - target
            } else {
                current + total_tabs - target
            };

            // REAPER actions for tab switching
            let action_next_tab = CommandId::new(40861);
            let action_prev_tab = CommandId::new(40862);

            if forward_distance <= backward_distance {
                // Go forward
                for _ in 0..forward_distance {
                    reaper.medium_reaper().main_on_command_ex(
                        action_next_tab,
                        0,
                        ProjectContext::CurrentProject,
                    );
                }
            } else {
                // Go backward
                for _ in 0..backward_distance {
                    reaper.medium_reaper().main_on_command_ex(
                        action_prev_tab,
                        0,
                        ProjectContext::CurrentProject,
                    );
                }
            }

            // Verify we ended up at the right project
            let final_project = reaper.current_project();
            let final_info = project_to_info(&final_project);
            let success = final_info.guid == project_id;

            if success {
                info!(
                    "ReaperProject: select - successfully switched to {}",
                    final_info.name
                );
            } else {
                tracing::warn!(
                    "ReaperProject: select - ended at {} instead of expected project",
                    final_info.name
                );
            }

            success
        })
        .await
        .unwrap_or(false)
    }

    async fn open(&self, path: String) -> Option<ProjectInfo> {
        info!("ReaperProject: open({})", path);

        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Snapshot existing tab pointers before opening
            let mut existing_ptrs = std::collections::HashSet::new();
            for tab in 0..128u32 {
                match project_by_tab(reaper, tab) {
                    Some(p) => {
                        existing_ptrs.insert(p.raw().as_ptr() as usize);
                    }
                    None => break,
                }
            }

            // Create a new tab first (action 41929 = "New project tab, ignore default template")
            let action_new_tab = CommandId::new(41929);
            medium.main_on_command_ex(action_new_tab, 0, ProjectContext::CurrentProject);

            // Open the project file into the new tab (noprompt to skip save dialog)
            let file_path = camino::Utf8Path::new(&path);
            let mut behavior = reaper_medium::OpenProjectBehavior::default();
            behavior.open_as_template = false;
            behavior.prompt = false;
            medium.main_open_project(file_path, behavior);

            // Find the new tab by scanning for a pointer not in our snapshot
            for tab in 0..128u32 {
                if let Some(p) = project_by_tab(reaper, tab) {
                    let ptr = p.raw().as_ptr() as usize;
                    if !existing_ptrs.contains(&ptr) {
                        debug!("Opened project in tab {} (ptr={:x}): {}", tab, ptr, path);
                        return Some(project_to_info(&p));
                    }
                }
            }

            // Fallback: the project may have loaded into the current tab
            let current = project_to_info(&reaper.current_project());
            debug!("Opened project (current tab fallback): {}", current.name);
            Some(current)
        })
        .await
        .flatten()
    }

    async fn create(&self) -> Option<ProjectInfo> {
        info!("ReaperProject: create");

        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Snapshot existing tab pointers before creating
            let mut existing_ptrs = std::collections::HashSet::new();
            for tab in 0..128u32 {
                match project_by_tab(reaper, tab) {
                    Some(p) => {
                        existing_ptrs.insert(p.raw().as_ptr() as usize);
                    }
                    None => break,
                }
            }
            let old_count = existing_ptrs.len() as u32;

            // Fire REAPER action 41929 = "New project tab (ignore default template)"
            let action_new_tab = CommandId::new(41929);
            medium.main_on_command_ex(action_new_tab, 0, ProjectContext::CurrentProject);

            // Find the new tab by scanning for a pointer not in our snapshot
            for tab in 0..128u32 {
                if let Some(p) = project_by_tab(reaper, tab) {
                    let ptr = p.raw().as_ptr() as usize;
                    if !existing_ptrs.contains(&ptr) {
                        debug!("New project tab at index {} (ptr={:x})", tab, ptr);
                        return Some(project_to_info(&p));
                    }
                }
            }

            // Fallback: new tab appears at old_count
            if let Some(p) = project_by_tab(reaper, old_count) {
                debug!("New project tab via fallback at index {}", old_count);
                return Some(project_to_info(&p));
            }

            tracing::warn!("create: could not find new tab, returning current project");
            Some(project_to_info(&reaper.current_project()))
        })
        .await
        .flatten()
    }

    async fn close(&self, project_id: String) -> bool {
        info!("ReaperProject: close({})", project_id);

        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Check if target is already the current project
            let current_info = project_to_info(&reaper.current_project());
            if current_info.guid != project_id {
                // Navigate to the target tab
                let action_next = CommandId::new(40861);
                let mut found = false;
                for _ in 0..128 {
                    medium.main_on_command_ex(action_next, 0, ProjectContext::CurrentProject);
                    let now = project_to_info(&reaper.current_project());
                    if now.guid == project_id {
                        found = true;
                        break;
                    }
                }
                if !found {
                    info!("ReaperProject: close - project {} not found", project_id);
                    return false;
                }
            }

            // Close the current tab: action 40860
            // Note: undomaxmem=0 in reaper.ini prevents the "Save changes?"
            // dialog. REAPER must be launched with -cfgfile pointing to the
            // rig-specific ini for this to work.
            let action_close_tab = CommandId::new(40860);
            medium.main_on_command_ex(action_close_tab, 0, ProjectContext::CurrentProject);

            true
        })
        .await
        .unwrap_or(false)
    }

    async fn get_by_slot(&self, slot: u32) -> Option<ProjectInfo> {
        debug!("ReaperProject: get_by_slot({})", slot);

        main_thread::query(move || {
            let reaper = Reaper::get();
            project_by_tab(reaper, slot).map(|p| project_to_info(&p))
        })
        .await
        .flatten()
    }

    // =========================================================================
    // Undo
    // =========================================================================

    async fn begin_undo_block(&self, project: daw_proto::ProjectContext, label: String) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            Reaper::get()
                .medium_reaper()
                .undo_begin_block_2(reaper_medium::ProjectContext::Proj(proj.raw()));
            // Stash label for end_undo_block fallback
            UNDO_LABEL.with(|cell| cell.replace(Some(label)));
        });
    }

    async fn end_undo_block(
        &self,
        project: daw_proto::ProjectContext,
        label: String,
        scope: Option<daw_proto::UndoScope>,
    ) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            // Use the provided label, falling back to whatever was stashed at begin
            let final_label = if !label.is_empty() {
                label
            } else {
                UNDO_LABEL
                    .with(|cell| cell.take())
                    .unwrap_or_else(|| "FTS action".to_string())
            };

            // Convert daw_proto::UndoScope to reaper_medium::UndoScope
            let reaper_scope = scope
                .as_ref()
                .map(convert_undo_scope)
                .unwrap_or(UndoScope::All);

            Reaper::get().medium_reaper().undo_end_block_2(
                reaper_medium::ProjectContext::Proj(proj.raw()),
                final_label.as_str(),
                reaper_scope,
            );
        });
    }

    async fn undo(&self, project: daw_proto::ProjectContext) -> bool {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            Some(proj.undo())
        })
        .await
        .flatten()
        .unwrap_or(false)
    }

    async fn redo(&self, project: daw_proto::ProjectContext) -> bool {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            Some(proj.redo())
        })
        .await
        .flatten()
        .unwrap_or(false)
    }

    async fn last_undo_label(&self, project: daw_proto::ProjectContext) -> Option<String> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            proj.label_of_last_undoable_action()
                .map(|s| s.to_str().to_string())
        })
        .await
        .flatten()
    }

    async fn last_redo_label(&self, project: daw_proto::ProjectContext) -> Option<String> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            proj.label_of_last_redoable_action()
                .map(|s| s.to_str().to_string())
        })
        .await
        .flatten()
    }

    // =========================================================================
    // Actions / Commands
    // =========================================================================

    async fn run_command(&self, project: daw_proto::ProjectContext, command: String) -> bool {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Resolve command string to CommandId (numeric or named)
            let cmd_id = if let Ok(numeric_id) = command.parse::<u32>() {
                CommandId::new(numeric_id)
            } else if let Some(named_id) = medium.named_command_lookup(command.as_str()) {
                named_id
            } else {
                // Try with underscore prefix (SWS convention)
                let prefixed = format!("_{command}");
                if let Some(named_id) = medium.named_command_lookup(prefixed.as_str()) {
                    named_id
                } else {
                    debug!("run_command: command not found: {}", command);
                    return false;
                }
            };

            // REAPER actions always operate on the "current" project tab,
            // so we must switch to the correct tab before executing
            let proj_ctx = match resolve_project(&project) {
                Some(p) => {
                    unsafe {
                        medium.low().SelectProjectInstance(p.raw().as_ptr());
                    }
                    ProjectContext::Proj(p.raw())
                }
                None => ProjectContext::CurrentProject,
            };

            medium.main_on_command_ex(cmd_id, 0, proj_ctx);
            debug!("run_command: executed '{}'", command);
            true
        })
        .await
        .unwrap_or(false)
    }

    // =========================================================================
    // Save
    // =========================================================================

    async fn save(&self, project: daw_proto::ProjectContext) {
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let proj_ctx = match resolve_project(&project) {
                Some(p) => ProjectContext::Proj(p.raw()),
                None => ProjectContext::CurrentProject,
            };

            // Action 40026 = "File: Save project"
            medium.main_on_command_ex(CommandId::new(40026), 0, proj_ctx);
        });
    }

    async fn save_all(&self) {
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Action 40897 = "File: Save all projects"
            medium.main_on_command_ex(CommandId::new(40897), 0, ProjectContext::CurrentProject);
        });
    }

    // =========================================================================
    // Ruler Lane Management (v7.62+)
    // =========================================================================

    async fn set_ruler_lane_name(
        &self,
        project: daw_proto::ProjectContext,
        lane_index: u32,
        name: String,
    ) {
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();
            let proj_ctx = match resolve_project(&project) {
                Some(p) => ProjectContext::Proj(p.raw()),
                None => ProjectContext::CurrentProject,
            };
            let key = std::ffi::CString::new(format!("RULER_LANE_NAME:{}", lane_index)).unwrap();
            let value = std::ffi::CString::new(name).unwrap();
            unsafe {
                low.GetSetProjectInfo_String(
                    proj_ctx.to_raw(),
                    key.as_ptr(),
                    value.as_ptr() as *mut _,
                    true,
                );
            }
        });
    }

    async fn get_ruler_lane_name(
        &self,
        project: daw_proto::ProjectContext,
        lane_index: u32,
    ) -> String {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();
            let proj_ctx = match resolve_project(&project) {
                Some(p) => ProjectContext::Proj(p.raw()),
                None => ProjectContext::CurrentProject,
            };
            let key = std::ffi::CString::new(format!("RULER_LANE_NAME:{}", lane_index)).unwrap();
            let mut buf = [0u8; 256];
            let buf_ptr = buf.as_mut_ptr() as *mut std::ffi::c_char;
            unsafe {
                low.GetSetProjectInfo_String(proj_ctx.to_raw(), key.as_ptr(), buf_ptr, false);
                std::ffi::CStr::from_ptr(buf_ptr)
                    .to_string_lossy()
                    .to_string()
            }
        })
        .await
        .unwrap_or_default()
    }

    async fn ruler_lane_count(&self, project: daw_proto::ProjectContext) -> u32 {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();
            let proj_ctx = match resolve_project(&project) {
                Some(p) => ProjectContext::Proj(p.raw()),
                None => ProjectContext::CurrentProject,
            };
            // Count by probing lane names until we find an empty one
            let mut count = 0u32;
            for i in 1..=128 {
                let key = std::ffi::CString::new(format!("RULER_LANE_NAME:{}", i)).unwrap();
                let mut buf = [0u8; 256];
                let buf_ptr = buf.as_mut_ptr() as *mut std::ffi::c_char;
                unsafe {
                    low.GetSetProjectInfo_String(proj_ctx.to_raw(), key.as_ptr(), buf_ptr, false);
                    let name = std::ffi::CStr::from_ptr(buf_ptr).to_string_lossy();
                    if name.is_empty() {
                        break;
                    }
                    count = i;
                }
            }
            count
        })
        .await
        .unwrap_or(0)
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    async fn subscribe(&self, tx: Tx<ProjectEvent>) {
        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        let this = self.clone();
        tokio::spawn(async move {
            this.subscribe_impl(tx).await;
        });
    }
}

impl ReaperProject {
    /// Helper to get list of projects (used by subscribe)
    async fn get_project_list(&self) -> Vec<ProjectInfo> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let mut projects = Vec::new();

            for i in 0..128 {
                if let Some(project) = project_by_tab(reaper, i) {
                    let info = project_to_info(&project);
                    if !info.name.to_uppercase().contains("FTS-ROUTING") {
                        projects.push(info);
                    }
                } else {
                    break;
                }
            }
            projects
        })
        .await
        .unwrap_or_default()
    }

    /// Helper to get current project GUID
    async fn get_current_guid(&self) -> Option<String> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let project = reaper.current_project();
            Some(project_to_info(&project).guid)
        })
        .await
        .flatten()
    }

    async fn subscribe_impl(&self, tx: Tx<ProjectEvent>) {
        info!("ReaperProject::subscribe() - starting project stream");

        // Send initial state: all projects
        let projects = self.get_project_list().await;
        if tx
            .send(ProjectEvent::ProjectsChanged(projects.clone()))
            .await
            .is_err()
        {
            debug!("ReaperProject::subscribe() - client disconnected during initial send");
            return;
        }

        // Send current project
        let current_guid = self.get_current_guid().await;
        if tx
            .send(ProjectEvent::CurrentChanged(current_guid.clone()))
            .await
            .is_err()
        {
            debug!("ReaperProject::subscribe() - client disconnected");
            return;
        }

        // Poll for changes at 60Hz
        let mut last_guid = current_guid;
        let mut last_projects = projects;

        loop {
            tokio::time::sleep(Duration::from_micros(16667)).await;

            // Check for project list changes
            let current_projects = self.get_project_list().await;
            if current_projects != last_projects {
                if tx
                    .send(ProjectEvent::ProjectsChanged(current_projects.clone()))
                    .await
                    .is_err()
                {
                    debug!("ReaperProject::subscribe() - client disconnected");
                    break;
                }
                last_projects = current_projects;
            }

            // Check for current project change
            let current_guid = self.get_current_guid().await;
            if current_guid != last_guid {
                if tx
                    .send(ProjectEvent::CurrentChanged(current_guid.clone()))
                    .await
                    .is_err()
                {
                    debug!("ReaperProject::subscribe() - client disconnected");
                    break;
                }
                last_guid = current_guid;
            }
        }

        info!("ReaperProject::subscribe() - stream ended");
    }
}
