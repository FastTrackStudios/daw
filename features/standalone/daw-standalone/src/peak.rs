//! `impl Peaks for Standalone` — stub. No metering on the mock backend.

use daw_proto::{ItemRef, Peaks, ProjectContext, TakePeakData, TakeRef, TrackPeak, TrackRef};

use crate::sync::Standalone;

impl Peaks for Standalone {
    fn track_peak(&self, _project: ProjectContext, _track: TrackRef, _channel: u32) -> TrackPeak {
        TrackPeak::default()
    }

    fn take_peaks(
        &self,
        _project: ProjectContext,
        _item: ItemRef,
        _take: TakeRef,
        _block_size: u32,
    ) -> TakePeakData {
        TakePeakData::default()
    }
}
