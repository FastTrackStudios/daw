//! REAPER ExtState Implementation
//!
//! Implements ExtStateService by dispatching REAPER's GetExtState/SetExtState/
//! DeleteExtState/HasExtState C API calls to the main thread via the low-level
//! reaper-rs bindings (the pinned reaper-rs version doesn't expose ext state
//! in the medium/high layers).

use std::ffi::{CStr, CString};

use crate::{main_thread, project_context::resolve_project_context};
use daw_proto::{ExtStateService, ProjectContext};
use reaper_high::Reaper;
use roam::Context;
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
    async fn get_ext_state(&self, _cx: &Context, section: String, key: String) -> Option<String> {
        debug!("ReaperExtState::get({}, {})", section, key);

        main_thread::query(move || {
            let section_c = CString::new(section).ok()?;
            let key_c = CString::new(key).ok()?;
            let low = Reaper::get().medium_reaper().low();

            // Safety: CString args are valid NUL-terminated pointers, and we're
            // on the main thread. GetExtState returns a pointer to REAPER-owned
            // memory that remains valid until the next SetExtState/DeleteExtState
            // call for this section+key — we immediately copy it into a String.
            let ptr = unsafe { low.GetExtState(section_c.as_ptr(), key_c.as_ptr()) };

            if ptr.is_null() {
                return None;
            }

            let value = unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned();
            if value.is_empty() { None } else { Some(value) }
        })
        .await
        .flatten()
    }

    async fn set_ext_state(
        &self,
        _cx: &Context,
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

            // Safety: all CString args are valid NUL-terminated pointers, main thread.
            unsafe {
                low.SetExtState(
                    section_c.as_ptr(),
                    key_c.as_ptr(),
                    value_c.as_ptr(),
                    persist,
                );
            }
        });
    }

    async fn delete_ext_state(&self, _cx: &Context, section: String, key: String, persist: bool) {
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

            // Safety: CString args are valid NUL-terminated pointers, main thread.
            unsafe {
                low.DeleteExtState(section_c.as_ptr(), key_c.as_ptr(), persist);
            }
        });
    }

    async fn has_ext_state(&self, _cx: &Context, section: String, key: String) -> bool {
        debug!("ReaperExtState::has({}, {})", section, key);

        main_thread::query(move || {
            let section_c = CString::new(section).ok()?;
            let key_c = CString::new(key).ok()?;
            let low = Reaper::get().medium_reaper().low();

            // Safety: CString args are valid NUL-terminated pointers, main thread.
            Some(unsafe { low.HasExtState(section_c.as_ptr(), key_c.as_ptr()) })
        })
        .await
        .flatten()
        .unwrap_or(false)
    }

    // === Project-Scoped ExtState ===

    async fn get_project_ext_state(
        &self,
        _cx: &Context,
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

            let mut buf = vec![0u8; 4096]; // Buffer for the value
            let len_read = unsafe {
                low.GetProjExtState(
                    proj_ctx.to_raw(),
                    section_c.as_ptr(),
                    key_c.as_ptr(),
                    buf.as_mut_ptr() as *mut i8,
                    buf.len() as i32,
                )
            };

            if len_read <= 0 {
                return None;
            }

            // Find actual length (null-terminated)
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let value = String::from_utf8_lossy(&buf[..len]).into_owned();
            if value.is_empty() { None } else { Some(value) }
        })
        .await
        .flatten()
    }

    async fn set_project_ext_state(
        &self,
        _cx: &Context,
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

            unsafe {
                low.SetProjExtState(
                    proj_ctx.to_raw(),
                    section_c.as_ptr(),
                    key_c.as_ptr(),
                    value_c.as_ptr(),
                );
            }
        });
    }

    async fn delete_project_ext_state(
        &self,
        _cx: &Context,
        project: ProjectContext,
        section: String,
        key: String,
    ) {
        debug!(
            "ReaperExtState::delete_project_ext_state({}, {})",
            section, key
        );

        // Set to empty string to delete (REAPER API convention)
        self.set_project_ext_state(_cx, project, section, key, String::new())
            .await;
    }

    async fn has_project_ext_state(
        &self,
        cx: &Context,
        project: ProjectContext,
        section: String,
        key: String,
    ) -> bool {
        debug!(
            "ReaperExtState::has_project_ext_state({}, {})",
            section, key
        );

        self.get_project_ext_state(cx, project, section, key)
            .await
            .is_some()
    }
}
