//! REAPER Tempo Map Implementation
//!
//! Implements TempoMapService for REAPER's tempo/time signature system.
//! Uses low-level REAPER APIs via medium_reaper().low() for tempo marker access.

use crate::main_thread;
use daw_proto::{
    Position, ProjectContext, TempoMapService, TempoPoint, TimePosition, TimeSignature,
};
use reaper_high::Reaper;
use reaper_medium::{MeasureMode, ProjectContext as ReaperProjectContext};
use roam::Context;
use std::ptr::null_mut;
use tracing::debug;

/// REAPER tempo map implementation.
///
/// Provides full access to REAPER's tempo envelope and time signature markers
/// using low-level APIs.
#[derive(Clone)]
pub struct ReaperTempoMap;

impl ReaperTempoMap {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperTempoMap {
    fn default() -> Self {
        Self::new()
    }
}

impl TempoMapService for ReaperTempoMap {
    // =========================================================================
    // Query Methods
    // =========================================================================

    async fn get_tempo_points(&self, _cx: &Context, _project: ProjectContext) -> Vec<TempoPoint> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();
            let count = medium.count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);

            let mut points = Vec::with_capacity(count as usize);

            for i in 0..count {
                let mut timepos: f64 = 0.0;
                let mut measurepos: i32 = 0;
                let mut beatpos: f64 = 0.0;
                let mut bpm: f64 = 120.0;
                let mut timesig_num: i32 = 0;
                let mut timesig_denom: i32 = 0;
                let mut lineartempo: bool = false;

                let exists = unsafe {
                    low.GetTempoTimeSigMarker(
                        null_mut(), // current project
                        i as i32,
                        &mut timepos,
                        &mut measurepos,
                        &mut beatpos,
                        &mut bpm,
                        &mut timesig_num,
                        &mut timesig_denom,
                        &mut lineartempo,
                    )
                };

                if exists {
                    let time_sig = if timesig_num > 0 && timesig_denom > 0 {
                        Some(TimeSignature::new(timesig_num as u32, timesig_denom as u32))
                    } else {
                        None
                    };

                    points.push(TempoPoint {
                        position: Position::from_time(TimePosition::from_seconds(timepos)),
                        bpm,
                        time_signature: time_sig,
                        shape: None,
                        bezier_tension: None,
                        selected: None,
                        linear: Some(lineartempo),
                    });
                }
            }

            points
        })
        .await
        .unwrap_or_default()
    }

    async fn get_tempo_point(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        index: u32,
    ) -> Option<TempoPoint> {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            let mut timepos: f64 = 0.0;
            let mut measurepos: i32 = 0;
            let mut beatpos: f64 = 0.0;
            let mut bpm: f64 = 120.0;
            let mut timesig_num: i32 = 0;
            let mut timesig_denom: i32 = 0;
            let mut lineartempo: bool = false;

            let exists = unsafe {
                low.GetTempoTimeSigMarker(
                    null_mut(),
                    index as i32,
                    &mut timepos,
                    &mut measurepos,
                    &mut beatpos,
                    &mut bpm,
                    &mut timesig_num,
                    &mut timesig_denom,
                    &mut lineartempo,
                )
            };

            if exists {
                let time_sig = if timesig_num > 0 && timesig_denom > 0 {
                    Some(TimeSignature::new(timesig_num as u32, timesig_denom as u32))
                } else {
                    None
                };

                Some(TempoPoint {
                    position: Position::from_time(TimePosition::from_seconds(timepos)),
                    bpm,
                    time_signature: time_sig,
                    shape: None,
                    bezier_tension: None,
                    selected: None,
                    linear: Some(lineartempo),
                })
            } else {
                None
            }
        })
        .await
        .unwrap_or(None)
    }

    async fn tempo_point_count(&self, _cx: &Context, _project: ProjectContext) -> usize {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper
                .medium_reaper()
                .count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject)
                as usize
        })
        .await
        .unwrap_or(0)
    }

    async fn get_tempo_at(&self, _cx: &Context, _project: ProjectContext, seconds: f64) -> f64 {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Ok(pos) = reaper_medium::PositionInSeconds::new(seconds) {
                medium
                    .time_map_2_get_divided_bpm_at_time(ReaperProjectContext::CurrentProject, pos)
                    .get()
            } else {
                reaper.current_project().tempo().bpm().get()
            }
        })
        .await
        .unwrap_or(120.0)
    }

    async fn get_time_signature_at(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        seconds: f64,
    ) -> (i32, i32) {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Ok(pos) = reaper_medium::PositionInSeconds::new(seconds) {
                let beat_info =
                    medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
                (
                    beat_info.time_signature.numerator.get() as i32,
                    beat_info.time_signature.denominator.get() as i32,
                )
            } else {
                (4, 4)
            }
        })
        .await
        .unwrap_or((4, 4))
    }

    async fn time_to_musical(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        seconds: f64,
    ) -> (i32, i32, f64) {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Ok(pos) = reaper_medium::PositionInSeconds::new(seconds) {
                let result =
                    medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
                let measure = result.measure_index + 1;
                let beats_since = result.beats_since_measure.get();
                let beat_in_measure = beats_since.floor() as i32 + 1;
                let fraction = beats_since.fract();
                (measure, beat_in_measure, fraction)
            } else {
                (1, 1, 0.0)
            }
        })
        .await
        .unwrap_or((1, 1, 0.0))
    }

    async fn musical_to_time(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        measure: i32,
        beat: i32,
        fraction: f64,
    ) -> f64 {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let measure_0based = (measure - 1).max(0);
            let beat_0based = (beat - 1).max(0) as f64 + fraction;

            if let Ok(beats) = reaper_medium::PositionInBeats::new(beat_0based) {
                let result = medium.time_map_2_beats_to_time(
                    ReaperProjectContext::CurrentProject,
                    MeasureMode::FromMeasureAtIndex(measure_0based),
                    beats,
                );
                result.get()
            } else {
                0.0
            }
        })
        .await
        .unwrap_or(0.0)
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    async fn add_tempo_point(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        seconds: f64,
        bpm: f64,
    ) -> u32 {
        debug!(
            "ReaperTempoMap: add_tempo_point at {} seconds, {} BPM",
            seconds, bpm
        );
        main_thread::query(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // -1 means add new marker
            let result = unsafe {
                low.SetTempoTimeSigMarker(
                    null_mut(), // current project
                    -1,         // -1 = add new
                    seconds,
                    -1,   // measurepos (-1 = auto)
                    -1.0, // beatpos (-1 = auto)
                    bpm,
                    0,     // timesig_num (0 = don't change)
                    0,     // timesig_denom (0 = don't change)
                    false, // lineartempo
                )
            };

            if result {
                // Return the new marker count - 1 as the index
                let count = reaper
                    .medium_reaper()
                    .count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);
                count.saturating_sub(1)
            } else {
                0
            }
        })
        .await
        .unwrap_or(0)
    }

    async fn remove_tempo_point(&self, _cx: &Context, _project: ProjectContext, index: u32) {
        debug!("ReaperTempoMap: remove_tempo_point at index {}", index);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            unsafe {
                low.DeleteTempoTimeSigMarker(null_mut(), index as i32);
            }
        });
    }

    async fn set_tempo_at_point(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        index: u32,
        bpm: f64,
    ) {
        debug!(
            "ReaperTempoMap: set_tempo_at_point index {} to {} BPM",
            index, bpm
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // First get existing marker info
            let mut timepos: f64 = 0.0;
            let mut measurepos: i32 = 0;
            let mut beatpos: f64 = 0.0;
            let mut _old_bpm: f64 = 120.0;
            let mut timesig_num: i32 = 0;
            let mut timesig_denom: i32 = 0;
            let mut lineartempo: bool = false;

            let exists = unsafe {
                low.GetTempoTimeSigMarker(
                    null_mut(),
                    index as i32,
                    &mut timepos,
                    &mut measurepos,
                    &mut beatpos,
                    &mut _old_bpm,
                    &mut timesig_num,
                    &mut timesig_denom,
                    &mut lineartempo,
                )
            };

            if exists {
                unsafe {
                    low.SetTempoTimeSigMarker(
                        null_mut(),
                        index as i32,
                        timepos,
                        measurepos,
                        beatpos,
                        bpm,
                        timesig_num,
                        timesig_denom,
                        lineartempo,
                    );
                }
            }
        });
    }

    async fn set_time_signature_at_point(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        index: u32,
        numerator: i32,
        denominator: i32,
    ) {
        debug!(
            "ReaperTempoMap: set_time_signature_at_point index {} to {}/{}",
            index, numerator, denominator
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // First get existing marker info
            let mut timepos: f64 = 0.0;
            let mut measurepos: i32 = 0;
            let mut beatpos: f64 = 0.0;
            let mut bpm: f64 = 120.0;
            let mut _timesig_num: i32 = 0;
            let mut _timesig_denom: i32 = 0;
            let mut lineartempo: bool = false;

            let exists = unsafe {
                low.GetTempoTimeSigMarker(
                    null_mut(),
                    index as i32,
                    &mut timepos,
                    &mut measurepos,
                    &mut beatpos,
                    &mut bpm,
                    &mut _timesig_num,
                    &mut _timesig_denom,
                    &mut lineartempo,
                )
            };

            if exists {
                unsafe {
                    low.SetTempoTimeSigMarker(
                        null_mut(),
                        index as i32,
                        timepos,
                        measurepos,
                        beatpos,
                        bpm,
                        numerator,
                        denominator,
                        lineartempo,
                    );
                }
            }
        });
    }

    async fn move_tempo_point(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        index: u32,
        seconds: f64,
    ) {
        debug!(
            "ReaperTempoMap: move_tempo_point index {} to {} seconds",
            index, seconds
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // First get existing marker info
            let mut _timepos: f64 = 0.0;
            let mut measurepos: i32 = 0;
            let mut beatpos: f64 = 0.0;
            let mut bpm: f64 = 120.0;
            let mut timesig_num: i32 = 0;
            let mut timesig_denom: i32 = 0;
            let mut lineartempo: bool = false;

            let exists = unsafe {
                low.GetTempoTimeSigMarker(
                    null_mut(),
                    index as i32,
                    &mut _timepos,
                    &mut measurepos,
                    &mut beatpos,
                    &mut bpm,
                    &mut timesig_num,
                    &mut timesig_denom,
                    &mut lineartempo,
                )
            };

            if exists {
                unsafe {
                    low.SetTempoTimeSigMarker(
                        null_mut(),
                        index as i32,
                        seconds, // new position
                        -1,      // auto measure
                        -1.0,    // auto beat
                        bpm,
                        timesig_num,
                        timesig_denom,
                        lineartempo,
                    );
                }
            }
        });
    }

    // =========================================================================
    // Project Defaults
    // =========================================================================

    async fn get_default_tempo(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper.current_project().tempo().bpm().get()
        })
        .await
        .unwrap_or(120.0)
    }

    async fn set_default_tempo(&self, _cx: &Context, _project: ProjectContext, bpm: f64) {
        debug!("ReaperTempoMap: set_default_tempo to {} BPM", bpm);
        main_thread::run(move || {
            let reaper = Reaper::get();
            if let Ok(bpm_value) = reaper_medium::Bpm::new(bpm) {
                let tempo = reaper_high::Tempo::from_bpm(bpm_value);
                let _ = reaper
                    .current_project()
                    .set_tempo(tempo, reaper_medium::UndoBehavior::OmitUndoPoint);
            }
        });
    }

    async fn get_default_time_signature(
        &self,
        _cx: &Context,
        _project: ProjectContext,
    ) -> (i32, i32) {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let measure_info =
                medium.time_map_get_measure_info(ReaperProjectContext::CurrentProject, 0);
            (
                measure_info.time_signature.numerator.get() as i32,
                measure_info.time_signature.denominator.get() as i32,
            )
        })
        .await
        .unwrap_or((4, 4))
    }

    async fn set_default_time_signature(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        numerator: i32,
        denominator: i32,
    ) {
        debug!(
            "ReaperTempoMap: set_default_time_signature to {}/{}",
            numerator, denominator
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // Get tempo at position 0
            let bpm = reaper.current_project().tempo().bpm().get();

            // Check if there's already a marker at position 0
            let count = reaper
                .medium_reaper()
                .count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);

            let mut found_at_zero = false;
            for i in 0..count {
                let mut timepos: f64 = 0.0;
                let mut measurepos: i32 = 0;
                let mut beatpos: f64 = 0.0;
                let mut marker_bpm: f64 = 120.0;
                let mut timesig_num: i32 = 0;
                let mut timesig_denom: i32 = 0;
                let mut lineartempo: bool = false;

                let exists = unsafe {
                    low.GetTempoTimeSigMarker(
                        null_mut(),
                        i as i32,
                        &mut timepos,
                        &mut measurepos,
                        &mut beatpos,
                        &mut marker_bpm,
                        &mut timesig_num,
                        &mut timesig_denom,
                        &mut lineartempo,
                    )
                };

                if exists && timepos < 0.001 {
                    // Update existing marker at position 0
                    unsafe {
                        low.SetTempoTimeSigMarker(
                            null_mut(),
                            i as i32,
                            0.0,
                            0,
                            0.0,
                            marker_bpm,
                            numerator,
                            denominator,
                            lineartempo,
                        );
                    }
                    found_at_zero = true;
                    break;
                }
            }

            if !found_at_zero {
                // Add new marker at position 0
                unsafe {
                    low.SetTempoTimeSigMarker(
                        null_mut(),
                        -1, // add new
                        0.0,
                        0,
                        0.0,
                        bpm,
                        numerator,
                        denominator,
                        false,
                    );
                }
            }
        });
    }
}
