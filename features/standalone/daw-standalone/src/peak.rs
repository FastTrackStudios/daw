//! `impl Peaks for Standalone` — reads the live per-track meter bank.
//!
//! The audio engine writes linear block peaks into [`crate::metering::Meters`];
//! here we resolve the track to its index, read its cell, and convert to dB. No
//! audio engine attached ⇒ the bank is empty ⇒ silence (`-150 dB`), matching the
//! old stub. Take-waveform peaks remain unimplemented (no on-disk media in the
//! standalone demo yet).

use daw_proto::{
    ItemRef, Peaks, ProjectContext, TakePeakData, TakeRef, TrackPeak, TrackRef, Tracks,
};

use crate::metering::linear_to_db;
use crate::sync::Standalone;

impl Peaks for Standalone {
    fn track_peak(&self, project: ProjectContext, track: TrackRef, channel: u32) -> TrackPeak {
        let Some(index) = self.resolve_track_index(project, &track) else {
            return TrackPeak::default();
        };
        let meters = self.meters();
        let Some(cell) = meters.cell(index) else {
            return TrackPeak::default();
        };
        TrackPeak {
            peak_db: linear_to_db(cell.peak(channel)),
            peak_hold_db: linear_to_db(cell.hold(channel)),
        }
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

impl Standalone {
    /// Resolve a [`TrackRef`] to its 0-based track index in `project`. Used to
    /// map a meter request onto a [`crate::metering::Meters`] cell. `Master` has
    /// no meter cell here, so it returns `None`.
    fn resolve_track_index(&self, project: ProjectContext, track: &TrackRef) -> Option<usize> {
        match track {
            TrackRef::Index(i) => Some(*i as usize),
            TrackRef::Master => None,
            TrackRef::Guid(guid) => self.all(project).iter().position(|t| &t.guid == guid),
        }
    }
}
