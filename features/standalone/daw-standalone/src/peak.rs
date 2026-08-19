//! `impl Peaks for Standalone` — reads the live per-track meter bank.
//!
//! The audio engine writes linear block peaks into [`crate::metering::Meters`];
//! here we resolve the track to its index, read its cell, and convert to dB. No
//! audio engine attached ⇒ the bank is empty ⇒ silence (`-150 dB`), matching the
//! old stub. Take-waveform peaks read the take's materialized source through
//! [`crate::take_reader::TakeReader`], in take playback time.

use daw_proto::{
    ItemRef, Peaks, ProjectContext, TakePeakData, TakeRef, TrackPeak, TrackRef, Tracks,
};

use crate::metering::linear_to_db;
use crate::sync::Standalone;

// Meter frames stream from the hub fed by the ~30 Hz pump, spawned
// lazily on the first subscription (`meters_hub` is called from the
// stream host's async attach path — see `sync/daw.rs`).
impl daw_proto::PeaksStreamSource for Standalone {
    fn meters_hub(&self) -> &architect::PubSub<daw_proto::MeterFrame> {
        self.spawn_meter_pump();
        &self.meter_events
    }
}

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

    // r[impl drums.open.peaks]
    #[cfg(any(feature = "audio", feature = "decode"))]
    fn take_peaks(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        block_size: u32,
    ) -> TakePeakData {
        // A take with no materialized media has no waveform: empty, not
        // an error, so a lane can still be laid out around it.
        let Some(reader) =
            crate::take_reader::TakeReader::open_or_decode(self, project, item, take)
        else {
            return TakePeakData::default();
        };
        let rate = reader.sample_rate() as f64;
        let channels = reader.channels().max(1) as u32;
        let block = block_size.max(1);
        if rate <= 0.0 || reader.length <= 0.0 {
            return TakePeakData {
                sample_rate: rate,
                num_channels: channels,
                peaks: Vec::new(),
                samples_per_peak: block,
            };
        }
        // Blocks are in take playback time at the source rate, so a
        // block spans the same wall time whatever the play rate or the
        // markers do to the source underneath.
        let block_secs = block as f64 / rate;
        let blocks = (reader.length / block_secs).ceil() as usize;
        let stride = channels as usize * 2;
        let mut peaks = vec![0.0f64; blocks * stride];
        for (b, out) in peaks.chunks_mut(stride).enumerate() {
            let t0 = b as f64 * block_secs;
            reader.peak_block(t0, t0 + block_secs, out);
        }
        TakePeakData {
            sample_rate: rate,
            num_channels: channels,
            peaks,
            samples_per_peak: block,
        }
    }

    #[cfg(not(any(feature = "audio", feature = "decode")))]
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
