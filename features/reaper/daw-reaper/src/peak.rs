//! `impl Peaks for Reaper` — track meters + take waveform peaks.

use daw_proto::{ItemRef, Peaks, ProjectContext, TakePeakData, TakeRef, TrackPeak, TrackRef};
use reaper_high::Reaper;

use crate::project_context::find_project_by_guid;
use crate::safe_wrappers::peak as peak_sw;
use crate::track::resolve_track_pub;

fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

impl Peaks for crate::Reaper {
    fn track_peak(&self, project: ProjectContext, track: TrackRef, channel: u32) -> TrackPeak {
        (|| -> Option<TrackPeak> {
            let proj = resolve_project(&project)?;
            let t = resolve_track_pub(&proj, &track)?;
            let raw = t.raw().ok()?;
            let low = Reaper::get().medium_reaper().low();
            let peak_linear = peak_sw::track_get_peak_info(low, raw, channel as i32);
            let peak_hold_db = peak_sw::track_get_peak_hold_db(low, raw, channel as i32, false);
            let peak_db = if peak_linear > 0.0 {
                20.0 * peak_linear.log10()
            } else {
                -150.0
            };
            Some(TrackPeak {
                peak_db,
                peak_hold_db,
            })
        })()
        .unwrap_or_default()
    }

    fn take_peaks(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        block_size: u32,
    ) -> TakePeakData {
        (|| -> Option<TakePeakData> {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();

            let reaper_project_ctx = match &project {
                ProjectContext::Current => reaper_medium::ProjectContext::CurrentProject,
                ProjectContext::Project(guid) => {
                    let proj = find_project_by_guid(guid)?;
                    reaper_medium::ProjectContext::Proj(proj.raw())
                }
            };

            let midi_item = crate::midi::resolve_item(medium, reaper_project_ctx, &item)?;
            let midi_take = crate::midi::resolve_take(medium, midi_item, &take)?;
            let source = crate::safe_wrappers::item::get_take_source(medium, midi_take)?;
            let item_medium = crate::safe_wrappers::item::get_take_item(low, midi_take)?;
            let length = crate::safe_wrappers::item::get_item_info_value(
                medium,
                item_medium,
                reaper_medium::ItemAttributeKey::Length,
            );

            let peak_rate = 44100.0 / block_size as f64;
            let num_channels = 2i32;
            let num_peaks = (length * peak_rate).ceil() as i32;
            if num_peaks <= 0 {
                return Some(TakePeakData::default());
            }
            let buf_size = (num_channels * num_peaks) as usize;
            let mut buf = vec![0.0f64; buf_size];
            let peaks_read = peak_sw::pcm_source_get_peaks(
                low,
                source,
                peak_rate,
                0.0,
                num_channels,
                num_peaks,
                0,
                &mut buf,
            );
            if peaks_read <= 0 {
                return Some(TakePeakData::default());
            }
            let actual_size = (num_channels * peaks_read) as usize;
            buf.truncate(actual_size);

            Some(TakePeakData {
                sample_rate: 44100.0,
                num_channels: num_channels as u32,
                peaks: buf,
                samples_per_peak: block_size,
            })
        })()
        .unwrap_or_default()
    }
}

// ── Streaming: ~30 Hz meter frames into DawEventHub ──────────────────
//
// Meter frames stream from the central hub, fed by
// [`poll_and_broadcast_meters`] — called from the extension's ~30 Hz
// timer callback on the REAPER main thread (the same pump that drives
// `poll_and_broadcast_transport`; REAPER API calls are main-thread-only,
// so no spawned task can do this). Same wire semantics as
// daw-standalone's meter pump: one [`daw_proto::MeterFrame`] per tick
// for the active project, `tracks[i]` = project track index `i` (the
// order `Tracks::all` returns), linear `0..1` peak + decaying hold.

impl daw_proto::PeaksStreamSource for crate::Reaper {
    fn meters_hub(&self) -> &architect::PubSub<daw_proto::MeterFrame> {
        crate::event_hub::hub().meters_hub()
    }
}

/// Peak-hold decay applied per ~30 Hz poll tick. Standalone decays its
/// hold per audio block (`HOLD_DECAY = 0.96` at ≈100 blocks/s, i.e.
/// ×0.0169/s); `0.873^30 ≈ 0.0169` gives the same ~half-second fall at
/// the poll rate, so holds behave identically across backends.
pub const HOLD_DECAY_PER_TICK: f32 = 0.873;

/// Per-track decaying peak-hold state for one project's meter stream.
///
/// REAPER's own `Track_GetPeakHoldDB` hold has host-defined
/// latch/reset semantics; computing the hold here from the raw block
/// peaks keeps the wire contract byte-identical to daw-standalone's
/// `TrackMeter` (hold = `max(prev_hold * decay, peak)`, never below
/// the instantaneous peak). Pure — unit-testable without REAPER.
#[derive(Default)]
pub struct MeterHoldState {
    /// Decaying (L, R) hold per track index.
    holds: Vec<(f32, f32)>,
}

impl MeterHoldState {
    /// Fold one tick of per-track `(left, right)` block peaks into the
    /// decaying holds and return the assembled per-track levels, in
    /// input order. Resizes to the input's track count (tracks added
    /// since the last tick start from silence; removed tracks drop
    /// their hold state).
    pub fn frame_levels(
        &mut self,
        peaks: &[(f32, f32)],
        hold_decay: f32,
    ) -> Vec<daw_proto::TrackLevels> {
        self.holds.resize(peaks.len(), (0.0, 0.0));
        self.holds
            .iter_mut()
            .zip(peaks)
            .map(|(hold, &(left, right))| {
                hold.0 = (hold.0 * hold_decay).max(left);
                hold.1 = (hold.1 * hold_decay).max(right);
                daw_proto::TrackLevels {
                    peak_left: left,
                    peak_right: right,
                    hold_left: hold.0,
                    hold_right: hold.1,
                }
            })
            .collect()
    }
}

/// Hold state per project GUID, so tab switches don't cross-pollute
/// holds and each project's meters resume where they left off.
static METER_HOLDS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, MeterHoldState>>,
> = std::sync::OnceLock::new();

fn meter_holds() -> &'static std::sync::Mutex<std::collections::HashMap<String, MeterHoldState>> {
    METER_HOLDS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Poll REAPER track peaks for the ACTIVE project and publish one
/// [`daw_proto::MeterFrame`] through the [`crate::event_hub`] meters
/// hub. Skips all work while the stream has no subscribers.
///
/// **MUST be called from the REAPER main thread** — typically the
/// extension's ~30 Hz timer callback (`Track_GetPeakInfo` and project
/// enumeration are main-thread-only).
pub fn poll_and_broadcast_meters() {
    let hub = crate::event_hub::hub();
    if hub.meters_subscriber_count() == 0 {
        return;
    }

    let reaper = Reaper::get();
    let project = reaper.current_project();
    let project_guid = crate::project_context::project_guid(&project);
    let low = reaper.medium_reaper().low();

    // Post-fader block peaks, linear, in project track order — the
    // same order `Tracks::all` returns, so `tracks[i]` on the frame is
    // project track index `i` (standalone's contract).
    let peaks: Vec<(f32, f32)> = project
        .tracks()
        .map(|t| match t.raw() {
            Ok(raw) => (
                peak_sw::track_get_peak_info(low, raw, 0).max(0.0) as f32,
                peak_sw::track_get_peak_info(low, raw, 1).max(0.0) as f32,
            ),
            Err(_) => (0.0, 0.0),
        })
        .collect();

    let tracks = {
        let mut holds = meter_holds().lock().expect("meter holds mutex poisoned");
        holds
            .entry(project_guid.clone())
            .or_default()
            .frame_levels(&peaks, HOLD_DECAY_PER_TICK)
    };

    hub.publish_meter_frame(daw_proto::MeterFrame {
        project_guid,
        tracks,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames assemble from a fake peak source in input (project) order.
    #[test]
    fn frame_assembly_preserves_track_order() {
        let mut state = MeterHoldState::default();
        let levels = state.frame_levels(&[(0.1, 0.2), (0.3, 0.4), (0.5, 0.6)], 0.9);
        assert_eq!(levels.len(), 3);
        assert_eq!((levels[0].peak_left, levels[0].peak_right), (0.1, 0.2));
        assert_eq!((levels[1].peak_left, levels[1].peak_right), (0.3, 0.4));
        assert_eq!((levels[2].peak_left, levels[2].peak_right), (0.5, 0.6));
    }

    /// Hold latches to the peak, decays on quieter ticks, and never
    /// falls below the instantaneous peak — standalone `TrackMeter`
    /// semantics.
    #[test]
    fn hold_latches_decays_and_stays_above_peak() {
        let mut state = MeterHoldState::default();

        let first = state.frame_levels(&[(0.8, 0.4)], 0.5);
        assert_eq!(first[0].hold_left, 0.8); // first tick latches up
        assert_eq!(first[0].hold_right, 0.4);

        // Quieter tick: peak follows, hold decays (0.8 * 0.5 = 0.4).
        let second = state.frame_levels(&[(0.1, 0.1)], 0.5);
        assert_eq!(second[0].peak_left, 0.1);
        assert!((second[0].hold_left - 0.4).abs() < 1e-6);
        assert!(second[0].hold_left >= second[0].peak_left);
        assert!(second[0].hold_right >= second[0].peak_right);

        // Louder transient re-latches.
        let third = state.frame_levels(&[(0.9, 0.9)], 0.5);
        assert_eq!(third[0].hold_left, 0.9);
    }

    /// Track count changes are followed: new tracks start silent, and
    /// shrinking drops trailing hold state.
    #[test]
    fn resizes_with_track_count() {
        let mut state = MeterHoldState::default();
        state.frame_levels(&[(0.9, 0.9)], 0.9);

        let grown = state.frame_levels(&[(0.9, 0.9), (0.0, 0.0)], 0.9);
        assert_eq!(grown.len(), 2);
        assert_eq!(grown[1].hold_left, 0.0); // new track has no stale hold

        let shrunk = state.frame_levels(&[(0.1, 0.1)], 0.9);
        assert_eq!(shrunk.len(), 1);
    }
}
