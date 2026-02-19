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

use reaper_high::TaskSupport;
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
