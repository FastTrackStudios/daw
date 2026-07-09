//! Peak metering service — track meters + take waveform peak data.

use super::{TakePeakData, TrackPeak};
use crate::item::{ItemRef, TakeRef};
use crate::project::ProjectContext;
use crate::track::TrackRef;

#[architect::rpc]
pub trait Peaks {
    /// Current peak level for a track channel. Peak and peak-hold
    /// values in dB (0.0 = full scale, negative = below).
    fn track_peak(&self, project: ProjectContext, track: TrackRef, channel: u32) -> TrackPeak;

    /// Waveform peak data for a take. `block_size` controls the
    /// resolution: larger = fewer peaks = faster but less detailed.
    /// Typical values 1024–4096 samples per peak.
    fn take_peaks(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        block_size: u32,
    ) -> TakePeakData;
}
