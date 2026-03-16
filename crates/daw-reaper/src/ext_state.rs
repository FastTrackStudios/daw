//! REAPER ExtState Implementation
//!
//! Implements ExtStateService by dispatching REAPER's GetExtState/SetExtState/
//! DeleteExtState/HasExtState C API calls to the main thread via the low-level
//! reaper-rs bindings (the pinned reaper-rs version doesn't expose ext state
//! in the medium/high layers).

use std::ffi::CString;

use crate::safe_wrappers::ext_state as sw;
use crate::{main_thread, project_context::resolve_project_context};
use daw_proto::{ExtStateService, ProjectContext};
use reaper_high::Reaper;
use tracing::debug;

/// REAPER ext state implementation.
///
/// Zero-field struct — all state lives in REAPER itself (in memory or in
/// `reaper-extstate.ini` for persistent values).
#[derive(Clone)]
pub struct ReaperExtState;

impl ReaperExtState {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperExtState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtStateService for ReaperExtState {
    async fn get_ext_state(&self, section: String, key: String) -> Option<String> {
        debug!("ReaperExtState::get({}, {})", section, key);

        main_thread::query(move || {
            let section_c = CString::new(section).ok()?;
            let key_c = CString::new(key).ok()?;
            let low = Reaper::get().medium_reaper().low();
            sw::get_ext_state(low, &section_c, &key_c)
        })
        .await
        .flatten()
    }

    async fn set_ext_state(
        &self,
        section: String,
        key: String,
        value: String,
        persist: bool,
    ) {
        debug!(
            "ReaperExtState::set({}, {}, persist={})",
            section, key, persist
        );

        main_thread::run(move || {
            let Ok(section_c) = CString::new(section) else {
                return;
            };
            let Ok(key_c) = CString::new(key) else {
                return;
            };
            let Ok(value_c) = CString::new(value) else {
                return;
            };
            let low = Reaper::get().medium_reaper().low();
            sw::set_ext_state(low, &section_c, &key_c, &value_c, persist);
        });
    }

    async fn delete_ext_state(&self, section: String, key: String, persist: bool) {
        debug!(
            "ReaperExtState::delete({}, {}, persist={})",
            section, key, persist
        );

        main_thread::run(move || {
            let Ok(section_c) = CString::new(section) else {
                return;
            };
            let Ok(key_c) = CString::new(key) else {
                return;
            };
            let low = Reaper::get().medium_reaper().low();
            sw::delete_ext_state(low, &section_c, &key_c, persist);
        });
    }

    async fn has_ext_state(&self, section: String, key: String) -> bool {
        debug!("ReaperExtState::has({}, {})", section, key);

        main_thread::query(move || {
            let section_c = CString::new(section).ok()?;
            let key_c = CString::new(key).ok()?;
            let low = Reaper::get().medium_reaper().low();
            Some(sw::has_ext_state(low, &section_c, &key_c))
        })
        .await
        .flatten()
        .unwrap_or(false)
    }

    // === Project-Scoped ExtState ===

    async fn get_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> Option<String> {
        debug!(
            "ReaperExtState::get_project_ext_state({}, {})",
            section, key
        );

        main_thread::query(move || {
            let section_c = CString::new(section).ok()?;
            let key_c = CString::new(key).ok()?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = resolve_project_context(&project);
            sw::get_proj_ext_state(low, proj_ctx, &section_c, &key_c, 4096)
        })
        .await
        .flatten()
    }

    async fn set_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
        value: String,
    ) {
        debug!(
            "ReaperExtState::set_project_ext_state({}, {})",
            section, key
        );

        main_thread::run(move || {
            let Ok(section_c) = CString::new(section) else {
                return;
            };
            let Ok(key_c) = CString::new(key) else {
                return;
            };
            let Ok(value_c) = CString::new(value) else {
                return;
            };
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = resolve_project_context(&project);
            sw::set_proj_ext_state(low, proj_ctx, &section_c, &key_c, &value_c);
        });
    }

    async fn delete_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) {
        debug!(
            "ReaperExtState::delete_project_ext_state({}, {})",
            section, key
        );

        // Set to empty string to delete (REAPER API convention)
        self.set_project_ext_state(project, section, key, String::new())
            .await;
    }

    async fn has_project_ext_state(
        &self,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> bool {
        debug!(
            "ReaperExtState::has_project_ext_state({}, {})",
            section, key
        );

        self.get_project_ext_state(project, section, key)
            .await
            .is_some()
    }
}
