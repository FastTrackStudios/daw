//! Safe wrappers for REAPER peak metering and waveform APIs.

use super::ReaperLow;
use reaper_medium::MediaTrack;

/// Get the current peak level for a track channel (linear scale, typically 0.0–1.0+).
pub fn track_get_peak_info(low: &ReaperLow, track: MediaTrack, channel: i32) -> f64 {
    unsafe { low.Track_GetPeakInfo(track.as_ptr(), channel) }
}

/// Get the peak hold level in dB for a track channel.
/// Pass `clear = false` to read without resetting the hold.
pub fn track_get_peak_hold_db(
    low: &ReaperLow,
    track: MediaTrack,
    channel: i32,
    clear: bool,
) -> f64 {
    unsafe { low.Track_GetPeakHoldDB(track.as_ptr(), channel, clear) }
}

/// Read peak data from a PCM source (for waveform display).
///
/// Returns the number of peaks actually read (may be less than requested).
/// `buf` must be pre-allocated with at least `num_channels * num_samples_per_channel` elements.
pub fn pcm_source_get_peaks(
    low: &ReaperLow,
    source: reaper_medium::PcmSource,
    peak_rate: f64,
    start_time: f64,
    num_channels: i32,
    num_samples_per_channel: i32,
    want_extra_type: i32,
    buf: &mut [f64],
) -> i32 {
    unsafe {
        low.PCM_Source_GetPeaks(
            source.as_ptr(),
            peak_rate,
            start_time,
            num_channels,
            num_samples_per_channel,
            want_extra_type,
            buf.as_mut_ptr(),
        )
    }
}
