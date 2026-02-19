//! REAPER Position Conversion Implementation
//!
//! Uses REAPER's TimeMap APIs for accurate position conversions based on the
//! project's tempo map and time signature changes.

use daw_proto::{
    MeasureMode, PositionConversionService, PositionInBeats, PositionInQuarterNotes,
    PositionInSeconds, ProjectContext, QuarterNotesToMeasureResult, TimeSignature,
    TimeToBeatsResult, TimeToQuarterNotesResult,
};
use reaper_high::Reaper;
use roam::Context;

use crate::main_thread;

/// REAPER position conversion implementation using TimeMap APIs
#[derive(Clone)]
pub struct ReaperPositionConversion;

impl ReaperPositionConversion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperPositionConversion {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve a daw_proto::ProjectContext to a reaper_high::Project
fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => {
            // Enumerate all projects to find one with matching GUID
            use crate::project_context::find_project_by_guid;
            find_project_by_guid(guid)
        }
    }
}

impl PositionConversionService for ReaperPositionConversion {
    async fn time_to_beats(
        &self,
        _cx: &Context,
        project: ProjectContext,
        position: PositionInSeconds,
        measure_mode: MeasureMode,
    ) -> TimeToBeatsResult {
        let time = position.as_seconds();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();

            let mut measure_index: i32 = 0;
            let mut beats_since_measure: i32 = 0;
            let mut full_beats: f64 = 0.0;
            let mut denom: i32 = 0;

            let beats_frac = unsafe {
                low.TimeMap2_timeToBeats(
                    proj.raw().as_ptr(),
                    time,
                    &mut measure_index,
                    &mut beats_since_measure,
                    &mut full_beats,
                    &mut denom,
                )
            };

            // Apply measure mode
            let final_measure = match measure_mode {
                MeasureMode::IgnoreMeasure => 0,
                MeasureMode::FromMeasureAtIndex(idx) => idx - measure_index,
            };

            // Get time signature at this position
            let mut num: i32 = 4;
            let mut denom: i32 = 4;
            let mut tempo: f64 = 120.0;
            unsafe {
                low.TimeMap_GetTimeSigAtTime(
                    proj.raw().as_ptr(),
                    time,
                    &mut num,
                    &mut denom,
                    &mut tempo,
                );
            }

            Some(TimeToBeatsResult {
                full_beats: PositionInBeats::from_beats(full_beats),
                measure_index: final_measure,
                beats_since_measure: PositionInBeats::from_beats(beats_frac),
                time_signature: TimeSignature::new(num as u32, denom as u32),
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn beats_to_time(
        &self,
        _cx: &Context,
        project: ProjectContext,
        position: PositionInBeats,
        measure_mode: MeasureMode,
    ) -> PositionInSeconds {
        let full_beats = position.as_beats();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();

            // Adjust beats based on measure mode
            let adjusted_beats = match measure_mode {
                MeasureMode::IgnoreMeasure => full_beats,
                MeasureMode::FromMeasureAtIndex(measure_idx) => {
                    // Get the beat position at the start of this measure
                    let measure_start_time = unsafe {
                        low.TimeMap_GetMeasureInfo(
                            proj.raw().as_ptr(),
                            measure_idx,
                            std::ptr::null_mut(), // qn_startOut
                            std::ptr::null_mut(), // qn_endOut
                            std::ptr::null_mut(), // timesig_numOut
                            std::ptr::null_mut(), // timesig_denomOut
                            std::ptr::null_mut(), // tempoOut
                        )
                    };
                    let mut measure_start_beats = 0.0;
                    unsafe {
                        low.TimeMap2_timeToBeats(
                            proj.raw().as_ptr(),
                            measure_start_time,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &mut measure_start_beats,
                            std::ptr::null_mut(),
                        );
                    }
                    measure_start_beats + full_beats
                }
            };

            let time = unsafe {
                low.TimeMap2_beatsToTime(proj.raw().as_ptr(), adjusted_beats, std::ptr::null())
            };

            Some(PositionInSeconds::from_seconds(time))
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn time_to_quarter_notes(
        &self,
        _cx: &Context,
        project: ProjectContext,
        position: PositionInSeconds,
    ) -> TimeToQuarterNotesResult {
        let time = position.as_seconds();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();

            let qn_position = unsafe { low.TimeMap2_timeToQN(proj.raw().as_ptr(), time) };

            // Get measure info
            let mut qn_measure_start: f64 = 0.0;
            let mut qn_measure_end: f64 = 0.0;
            let measure_index = unsafe {
                low.TimeMap_QNToMeasures(
                    proj.raw().as_ptr(),
                    qn_position,
                    &mut qn_measure_start,
                    &mut qn_measure_end,
                )
            };

            let qn_since_measure = qn_position - qn_measure_start;

            // Get time signature
            let mut num: i32 = 4;
            let mut denom: i32 = 4;
            let mut tempo: f64 = 120.0;
            unsafe {
                low.TimeMap_GetTimeSigAtTime(
                    proj.raw().as_ptr(),
                    time,
                    &mut num,
                    &mut denom,
                    &mut tempo,
                );
            }

            Some(TimeToQuarterNotesResult {
                quarter_notes: PositionInQuarterNotes::from_quarter_notes(qn_position),
                measure_index,
                quarter_notes_since_measure: PositionInQuarterNotes::from_quarter_notes(
                    qn_since_measure,
                ),
                time_signature: TimeSignature::new(num as u32, denom as u32),
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn quarter_notes_to_time(
        &self,
        _cx: &Context,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInSeconds {
        let qn = position.as_quarter_notes();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();

            let time = unsafe { low.TimeMap2_QNToTime(proj.raw().as_ptr(), qn) };

            Some(PositionInSeconds::from_seconds(time))
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn quarter_notes_to_measure(
        &self,
        _cx: &Context,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> QuarterNotesToMeasureResult {
        let qn = position.as_quarter_notes();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();

            let mut qn_measure_start: f64 = 0.0;
            let mut qn_measure_end: f64 = 0.0;

            let measure_index = unsafe {
                low.TimeMap_QNToMeasures(
                    proj.raw().as_ptr(),
                    qn,
                    &mut qn_measure_start,
                    &mut qn_measure_end,
                )
            };

            // Get time signature at this position
            let time = unsafe { low.TimeMap2_QNToTime(proj.raw().as_ptr(), qn) };
            let mut num: i32 = 4;
            let mut denom: i32 = 4;
            let mut tempo: f64 = 120.0;
            unsafe {
                low.TimeMap_GetTimeSigAtTime(
                    proj.raw().as_ptr(),
                    time,
                    &mut num,
                    &mut denom,
                    &mut tempo,
                );
            }

            Some(QuarterNotesToMeasureResult {
                measure_index,
                start: PositionInQuarterNotes::from_quarter_notes(qn_measure_start),
                end: PositionInQuarterNotes::from_quarter_notes(qn_measure_end),
                time_signature: TimeSignature::new(num as u32, denom as u32),
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn beats_to_quarter_notes(
        &self,
        cx: &Context,
        project: ProjectContext,
        position: PositionInBeats,
    ) -> PositionInQuarterNotes {
        // Convert beats → time → quarter notes
        let time = self
            .beats_to_time(cx, project.clone(), position, MeasureMode::IgnoreMeasure)
            .await;
        let result = self.time_to_quarter_notes(cx, project, time).await;
        result.quarter_notes
    }

    async fn quarter_notes_to_beats(
        &self,
        cx: &Context,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInBeats {
        // Convert quarter notes → time → beats
        let time = self
            .quarter_notes_to_time(cx, project.clone(), position)
            .await;
        let result = self
            .time_to_beats(cx, project, time, MeasureMode::IgnoreMeasure)
            .await;
        result.full_beats
    }
}
