//! Extension State — Persistent Key-Value Storage
//!
//! Inspired by rea-rs's `HasExtState` trait. Provides persistent key-value
//! storage via REAPER's `GetExtState`/`SetExtState`/`DeleteExtState` C API.
//!
//! Values are stored as strings, scoped by a `section` namespace and `key`.
//! The `persist` flag controls whether values survive REAPER restarts
//! (stored in `reaper-extstate.ini`).
//!
//! Typed access via serde is provided client-side in `daw-control`, not here.
//! This keeps the proto layer free of serde dependencies.

use crate::project::ProjectContext;
use vox::service;

/// Service for persistent key-value storage (REAPER's ExtState API).
///
/// Each key is scoped by a `section` string (typically your extension name)
/// and a `key` string. Values are plain strings at the RPC level.
///
/// # Example (via daw-control)
///
/// ```rust,ignore
/// // Store a value
/// daw.ext_state().set("MyExtension", "last_preset", "Clean Guitar", true).await?;
///
/// // Retrieve it
/// let preset = daw.ext_state().get("MyExtension", "last_preset").await?;
/// ```
#[service]
pub trait ExtStateService {
    /// Get a value by section and key. Returns `None` if not set or empty.
    async fn get_ext_state(&self, section: String, key: String) -> Option<String>;

    /// Set a value. If `persist` is true, it survives REAPER restarts.
    async fn set_ext_state(&self, section: String, key: String, value: String, persist: bool);

    /// Delete a value. If `persist` is true, also removes from persistent storage.
    async fn delete_ext_state(&self, section: String, key: String, persist: bool);

    /// Check if a value exists for the given section and key.
    async fn has_ext_state(&self, section: String, key: String) -> bool;

    // === Project-Scoped ExtState ===
    // These methods store data in the project file (.RPP) instead of global storage.

    /// Get a project-scoped value by section and key. Returns `None` if not set or empty.
    /// Project-scoped values are saved with the .RPP file.
    async fn get_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> Option<String>;

    /// Set a project-scoped value. The value is saved with the project file.
    async fn set_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
        value: String,
    );

    /// Delete a project-scoped value.
    async fn delete_project_ext_state(&self, project: ProjectContext, section: String, key: String);

    /// Check if a project-scoped value exists for the given section and key.
    async fn has_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> bool;
}
