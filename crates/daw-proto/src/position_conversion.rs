//! Position conversion service
//!
//! Provides methods for converting between different position representations.
//! These conversions require project context because they depend on the tempo map.

use crate::ProjectContext;
use crate::primitives::{
    MeasureMode, PositionInBeats, PositionInQuarterNotes, PositionInSeconds,
    QuarterNotesToMeasureResult, TimeToBeatsResult, TimeToQuarterNotesResult,
};
use roam::service;

/// Service for converting between position types
///
/// Position conversions require project context because they depend on:
/// - The project's tempo map (which can change over time)
/// - Time signature changes throughout the project
///
/// Following reaper-rs design, conversions return rich result types that include
/// measure context and time signature information.
#[service]
pub trait PositionConversionService {
    /// Convert time position to beats
    ///
    /// Returns the beat position along with measure context and time signature.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Time position in seconds
    /// * `measure_mode` - How to handle measure boundaries
    async fn time_to_beats(
        &self,
        project: ProjectContext,
        position: PositionInSeconds,
        measure_mode: MeasureMode,
    ) -> TimeToBeatsResult;

    /// Convert beat position to time
    ///
    /// Returns the time position in seconds.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Position in beats
    /// * `measure_mode` - How to handle measure boundaries
    async fn beats_to_time(
        &self,
        project: ProjectContext,
        position: PositionInBeats,
        measure_mode: MeasureMode,
    ) -> PositionInSeconds;

    /// Convert time position to quarter notes
    ///
    /// Returns the quarter note position along with measure context.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Time position in seconds
    async fn time_to_quarter_notes(
        &self,
        project: ProjectContext,
        position: PositionInSeconds,
    ) -> TimeToQuarterNotesResult;

    /// Convert quarter notes to time position
    ///
    /// Returns the time position in seconds.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Position in quarter notes
    async fn quarter_notes_to_time(
        &self,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInSeconds;

    /// Convert quarter notes to measure information
    ///
    /// Returns measure index and the start/end positions of that measure.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Position in quarter notes
    async fn quarter_notes_to_measure(
        &self,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> QuarterNotesToMeasureResult;

    /// Convert beats to quarter notes
    ///
    /// In most cases, beats == quarter notes, but this can vary with time signature.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Position in beats
    async fn beats_to_quarter_notes(
        &self,
        project: ProjectContext,
        position: PositionInBeats,
    ) -> PositionInQuarterNotes;

    /// Convert quarter notes to beats
    ///
    /// In most cases, quarter notes == beats, but this can vary with time signature.
    ///
    /// # Arguments
    /// * `project` - Which project's tempo map to use
    /// * `position` - Position in quarter notes
    async fn quarter_notes_to_beats(
        &self,
        project: ProjectContext,
        position: PositionInQuarterNotes,
    ) -> PositionInBeats;
}
