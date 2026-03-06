//! Safe wrappers for REAPER marker and region APIs.

use super::ReaperLow;
use std::os::raw::c_char;

/// Delete a project marker or region.
pub fn delete_project_marker(
    low: &ReaperLow,
    project: *mut reaper_low::raw::ReaProject,
    id: i32,
    is_region: bool,
) {
    unsafe {
        low.DeleteProjectMarker(project, id, is_region);
    }
}

/// Set a project marker/region position and name.
///
/// Pass `name` as `std::ptr::null()` to keep the existing name, or `-1.0` for
/// position/end to keep the existing values.
pub fn set_project_marker(
    low: &ReaperLow,
    id: i32,
    is_region: bool,
    pos: f64,
    end: f64,
    name: *const c_char,
) {
    unsafe {
        low.SetProjectMarker(id, is_region, pos, end, name);
    }
}

/// Set a project marker/region by index with color.
pub fn set_project_marker_by_index2(
    low: &ReaperLow,
    project: *mut reaper_low::raw::ReaProject,
    id: i32,
    is_region: bool,
    pos: f64,
    end: f64,
    name_idx: i32,
    name: *const c_char,
    color: i32,
    flags: i32,
) {
    unsafe {
        low.SetProjectMarkerByIndex2(project, id, is_region, pos, end, name_idx, name, color, flags);
    }
}

/// Enumerate project markers. Returns 0 when no more markers exist.
pub fn enum_project_markers(
    low: &ReaperLow,
    idx: i32,
) -> Option<EnumMarkerResult> {
    let mut is_region = false;
    let mut pos = 0.0;
    let mut end = 0.0;
    let mut name_ptr: *const c_char = std::ptr::null();
    let mut marker_idx = 0;
    let result = unsafe {
        low.EnumProjectMarkers(idx, &mut is_region, &mut pos, &mut end, &mut name_ptr, &mut marker_idx)
    };
    if result == 0 {
        return None;
    }
    Some(EnumMarkerResult {
        is_region,
        pos,
        end,
        name: super::cstring::read_cstr_or_empty(name_ptr),
        marker_idx,
    })
}

/// Result from enumerating project markers.
pub struct EnumMarkerResult {
    pub is_region: bool,
    pub pos: f64,
    pub end: f64,
    pub name: String,
    pub marker_idx: i32,
}
