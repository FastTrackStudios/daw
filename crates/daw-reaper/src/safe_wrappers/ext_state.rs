//! Safe wrappers for REAPER ExtState APIs.

use super::ReaperLow;
use std::ffi::CString;

/// Get a global ext state value.
pub fn get_ext_state(low: &ReaperLow, section: &CString, key: &CString) -> Option<String> {
    let ptr = unsafe { low.GetExtState(section.as_ptr(), key.as_ptr()) };
    super::cstring::read_cstr(ptr)
}

/// Set a global ext state value.
pub fn set_ext_state(low: &ReaperLow, section: &CString, key: &CString, value: &CString, persist: bool) {
    unsafe {
        low.SetExtState(section.as_ptr(), key.as_ptr(), value.as_ptr(), persist);
    }
}

/// Delete a global ext state value.
pub fn delete_ext_state(low: &ReaperLow, section: &CString, key: &CString, persist: bool) {
    unsafe {
        low.DeleteExtState(section.as_ptr(), key.as_ptr(), persist);
    }
}

/// Check if a global ext state value exists.
pub fn has_ext_state(low: &ReaperLow, section: &CString, key: &CString) -> bool {
    unsafe { low.HasExtState(section.as_ptr(), key.as_ptr()) }
}

/// Get a project-scoped ext state value.
pub fn get_proj_ext_state(
    low: &ReaperLow,
    project: *mut reaper_low::raw::ReaProject,
    section: &CString,
    key: &CString,
    buf_size: usize,
) -> Option<String> {
    super::buffer::with_string_buffer_i32(buf_size, |buf, len| unsafe {
        low.GetProjExtState(project, section.as_ptr(), key.as_ptr(), buf, len)
    })
}

/// Set a project-scoped ext state value.
pub fn set_proj_ext_state(
    low: &ReaperLow,
    project: *mut reaper_low::raw::ReaProject,
    section: &CString,
    key: &CString,
    value: &CString,
) {
    unsafe {
        low.SetProjExtState(project, section.as_ptr(), key.as_ptr(), value.as_ptr());
    }
}
