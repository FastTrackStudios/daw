//! Safe wrappers around `reaper-low` envelope APIs.
//!
//! reaper-medium / reaper-high don't expose the envelope CRUD surface we
//! need (point insertion, point reads with shape + tension, evaluation,
//! state-chunk getters). This module wraps the C calls in Rust types
//! and assumes the caller is on the REAPER main thread (use
//! [`crate::main_thread::query`] / [`crate::main_thread::run`] from
//! service code).
//!
//! All functions take `&Reaper` (low-level) so callers don't have to
//! re-deref the pointer chain. Pointers (`MediaTrack`, `TrackEnvelope`)
//! are passed through unchanged — REAPER guarantees their stability for
//! the lifetime of the project. Each function null-checks before
//! dereffing, so passing dangling/null pointers is defined behavior
//! (returns `None` / `false` / `0`).
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::too_many_arguments)]

use reaper_low::Reaper;
use reaper_low::raw::{MediaTrack, TrackEnvelope};
use std::ffi::{CStr, CString};

/// Number of envelopes attached to a track. `0` if `track` is null.
pub fn count_track_envelopes(low: &Reaper, track: *mut MediaTrack) -> u32 {
    if track.is_null() {
        return 0;
    }
    unsafe { low.CountTrackEnvelopes(track).max(0) as u32 }
}

/// Get the Nth track envelope by index. Returns null if out of range.
pub fn get_track_envelope(low: &Reaper, track: *mut MediaTrack, index: u32) -> *mut TrackEnvelope {
    if track.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { low.GetTrackEnvelope(track, index as i32) }
}

/// Resolve a track envelope by display name (e.g. "Volume", "Pan",
/// "Width"). Returns null if no match.
pub fn get_track_envelope_by_name(
    low: &Reaper,
    track: *mut MediaTrack,
    name: &str,
) -> *mut TrackEnvelope {
    if track.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(c) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    unsafe { low.GetTrackEnvelopeByName(track, c.as_ptr()) }
}

/// Resolve a track envelope by chunk-name tag (e.g. `<VOLENV2`,
/// `<PANENV2`). Used when distinguishing pre-/post-FX envelopes that
/// share a display name.
pub fn get_track_envelope_by_chunk_name(
    low: &Reaper,
    track: *mut MediaTrack,
    chunk_name: &str,
) -> *mut TrackEnvelope {
    if track.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(c) = CString::new(chunk_name) else {
        return std::ptr::null_mut();
    };
    unsafe { low.GetTrackEnvelopeByChunkName(track, c.as_ptr()) }
}

/// Get the human-readable envelope name. Returns `None` on failure.
pub fn get_envelope_name(low: &Reaper, envelope: *mut TrackEnvelope) -> Option<String> {
    if envelope.is_null() {
        return None;
    }
    let mut buf = vec![0i8; 256];
    let ok = unsafe { low.GetEnvelopeName(envelope, buf.as_mut_ptr() as *mut _, buf.len() as i32) };
    if !ok {
        return None;
    }
    let cstr = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
    cstr.to_str().ok().map(|s| s.to_string())
}

/// Number of points in an envelope. `0` for null envelope.
pub fn count_envelope_points(low: &Reaper, envelope: *mut TrackEnvelope) -> u32 {
    if envelope.is_null() {
        return 0;
    }
    unsafe { low.CountEnvelopePoints(envelope).max(0) as u32 }
}

/// Single envelope point read.
#[derive(Clone, Debug, Default)]
pub struct PointSample {
    pub time: f64,
    pub value: f64,
    /// Raw REAPER shape index (0 linear, 1 square, 2 slow start/end,
    /// 3 fast start, 4 fast end, 5 bezier).
    pub shape: i32,
    pub tension: f64,
    pub selected: bool,
}

/// Read a single point. Returns `None` if `index` is out of range.
pub fn get_envelope_point(
    low: &Reaper,
    envelope: *mut TrackEnvelope,
    index: u32,
) -> Option<PointSample> {
    if envelope.is_null() {
        return None;
    }
    let mut time = 0.0f64;
    let mut value = 0.0f64;
    let mut shape = 0i32;
    let mut tension = 0.0f64;
    let mut selected = false;
    let ok = unsafe {
        low.GetEnvelopePoint(
            envelope,
            index as i32,
            &mut time,
            &mut value,
            &mut shape,
            &mut tension,
            &mut selected,
        )
    };
    if !ok {
        return None;
    }
    Some(PointSample {
        time,
        value,
        shape,
        tension,
        selected,
    })
}

/// Insert a new point. Returns `true` on success.
///
/// `sort` should generally be `true` so the envelope's points stay in
/// time order; pass `false` only when batching multiple inserts and
/// calling [`sort_envelope_points`] yourself afterwards.
pub fn insert_envelope_point(
    low: &Reaper,
    envelope: *mut TrackEnvelope,
    time: f64,
    value: f64,
    shape: i32,
    tension: f64,
    selected: bool,
    sort: bool,
) -> bool {
    if envelope.is_null() {
        return false;
    }
    let mut sort_in = !sort;
    unsafe {
        low.InsertEnvelopePoint(
            envelope,
            time,
            value,
            shape,
            tension,
            selected,
            &mut sort_in,
        )
    }
}

/// Update an existing point in place. Pass `None` for any field you
/// don't want to change. Returns `true` on success.
pub fn set_envelope_point(
    low: &Reaper,
    envelope: *mut TrackEnvelope,
    index: u32,
    time: Option<f64>,
    value: Option<f64>,
    shape: Option<i32>,
    tension: Option<f64>,
    selected: Option<bool>,
    sort: bool,
) -> bool {
    if envelope.is_null() {
        return false;
    }
    let mut t = time.unwrap_or(0.0);
    let mut v = value.unwrap_or(0.0);
    let mut s = shape.unwrap_or(0);
    let mut ten = tension.unwrap_or(0.0);
    let mut sel = selected.unwrap_or(false);
    let mut sort_in = !sort;
    unsafe {
        low.SetEnvelopePoint(
            envelope,
            index as i32,
            if time.is_some() {
                &mut t
            } else {
                std::ptr::null_mut()
            },
            if value.is_some() {
                &mut v
            } else {
                std::ptr::null_mut()
            },
            if shape.is_some() {
                &mut s
            } else {
                std::ptr::null_mut()
            },
            if tension.is_some() {
                &mut ten
            } else {
                std::ptr::null_mut()
            },
            if selected.is_some() {
                &mut sel
            } else {
                std::ptr::null_mut()
            },
            &mut sort_in,
        )
    }
}

/// Delete a single point by index. Returns `true` on success.
pub fn delete_envelope_point(low: &Reaper, envelope: *mut TrackEnvelope, index: u32) -> bool {
    if envelope.is_null() {
        return false;
    }
    unsafe { low.DeleteEnvelopePointEx(envelope, -1, index as i32) }
}

/// Delete every point with `start <= time <= end`. Returns `true` if
/// at least one point was removed.
pub fn delete_envelope_points_in_range(
    low: &Reaper,
    envelope: *mut TrackEnvelope,
    start: f64,
    end: f64,
) -> bool {
    if envelope.is_null() {
        return false;
    }
    unsafe { low.DeleteEnvelopePointRange(envelope, start, end) }
}

/// Force-sort an envelope's points by time. Use after batched
/// `insert_envelope_point(.., sort=false)` calls.
pub fn sort_envelope_points(low: &Reaper, envelope: *mut TrackEnvelope) -> bool {
    if envelope.is_null() {
        return false;
    }
    unsafe { low.Envelope_SortPoints(envelope) }
}

/// Evaluate the envelope at a specific time. Returns `(value, dvds,
/// ddvds, dddvds)` — value plus derivatives. Returns `None` if the
/// envelope is null.
pub fn evaluate_envelope(
    low: &Reaper,
    envelope: *mut TrackEnvelope,
    time: f64,
    samplerate: f64,
    samples_requested: i32,
) -> Option<(f64, f64, f64, f64)> {
    if envelope.is_null() {
        return None;
    }
    let mut value = 0.0f64;
    let mut dvds = 0.0f64;
    let mut ddvds = 0.0f64;
    let mut dddvds = 0.0f64;
    let _shape_changed = unsafe {
        low.Envelope_Evaluate(
            envelope,
            time,
            samplerate,
            samples_requested,
            &mut value,
            &mut dvds,
            &mut ddvds,
            &mut dddvds,
        )
    };
    Some((value, dvds, ddvds, dddvds))
}
