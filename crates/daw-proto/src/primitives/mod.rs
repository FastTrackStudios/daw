//! Primitive types for DAW Protocol
//!
//! This module defines the core position, duration, tempo, and time signature types
//! used throughout the DAW Protocol.
//!
//! ## Position Types
//! Positions can be negative (for pre-roll, count-in, offsets):
//! - [`PositionInSeconds`] - Time in seconds
//! - [`PositionInBeats`] - Musical beats
//! - [`PositionInQuarterNotes`] - Quarter notes (REAPER's native time mapping)
//! - [`MusicalPosition`] - Measure.beat.subdivision
//! - [`PositionInPpq`] - MIDI PPQ ticks
//!
//! ## Duration Types
//! Durations are strictly non-negative (>= 0):
//! - [`Duration`] - Time span in seconds
//! - [`DurationInBeats`] - Time span in beats
//! - [`DurationInQuarterNotes`] - Time span in quarter notes
//!
//! ## Tempo & Time Signature
//! - [`Tempo`] - BPM (must be > 0)
//! - [`TimeSignature`] - Numerator/denominator (uses NonZeroU32 for safety)
//!
//! ## Enums
//! - [`TimeMode`] - Time display format
//! - [`BeatAttachMode`] - How items attach to timeline
//! - [`MeasureMode`] - Beat calculation behavior
//! - [`TimeModeOverride`] - Time mode override
//! - [`SendMidiTime`] - MIDI event timing
//!
//! ## Position Conversions
//! - [`TimeToBeatsResult`] - Result of time-to-beats conversion with measure context
//! - [`QuarterNotesToMeasureResult`] - Quarter notes to measure conversion
//! - [`TimeToQuarterNotesResult`] - Time to quarter notes with measure context

mod conversion;
mod duration;
mod enums;
mod position;
mod tempo;
mod time_signature;

// Re-export all types
pub use conversion::{QuarterNotesToMeasureResult, TimeToBeatsResult, TimeToQuarterNotesResult};
pub use duration::{Duration, DurationInBeats, DurationInQuarterNotes};
pub use enums::{
    AutomationMode, BeatAttachMode, MeasureMode, SendMidiTime, TimeMode, TimeModeOverride,
};
pub use position::{
    MusicalPosition, PositionInBeats, PositionInPpq, PositionInQuarterNotes, PositionInSeconds,
};
pub use tempo::Tempo;
pub use time_signature::TimeSignature;

// Backward compatibility aliases (deprecated - will be removed)
/// @deprecated Use `PositionInSeconds` instead
pub type TimePosition = PositionInSeconds;

/// @deprecated Use `PositionInPpq` instead
pub type MidiPosition = PositionInPpq;

/// Unified position type with multiple representations (legacy)
///
/// This type is kept for backward compatibility but may be removed in the future.
/// Prefer using specific position types directly.
#[derive(Debug, Clone, PartialEq, facet::Facet)]
pub struct Position {
    pub musical: Option<MusicalPosition>,
    pub time: Option<PositionInSeconds>,
    pub midi: Option<PositionInPpq>,
}

impl Position {
    /// Create from optional representations
    pub fn new(
        musical: Option<MusicalPosition>,
        time: Option<PositionInSeconds>,
        midi: Option<PositionInPpq>,
    ) -> Self {
        Self {
            musical,
            time,
            midi,
        }
    }

    /// Create from musical position
    pub fn from_musical(musical: MusicalPosition) -> Self {
        Self {
            musical: Some(musical),
            time: None,
            midi: None,
        }
    }

    /// Create from time position
    pub fn from_time(time: PositionInSeconds) -> Self {
        Self {
            musical: None,
            time: Some(time),
            midi: None,
        }
    }

    /// Create from MIDI position
    pub fn from_midi(midi: PositionInPpq) -> Self {
        Self {
            musical: None,
            time: None,
            midi: Some(midi),
        }
    }

    /// Zero/start position
    pub fn start() -> Self {
        Self {
            musical: Some(MusicalPosition::ZERO),
            time: Some(PositionInSeconds::ZERO),
            midi: Some(PositionInPpq::ZERO),
        }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::start()
    }
}

/// Time range with start and end positions
#[derive(Debug, Clone, PartialEq, facet::Facet)]
pub struct TimeRange {
    pub start: Position,
    pub end: Position,
}

impl TimeRange {
    /// Create a new time range
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// Create from seconds
    pub fn from_seconds(start: f64, end: f64) -> Self {
        Self {
            start: Position::from_time(PositionInSeconds::from_seconds(start)),
            end: Position::from_time(PositionInSeconds::from_seconds(end)),
        }
    }

    /// Get start in seconds
    pub fn start_seconds(&self) -> f64 {
        self.start.time.map(|t| t.as_seconds()).unwrap_or(0.0)
    }

    /// Get end in seconds
    pub fn end_seconds(&self) -> f64 {
        self.end.time.map(|t| t.as_seconds()).unwrap_or(0.0)
    }

    /// Get duration in seconds
    pub fn duration_seconds(&self) -> f64 {
        (self.end_seconds() - self.start_seconds()).max(0.0)
    }

    /// Check if this range contains the given time in seconds
    pub fn contains(&self, seconds: f64) -> bool {
        let start = self.start_seconds();
        let end = self.end_seconds();
        seconds >= start && seconds <= end
    }

    /// Check if this range overlaps with another range
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        let self_start = self.start_seconds();
        let self_end = self.end_seconds();
        let other_start = other.start_seconds();
        let other_end = other.end_seconds();

        // Ranges overlap if one starts before the other ends
        self_start <= other_end && other_start <= self_end
    }
}

impl Default for TimeRange {
    fn default() -> Self {
        Self {
            start: Position::start(),
            end: Position::start(),
        }
    }
}
