//! REAPER Position Conversion Implementation
//!
//! Uses REAPER's TimeMap APIs for accurate position conversions based on the
//! project's tempo map and time signature changes.

use crate::safe_wrappers::time_map as sw;
use daw_proto::{
    MeasureMode, PositionConversionService, PositionInBeats, PositionInQuarterNotes,
    PositionInSeconds, ProjectContext, QuarterNotesToMeasureResult, TimeSignature,
    TimeToBeatsResult, TimeToQuarterNotesResult,
};
use reaper_high::Reaper;

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
        project: ProjectContext,
        position: PositionInSeconds,
        measure_mode: MeasureMode,
    ) -> TimeToBeatsResult {
        let time = position.as_seconds();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = proj.context();

            let result = sw::time_to_beats(low, proj_ctx, time);

            // Apply measure mode
            let final_measure = match measure_mode {
                MeasureMode::IgnoreMeasure => 0,
                MeasureMode::FromMeasureAtIndex(idx) => idx - result.measure_index,
            };

            // Get time signature at this position
            let ts = sw::get_time_sig_at_time(low, proj_ctx, time);

            Some(TimeToBeatsResult {
                full_beats: PositionInBeats::from_beats(result.full_beats),
                measure_index: final_measure,
                beats_since_measure: PositionInBeats::from_beats(result.beats_frac),
                time_signature: TimeSignature::new(ts.num as u32, ts.denom as u32),
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn beats_to_time(
        &self,
        project: ProjectContext,
        position: PositionInBeats,
        measure_mode: MeasureMode,
    ) -> PositionInSeconds {
        let full_beats = position.as_beats();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = proj.context();

            // Adjust beats based on measure mode
            let adjusted_beats = match measure_mode {
                MeasureMode::IgnoreMeasure => full_beats,
                MeasureMode::FromMeasureAtIndex(measure_idx) => {
                    // Get the beat position at the start of this measure
                    let measure_start_time = sw::get_measure_info(low, proj_ctx, measure_idx);
                    let tb = sw::time_to_beats(low, proj_ctx, measure_start_time);
                    tb.full_beats + full_beats
                }
            };

            let time = sw::beats_to_time(low, proj_ctx, adjusted_beats, None);

            Some(PositionInSeconds::from_seconds(time))
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn time_to_quarter_notes(
        &self,
        project: ProjectContext,
        position: PositionInSeconds,
    ) -> TimeToQuarterNotesResult {
        let time = position.as_seconds();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = proj.context();

            let qn_position = sw::time_to_qn(low, proj_ctx, time);

            // Get measure info
            let minfo = sw::qn_to_measures(low, proj_ctx, qn_position);
            let qn_since_measure = qn_position - minfo.qn_start;

            // Get time signature
            let ts = sw::get_time_sig_at_time(low, proj_ctx, time);

            Some(TimeToQuarterNotesResult {
                quarter_notes: PositionInQuarterNotes::from_quarter_notes(qn_position),
                measure_index: minfo.measure_index,
                quarter_notes_since_measure: PositionInQuarterNotes::from_quarter_notes(
                    qn_since_measure,
                ),
                time_signature: TimeSignature::new(ts.num as u32, ts.denom as u32),
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn quarter_notes_to_time(
        &self,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInSeconds {
        let qn = position.as_quarter_notes();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();

            let time = sw::qn_to_time(low, proj.context(), qn);

            Some(PositionInSeconds::from_seconds(time))
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn quarter_notes_to_measure(
        &self,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> QuarterNotesToMeasureResult {
        let qn = position.as_quarter_notes();

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = proj.context();

            let minfo = sw::qn_to_measures(low, proj_ctx, qn);

            // Get time signature at this position
            let time = sw::qn_to_time(low, proj_ctx, qn);
            let ts = sw::get_time_sig_at_time(low, proj_ctx, time);

            Some(QuarterNotesToMeasureResult {
                measure_index: minfo.measure_index,
                start: PositionInQuarterNotes::from_quarter_notes(minfo.qn_start),
                end: PositionInQuarterNotes::from_quarter_notes(minfo.qn_end),
                time_signature: TimeSignature::new(ts.num as u32, ts.denom as u32),
            })
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn beats_to_quarter_notes(
        &self,
        project: ProjectContext,
        position: PositionInBeats,
    ) -> PositionInQuarterNotes {
        // Convert beats → time → quarter notes
        let time = self
            .beats_to_time(project.clone(), position, MeasureMode::IgnoreMeasure)
            .await;
        let result = self.time_to_quarter_notes(project, time).await;
        result.quarter_notes
    }

    async fn quarter_notes_to_beats(
        &self,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInBeats {
        // Convert quarter notes → time → beats
        let time = self.quarter_notes_to_time(project.clone(), position).await;
        let result = self
            .time_to_beats(project, time, MeasureMode::IgnoreMeasure)
            .await;
        result.full_beats
    }
}
