//! Main Thread Bridge
//!
//! REAPER APIs can only be called from the main thread. Service handlers run on
//! tokio worker threads (dispatched by roam). This module provides the bridge
//! between the two worlds.
//!
//! Inspired by helgobox's `MainThreadLayer` pattern, but adapted for roam's
//! architecture. Instead of a Tower middleware that re-schedules the entire
//! handler future, we provide two ergonomic helpers that each service method
//! calls:
//!
//! - [`query`] — for read operations that return a value
//! - [`run`] — for fire-and-forget mutations
//!
//! # Before
//!
//! ```rust,ignore
//! async fn get_tracks(&self, _cx: &Context, project: ProjectContext) -> Vec<Track> {
//!     let Some(ts) = task_support() else {
//!         warn!("TaskSupport not set");
//!         return vec![];
//!     };
//!     ts.main_thread_future(move || {
//!         // ... actual work ...
//!     })
//!     .await
//!     .unwrap_or_default()
//! }
//! ```
//!
//! # After
//!
//! ```rust,ignore
//! async fn get_tracks(&self, _cx: &Context, project: ProjectContext) -> Vec<Track> {
//!     main_thread::query(|| {
//!         // ... actual work ...
//!     })
//!     .await
//!     .unwrap_or_default()
//! }
//! ```

use reaper_high::{Reaper, TaskSupport};
use std::sync::OnceLock;

/// Global TaskSupport instance — set by the extension during initialization.
static TASK_SUPPORT: OnceLock<&'static TaskSupport> = OnceLock::new();

/// Called by the extension during initialization.
pub fn set_task_support(task_support: &'static TaskSupport) {
    let _ = TASK_SUPPORT.set(task_support);
}

/// Get the global TaskSupport reference.
pub(crate) fn task_support() -> Option<&'static TaskSupport> {
    TASK_SUPPORT.get().copied()
}

/// Execute a closure on REAPER's main thread and return the result.
///
/// Use this for query/read operations that need to return a value from REAPER.
/// Returns `None` if TaskSupport is not initialized.
///
/// # Example
///
/// ```rust,ignore
/// let tracks = main_thread::query(|| {
///     proj.tracks().map(|t| build_track_info(&t)).collect()
/// }).await.unwrap_or_default();
/// ```
pub async fn query<F, R>(f: F) -> Option<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let ts = task_support()?;
    ts.main_thread_future(f).await.ok()
}

/// Execute a closure on REAPER's main thread, fire-and-forget.
///
/// Use this for mutation operations that don't need to return a value.
/// Silently does nothing if TaskSupport is not initialized.
///
/// # Example
///
/// ```rust,ignore
/// main_thread::run(move || {
///     track.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
/// });
/// ```
pub fn run<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    if let Some(ts) = task_support() {
        let _ = ts.do_later_in_main_thread_asap(f);
    }
}

// =============================================================================
// Undo Block Scoping
//
// Wraps mutations in REAPER undo blocks so multiple changes appear as a single
// undoable action. Uses reaper-high's `Project::undoable()` which handles
// RAII begin/end and nesting guards.
// =============================================================================

/// Execute a mutation on the main thread wrapped in an undo block.
///
/// The `label` appears in REAPER's Edit > Undo History. The closure receives
/// the resolved project so it can operate on it directly.
///
/// Fire-and-forget — use this for mutations that don't return a value.
///
/// # Example
///
/// ```rust,ignore
/// main_thread::run_undoable(project_ctx, "Rename track", move |proj| {
///     if let Some(t) = resolve_track(&proj, &track_ref) {
///         t.set_name("New Name");
///     }
/// });
/// ```
pub fn run_undoable<F>(project: daw_proto::ProjectContext, label: &'static str, f: F)
where
    F: FnOnce(reaper_high::Project) + Send + 'static,
{
    run(move || {
        let Some(proj) = resolve_project_for_undo(&project) else {
            return;
        };
        proj.undoable(label, || f(proj));
    });
}

/// Execute a query on the main thread wrapped in an undo block, returning a value.
///
/// Use this for mutations that need to return a result (e.g., adding a track
/// returns its GUID). The undo block groups all changes inside the closure.
///
/// # Example
///
/// ```rust,ignore
/// let guid = main_thread::query_undoable(project_ctx, "Add track", move |proj| {
///     let track = proj.insert_track_at(0).ok()?;
///     Some(track.guid().to_string_without_braces())
/// }).await.flatten().unwrap_or_default();
/// ```
pub async fn query_undoable<F, R>(
    project: daw_proto::ProjectContext,
    label: &'static str,
    f: F,
) -> Option<R>
where
    F: FnOnce(reaper_high::Project) -> R + Send + 'static,
    R: Send + 'static,
{
    query(move || {
        let proj = resolve_project_for_undo(&project)?;
        Some(proj.undoable(label, || f(proj)))
    })
    .await
    .flatten()
}

/// Resolve a ProjectContext to a reaper_high::Project (for undo helpers).
fn resolve_project_for_undo(ctx: &daw_proto::ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        daw_proto::ProjectContext::Current => Some(Reaper::get().current_project()),
        daw_proto::ProjectContext::Project(guid) => {
            crate::project_context::find_project_by_guid(guid)
        }
    }
}

// =============================================================================
// Pointer Validation
//
// REAPER uses raw C pointers for tracks, items, etc. These can become dangling
// if the user deletes an object between when we resolve it and when we use it.
// ValidatePtr2 checks if a pointer is still valid within a project.
// =============================================================================

/// Validate that a track pointer is still valid within its project.
///
/// Returns `true` if the track's raw MediaTrack pointer is still recognized
/// by REAPER for the given project. Returns `false` if the track was deleted
/// or if the pointer cannot be obtained.
pub fn is_track_valid(project: &reaper_high::Project, track: &reaper_high::Track) -> bool {
    let Ok(raw) = track.raw() else {
        return false;
    };
    Reaper::get()
        .medium_reaper()
        .validate_ptr_2(reaper_medium::ProjectContext::Proj(project.raw()), raw)
}

/// Validate that a MediaItem pointer is still valid within a project.
pub fn is_item_valid(
    project_ctx: reaper_medium::ProjectContext,
    item: reaper_medium::MediaItem,
) -> bool {
    Reaper::get()
        .medium_reaper()
        .validate_ptr_2(project_ctx, item)
}

/// Validate that a MediaItemTake pointer is still valid within a project.
pub fn is_take_valid(
    project_ctx: reaper_medium::ProjectContext,
    take: reaper_medium::MediaItemTake,
) -> bool {
    Reaper::get()
        .medium_reaper()
        .validate_ptr_2(project_ctx, take)
}
