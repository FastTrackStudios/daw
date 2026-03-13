//! Standalone position conversion implementation
//!
//! Provides simple conversions assuming constant tempo and time signature.

use daw_proto::{
    MeasureMode, PositionConversionService, PositionInBeats, PositionInQuarterNotes,
    PositionInSeconds, ProjectContext, QuarterNotesToMeasureResult, TimeSignature,
    TimeToBeatsResult, TimeToQuarterNotesResult,
};

/// Standalone position conversion implementation
///
/// Uses simplified conversion logic assuming:
/// - Constant tempo (120 BPM by default)
/// - Constant time signature (4/4 by default)
/// - No tempo map changes
#[derive(Default)]
pub struct StandalonePositionConversion;

impl StandalonePositionConversion {
    pub fn new() -> Self {
        Self
    }

    /// Get default tempo (120 BPM)
    fn default_tempo() -> f64 {
        120.0
    }

    /// Get default time signature (4/4)
    fn default_time_signature() -> TimeSignature {
        TimeSignature::new(4, 4)
    }
}

impl PositionConversionService for StandalonePositionConversion {
    async fn time_to_beats(
        &self,
        _project: ProjectContext,
        position: PositionInSeconds,
        _measure_mode: MeasureMode,
    ) -> TimeToBeatsResult {
        let tempo = Self::default_tempo();
        let time_sig = Self::default_time_signature();

        let seconds = position.as_seconds();
        let beats_per_second = tempo / 60.0;
        let total_beats = seconds * beats_per_second;

        let beats_per_measure = time_sig.numerator() as f64;
        let measure_index = (total_beats / beats_per_measure).floor() as i32;
        let beats_since_measure = total_beats % beats_per_measure;

        TimeToBeatsResult {
            full_beats: PositionInBeats::from_beats(total_beats),
            measure_index,
            beats_since_measure: PositionInBeats::from_beats(beats_since_measure),
            time_signature: time_sig,
        }
    }

    async fn beats_to_time(
        &self,
        _project: ProjectContext,
        position: PositionInBeats,
        _measure_mode: MeasureMode,
    ) -> PositionInSeconds {
        let tempo = Self::default_tempo();
        let beats = position.as_beats();
        let seconds_per_beat = 60.0 / tempo;
        let seconds = beats * seconds_per_beat;

        PositionInSeconds::from_seconds(seconds)
    }

    async fn time_to_quarter_notes(
        &self,
        _project: ProjectContext,
        position: PositionInSeconds,
    ) -> TimeToQuarterNotesResult {
        let tempo = Self::default_tempo();
        let time_sig = Self::default_time_signature();

        let seconds = position.as_seconds();
        let qn_per_second = tempo / 60.0;
        let total_qn = seconds * qn_per_second;

        let qn_per_measure = time_sig.quarter_notes_per_measure();
        let measure_index = (total_qn / qn_per_measure).floor() as i32;
        let qn_since_measure = total_qn % qn_per_measure;

        TimeToQuarterNotesResult {
            quarter_notes: PositionInQuarterNotes::from_quarter_notes(total_qn),
            measure_index,
            quarter_notes_since_measure: PositionInQuarterNotes::from_quarter_notes(
                qn_since_measure,
            ),
            time_signature: time_sig,
        }
    }

    async fn quarter_notes_to_time(
        &self,
        _project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInSeconds {
        let tempo = Self::default_tempo();
        let qn = position.as_quarter_notes();
        let seconds_per_qn = 60.0 / tempo;
        let seconds = qn * seconds_per_qn;

        PositionInSeconds::from_seconds(seconds)
    }

    async fn quarter_notes_to_measure(
        &self,
        _project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> QuarterNotesToMeasureResult {
        let time_sig = Self::default_time_signature();
        let qn = position.as_quarter_notes();
        let qn_per_measure = time_sig.quarter_notes_per_measure();

        let measure_index = (qn / qn_per_measure).floor() as i32;
        let start = measure_index as f64 * qn_per_measure;
        let end = start + qn_per_measure;

        QuarterNotesToMeasureResult {
            measure_index,
            start: PositionInQuarterNotes::from_quarter_notes(start),
            end: PositionInQuarterNotes::from_quarter_notes(end),
            time_signature: time_sig,
        }
    }

    async fn beats_to_quarter_notes(
        &self,
        _project: ProjectContext,
        position: PositionInBeats,
    ) -> PositionInQuarterNotes {
        // In 4/4 time, beats == quarter notes
        // For other time signatures, this would need adjustment
        PositionInQuarterNotes::from_quarter_notes(position.as_beats())
    }

    async fn quarter_notes_to_beats(
        &self,
        _project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInBeats {
        // In 4/4 time, quarter notes == beats
        // For other time signatures, this would need adjustment
        PositionInBeats::from_beats(position.as_quarter_notes())
    }
}
