//! REAPER Peak Service Implementation
//!
//! Implements PeakService for REAPER, providing real-time track peak metering
//! and take waveform peak data for display.

use daw_proto::{ItemRef, PeakService, ProjectContext, TakePeakData, TakeRef, TrackPeak, TrackRef};
use reaper_high::Reaper;
use roam::Context;

use crate::main_thread;
use crate::project_context::find_project_by_guid;
use crate::safe_wrappers::peak as peak_sw;
use crate::track::resolve_track_pub;

/// REAPER peak metering service implementation.
///
/// Zero-field struct — all state lives in REAPER itself.
#[derive(Clone)]
pub struct ReaperPeak;

impl ReaperPeak {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperPeak {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve a ProjectContext to a REAPER Project
fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

impl PeakService for ReaperPeak {
    async fn get_track_peak(
        &self,
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        channel: u32,
    ) -> TrackPeak {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let t = resolve_track_pub(&proj, &track)?;
            let raw = t.raw().ok()?;
            let low = Reaper::get().medium_reaper().low();

            let peak_linear = peak_sw::track_get_peak_info(low, raw, channel as i32);
            let peak_hold_db = peak_sw::track_get_peak_hold_db(low, raw, channel as i32, false);

            // Convert linear peak to dB (20 * log10)
            let peak_db = if peak_linear > 0.0 {
                20.0 * peak_linear.log10()
            } else {
                -150.0
            };

            Some(TrackPeak {
                peak_db,
                peak_hold_db,
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn get_take_peaks(
        &self,
        _cx: &Context,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        block_size: u32,
    ) -> TakePeakData {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();

            // Resolve project context to REAPER project
            let reaper_project_ctx = match &project {
                ProjectContext::Current => reaper_medium::ProjectContext::CurrentProject,
                ProjectContext::Project(guid) => {
                    let proj = find_project_by_guid(guid)?;
                    reaper_medium::ProjectContext::Proj(proj.raw())
                }
            };

            // Resolve item
            let midi_item = crate::midi::ReaperMidi::resolve_item(
                medium,
                reaper_project_ctx,
                &item,
            )?;

            // Resolve take
            let midi_take = crate::midi::ReaperMidi::resolve_take(medium, midi_item, &take)?;

            // Get PCM source
            let source = crate::safe_wrappers::item::get_take_source(medium, midi_take)?;

            // Get source length and sample rate from the take's item
            let take_item = crate::safe_wrappers::item::get_take_item(low, midi_take);
            let item_medium = reaper_medium::MediaItem::new(take_item)?;
            let length = crate::safe_wrappers::item::get_item_info_value(
                medium,
                item_medium,
                reaper_medium::ItemAttributeKey::Length,
            );

            // Calculate peak data dimensions
            let peak_rate = 44100.0 / block_size as f64;
            let num_channels = 2i32; // stereo
            let num_peaks = (length * peak_rate).ceil() as i32;

            if num_peaks <= 0 {
                return Some(TakePeakData::default());
            }

            // Allocate buffer and read peaks
            let buf_size = (num_channels * num_peaks) as usize;
            let mut buf = vec![0.0f64; buf_size];

            let peaks_read = peak_sw::pcm_source_get_peaks(
                low,
                source,
                peak_rate,
                0.0, // start_time
                num_channels,
                num_peaks,
                0, // want_extra_type: 0 = peak data
                &mut buf,
            );

            if peaks_read <= 0 {
                return Some(TakePeakData::default());
            }

            // Truncate buffer to actual peaks read
            let actual_size = (num_channels * peaks_read) as usize;
            buf.truncate(actual_size);

            Some(TakePeakData {
                sample_rate: 44100.0,
                num_channels: num_channels as u32,
                peaks: buf,
                samples_per_peak: block_size,
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }
}
