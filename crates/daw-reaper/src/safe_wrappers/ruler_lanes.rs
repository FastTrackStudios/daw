//! Safe wrappers for REAPER ruler lane APIs (v7.62+).
//!
//! These APIs are only available in REAPER 7.62 and later. Functions check
//! for availability at runtime and return `None`/`false` when unavailable.

use super::ReaperLow;
use reaper_medium::ProjectContext;
use std::ptr;

/// Returns `true` if the running REAPER version supports ruler lane APIs
/// (i.e., `GetRegionOrMarkerInfo_Value` is present).
pub fn supports_ruler_lanes() -> bool {
    let low = reaper_high::Reaper::get().medium_reaper().low();
    low.pointers().GetRegionOrMarkerInfo_Value.is_some()
}

/// Get the lane number for a marker/region by its enumeration index.
///
/// `idx` is the 0-based enumeration index (same as used with `EnumProjectMarkers3`).
/// Returns `None` if the API is unavailable or the marker doesn't exist.
pub fn get_marker_lane(low: &ReaperLow, project: ProjectContext, idx: u32) -> Option<u32> {
    let get_marker = low.pointers().GetRegionOrMarker?;
    let get_info = low.pointers().GetRegionOrMarkerInfo_Value?;

    let marker_ptr = unsafe { get_marker(project.to_raw(), idx as i32, ptr::null()) };
    if marker_ptr.is_null() {
        return None;
    }

    let lane = unsafe { get_info(project.to_raw(), marker_ptr, c"I_LANENUMBER".as_ptr()) };
    Some(lane as u32)
}

/// Set the lane number for a marker/region by its enumeration index.
///
/// `idx` is the 0-based enumeration index.
/// Returns `true` on success.
pub fn set_marker_lane(
    low: &ReaperLow,
    project: ProjectContext,
    idx: u32,
    lane: u32,
) -> bool {
    let get_marker = match low.pointers().GetRegionOrMarker {
        Some(f) => f,
        None => return false,
    };
    let set_info = match low.pointers().SetRegionOrMarkerInfo_Value {
        Some(f) => f,
        None => return false,
    };

    let marker_ptr = unsafe { get_marker(project.to_raw(), idx as i32, ptr::null()) };
    if marker_ptr.is_null() {
        return false;
    }

    unsafe { set_info(project.to_raw(), marker_ptr, c"I_LANENUMBER".as_ptr(), lane as f64) };
    true
}
