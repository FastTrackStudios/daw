//! Primitive types for DAW Protocol
//!
//! This module defines the core position and duration types used throughout the DAW Protocol.
//!
//! The primitives module provides position and timing representations that are
//! fundamental to DAW operations. All position types support conversion between
//! musical, time, and MIDI representations.
//!
//! Position can be represented in multiple ways:
//! - **Musical** - Measure.beat.subdivision (musical time)
//! - **Time** - Minutes:seconds.milliseconds (absolute time)
//! - **MIDI** - PPQ ticks (MIDI resolution)
//! //!
//! The unified Position type provides lazy calculation between representations.

use facet::Facet;
use std::fmt;

/// Musical position representation (measure.beat.subdivision)
///
/// Measure number (0-indexed internally, displayed as 1-indexed)
///
/// Beat within the measure (0-indexed internally, displayed as 1-indexed)
///
/// Subdivision of the beat (0-999, representing 0.0-0.999 of a beat)
#[derive(PartialEq, Eq, Clone, Facet)]
pub struct MusicalPosition {
    pub measure: i32,
    pub beat: i32,
    pub subdivision: i32,
}

impl MusicalPosition {
    /// Create a new musical position with validation
    pub fn new(measure: i32, beat: i32, subdivision: i32) -> Self {
        assert!(
            (0..=999).contains(&subdivision),
            "Subdivision must be in range 0-999, got {}",
            subdivision
        );
        Self {
            measure,
            beat,
            subdivision,
        }
    }

    /// Try to create a musical position, returning an error if invalid
    pub fn try_new(measure: i32, beat: i32, subdivision: i32) -> Result<Self, String> {
        if !(0..=999).contains(&subdivision) {
            return Err(format!(
                "Subdivision must be in range 0-999, got {}",
                subdivision
            ));
        }
        Ok(Self {
            measure,
            beat,
            subdivision,
        })
    }

    /// Get the start position (measure 0, beat 0, subdivision 0)
    pub fn start() -> Self {
        Self {
            measure: 0,
            beat: 0,
            subdivision: 0,
        }
    }

    /// Convert musical position to time position using BPM and time signature
    pub fn to_time_position(&self, bpm: f64, time_signature: TimeSignature) -> TimePosition {
        let beats_per_measure = time_signature.numerator as f64;
        let total_beats = self.measure as f64 * beats_per_measure
            + self.beat as f64
            + self.subdivision as f64 / 1000.0;
        let total_seconds = total_beats * (60.0 / bpm);
        TimePosition::from_seconds(total_seconds)
    }

    /// Convert musical position to MIDI position using PPQ resolution
    pub fn to_midi_position(
        &self,
        ppq_resolution: f64,
        time_signature: TimeSignature,
    ) -> MidiPosition {
        let beats_per_measure = time_signature.numerator as f64;
        let total_beats = self.measure as f64 * beats_per_measure
            + self.beat as f64
            + self.subdivision as f64 / 1000.0;
        let total_ppq = (total_beats * ppq_resolution) as i64;
        MidiPosition::new(total_ppq)
    }
}

impl Default for MusicalPosition {
    fn default() -> Self {
        Self::start()
    }
}

impl fmt::Display for MusicalPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{:03}",
            self.measure + 1,
            self.beat + 1,
            self.subdivision
        )
    }
}

/// Time position representation (minutes:seconds.milliseconds)
///
/// Minutes component
///
/// Seconds component (0-59)
///
/// Milliseconds component (0-999)
#[derive(Clone, PartialEq, Eq, Hash, Facet)]
pub struct TimePosition {
    pub minutes: i32,
    pub seconds: i32,
    pub milliseconds: i32,
}

impl TimePosition {
    /// Create a new time position with validation
    pub fn new(minutes: i32, seconds: i32, milliseconds: i32) -> Self {
        assert!(
            (0..=59).contains(&seconds),
            "Seconds must be in range 0-59, got {}",
            seconds
        );
        assert!(
            (0..=999).contains(&milliseconds),
            "Milliseconds must be in range 0-999, got {}",
            milliseconds
        );
        Self {
            minutes,
            seconds,
            milliseconds,
        }
    }

    /// Try to create a time position, returning an error if invalid
    pub fn try_new(minutes: i32, seconds: i32, milliseconds: i32) -> Result<Self, String> {
        if !(0..=59).contains(&seconds) {
            return Err(format!("Seconds must be in range 0-59, got {}", seconds));
        }
        if !(0..=999).contains(&milliseconds) {
            return Err(format!(
                "Milliseconds must be in range 0-999, got {}",
                milliseconds
            ));
        }
        Ok(Self {
            minutes,
            seconds,
            milliseconds,
        })
    }

    /// Create a time position from total seconds (floating point)
    pub fn from_seconds(total_seconds: f64) -> Self {
        let total_ms = (total_seconds * 1000.0) as i64;
        let minutes = (total_ms / 60_000) as i32;
        let remaining_ms = total_ms % 60_000;
        let seconds = (remaining_ms / 1000) as i32;
        let milliseconds = (remaining_ms % 1000) as i32;
        Self {
            minutes,
            seconds,
            milliseconds,
        }
    }

    /// Convert time position to total seconds
    pub fn to_seconds(&self) -> f64 {
        self.minutes as f64 * 60.0 + self.seconds as f64 + self.milliseconds as f64 / 1000.0
    }

    /// Get the start position (0:00.000)
    pub fn start() -> Self {
        Self {
            minutes: 0,
            seconds: 0,
            milliseconds: 0,
        }
    }

    /// Convert time position to musical position using BPM and time signature
    pub fn to_musical_position(&self, bpm: f64, time_signature: TimeSignature) -> MusicalPosition {
        let total_seconds = self.to_seconds();
        let beats_per_measure = time_signature.numerator as f64;
        let total_beats = total_seconds * (bpm / 60.0);
        let measure = (total_beats / beats_per_measure).floor() as i32;
        let beats_in_measure = total_beats % beats_per_measure;
        let beat = beats_in_measure.floor() as i32;
        let subdivision = ((beats_in_measure - beat as f64) * 1000.0).round() as i32;
        MusicalPosition::try_new(measure, beat, subdivision.clamp(0, 999))
            .unwrap_or_else(|_| MusicalPosition::start())
    }

    /// Convert time position to MIDI position using BPM and PPQ resolution
    pub fn to_midi_position(&self, bpm: f64, ppq_resolution: f64) -> MidiPosition {
        let total_seconds = self.to_seconds();
        let total_beats = total_seconds * (bpm / 60.0);
        let total_ppq = (total_beats * ppq_resolution) as i64;
        MidiPosition::new(total_ppq)
    }
}

impl Default for TimePosition {
    fn default() -> Self {
        Self::start()
    }
}

impl fmt::Display for TimePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{:02}.{:03}",
            self.minutes, self.seconds, self.milliseconds
        )
    }
}

/// MIDI position representation (PPQ ticks)
///
/// Pulses Per Quarter note - the number of ticks since the start
#[derive(Clone, Copy, PartialEq, Eq, Hash, Facet)]
pub struct MidiPosition {
    pub ppq: i64,
}

impl MidiPosition {
    /// Create a new MIDI position with the given PPQ value
    pub fn new(ppq: i64) -> Self {
        Self { ppq }
    }

    /// Get the zero position (0 PPQ)
    pub fn zero() -> Self {
        Self { ppq: 0 }
    }

    /// Convert MIDI position to musical position using PPQ resolution and time signature
    pub fn to_musical_position(
        &self,
        ppq_resolution: f64,
        time_signature: TimeSignature,
    ) -> MusicalPosition {
        let beats_per_measure = time_signature.numerator as f64;
        let total_beats = self.ppq as f64 / ppq_resolution;
        let measures = (total_beats / beats_per_measure).floor() as i32;
        let beats_in_measure = (total_beats % beats_per_measure).floor() as i32;
        let subdivision = ((total_beats % 1.0) * 1000.0).round() as i32;
        MusicalPosition::try_new(measures, beats_in_measure, subdivision.clamp(0, 999))
            .unwrap_or_else(|_| MusicalPosition::start())
    }

    /// Convert MIDI position to time position using PPQ resolution and BPM
    pub fn to_time_position(&self, ppq_resolution: f64, bpm: f64) -> TimePosition {
        let total_beats = self.ppq as f64 / ppq_resolution;
        let total_seconds = total_beats * (60.0 / bpm);
        TimePosition::from_seconds(total_seconds)
    }
}

impl fmt::Display for MidiPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} PPQ", self.ppq)
    }
}

/// Unified position type with lazy calculation
///
/// Stores available position representations and calculates missing ones on demand.
/// This allows efficient position tracking without constant conversions.
///
/// Musical position representation (measure.beat.subdivision)
///
/// Time position representation (minutes:seconds.milliseconds)
///
/// MIDI position representation (PPQ ticks)
#[derive(Clone, Facet)]
pub struct Position {
    pub musical: Option<MusicalPosition>,
    pub time: Option<TimePosition>,
    pub midi: Option<MidiPosition>,
}

impl Position {
    /// Create a position from optional representations
    pub fn new(
        musical: Option<MusicalPosition>,
        time: Option<TimePosition>,
        midi: Option<MidiPosition>,
    ) -> Self {
        Self {
            musical,
            time,
            midi,
        }
    }

    /// Create a position from a musical position
    pub fn from_musical(musical: MusicalPosition) -> Self {
        Self {
            musical: Some(musical),
            time: None,
            midi: None,
        }
    }

    /// Create a position from a time position
    pub fn from_time(time: TimePosition) -> Self {
        Self {
            musical: None,
            time: Some(time),
            midi: None,
        }
    }

    /// Create a position from a MIDI position
    pub fn from_midi(midi: MidiPosition) -> Self {
        Self {
            musical: None,
            time: None,
            midi: Some(midi),
        }
    }

    /// Get the start position with all representations initialized
    pub fn start() -> Self {
        Self {
            musical: Some(MusicalPosition::start()),
            time: Some(TimePosition::start()),
            midi: Some(MidiPosition::zero()),
        }
    }

    /// Check if musical position is available
    pub fn has_musical(&self) -> bool {
        self.musical.is_some()
    }

    /// Check if time position is available
    pub fn has_time(&self) -> bool {
        self.time.is_some()
    }

    /// Check if MIDI position is available
    pub fn has_midi(&self) -> bool {
        self.midi.is_some()
    }

    /// Get musical position - uses existing or calculates from available data
    pub fn musical(
        &mut self,
        bpm: f64,
        time_signature: TimeSignature,
        ppq_resolution: f64,
    ) -> MusicalPosition {
        if let Some(ref pos) = self.musical {
            return pos.clone();
        }

        let calculated = if let Some(ref time) = self.time {
            time.to_musical_position(bpm, time_signature)
        } else if let Some(ref midi) = self.midi {
            midi.to_musical_position(ppq_resolution, time_signature)
        } else {
            MusicalPosition::start()
        };

        self.musical = Some(calculated.clone());
        calculated
    }

    /// Get time position - uses existing or calculates from available data
    pub fn time(
        &mut self,
        bpm: f64,
        time_signature: TimeSignature,
        ppq_resolution: f64,
    ) -> TimePosition {
        if let Some(ref pos) = self.time {
            return pos.clone();
        }

        let calculated = if let Some(ref musical) = self.musical {
            musical.to_time_position(bpm, time_signature)
        } else if let Some(ref midi) = self.midi {
            midi.to_time_position(ppq_resolution, bpm)
        } else {
            TimePosition::start()
        };

        self.time = Some(calculated.clone());
        calculated
    }

    /// Get MIDI position - uses existing or calculates from available data
    pub fn midi(
        &mut self,
        bpm: f64,
        time_signature: TimeSignature,
        ppq_resolution: f64,
    ) -> MidiPosition {
        if let Some(ref pos) = self.midi {
            return pos.clone();
        }

        let calculated = if let Some(ref musical) = self.musical {
            musical.to_midi_position(ppq_resolution, time_signature)
        } else if let Some(ref time) = self.time {
            time.to_midi_position(bpm, ppq_resolution)
        } else {
            MidiPosition::zero()
        };

        self.midi = Some(calculated.clone());
        calculated
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::start()
    }
}

/// Time signature representation (e.g., 4/4, 3/4, 6/8)
///
/// Beats per measure (top number)
///
/// Beat unit (bottom number) - typically 4 (quarter note) or 8 (eighth note)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Facet)]
pub struct TimeSignature {
    pub numerator: i32,
    pub denominator: i32,
}

impl TimeSignature {
    /// Create a new time signature
    pub fn new(numerator: i32, denominator: i32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    /// Get the common time signature (4/4)
    pub fn common_time() -> Self {
        Self::new(4, 4)
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self::common_time()
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

/// Tempo/BPM representation
///
/// Beats per minute (typically 20.0 - 999.0)
#[derive(Clone, Copy, PartialEq, Facet)]
pub struct Tempo {
    pub bpm: f64,
}

impl Tempo {
    /// Create a new tempo with the given BPM
    pub fn new(bpm: f64) -> Self {
        Self { bpm }
    }

    /// Check if the tempo is within valid range (0 < bpm <= 999)
    pub fn is_valid(&self) -> bool {
        self.bpm > 0.0 && self.bpm <= 999.0
    }
}

impl Default for Tempo {
    fn default() -> Self {
        Self { bpm: 120.0 }
    }
}

impl fmt::Display for Tempo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} BPM", self.bpm)
    }
}
