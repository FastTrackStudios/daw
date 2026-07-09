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
