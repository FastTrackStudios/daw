//! Peak metering service trait

use super::{TakePeakData, TrackPeak};
use crate::item::{ItemRef, TakeRef};
use crate::project::ProjectContext;
use crate::track::TrackRef;
use vox::service;

/// Service for reading track peak meters and take waveform data
///
/// This service provides access to real-time peak levels for tracks
/// and waveform peak data for takes (used for waveform display).
#[service]
pub trait PeakService {
    /// Get the current peak level for a track channel
    ///
    /// Returns peak and peak-hold values in dB (0.0 = full scale, negative = below)
    async fn get_track_peak(
        &self,
        project: ProjectContext,
        track: TrackRef,
        channel: u32,
    ) -> TrackPeak;

    /// Get waveform peak data for a take
    ///
    /// The `block_size` parameter controls the resolution of peak data.
    /// Larger values = fewer peaks = faster but less detailed.
    /// Typical values: 1024-4096 samples per peak.
    async fn get_take_peaks(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        block_size: u32,
    ) -> TakePeakData;
}
