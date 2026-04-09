//! Take types
//!
//! Takes are alternative recordings or sources within an item.
//! An item can have multiple takes, but only one active take at a time.

use crate::primitives::Duration;
use facet::Facet;

/// Reference to a take within an item
#[repr(C)]
#[derive(Clone, Debug, Facet)]
pub enum TakeRef {
    /// Reference by GUID
    Guid(String),
    /// Reference by index within item
    Index(u32),
    /// Reference the currently active take
    Active,
}

/// Complete take state
#[derive(Clone, Debug, Facet)]
pub struct Take {
    /// Unique identifier
    pub guid: String,
    /// Item this take belongs to
    pub item_guid: String,
    /// Index within the item
    pub index: u32,
    /// Whether this is the active take
    pub is_active: bool,

    // Metadata
    /// Display name
    pub name: String,
    /// Custom color (0xRRGGBB format)
    pub color: Option<u32>,

    // Playback
    /// Take volume (1.0 = 0dB)
    pub volume: f64,
    /// Playback speed (1.0 = normal, 0.5 = half speed, 2.0 = double)
    pub play_rate: f64,
    /// Pitch adjustment in semitones
    pub pitch: f64,
    /// Whether to preserve pitch when changing playback rate
    pub preserve_pitch: bool,
    /// Offset into the source (start position)
    pub start_offset: Duration,

    // Source info
    /// Type of source (audio, MIDI, etc.)
    pub source_type: SourceType,
    /// Absolute path to the source file on disk (None for MIDI or empty takes)
    pub source_file_path: Option<String>,
    /// Length of the source media
    pub source_length: Option<Duration>,
    /// Sample rate of the source
    pub source_sample_rate: Option<u32>,
    /// Number of channels in the source
    pub source_channels: Option<u32>,

    // MIDI-specific
    /// Whether this is a MIDI take
    pub is_midi: bool,
    /// Number of MIDI notes (if MIDI take)
    pub midi_note_count: Option<u32>,
}

/// Type of media source
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Facet)]
pub enum SourceType {
    /// Audio file (WAV, MP3, FLAC, etc.)
    #[default]
    Audio = 0,
    /// MIDI data
    Midi = 1,
    /// Empty/no source
    Empty = 2,
    /// Video file
    Video = 3,
    /// Unknown source type
    Unknown = 255,
}

impl Take {
    /// Check if this take has a valid source
    pub fn has_source(&self) -> bool {
        !matches!(self.source_type, SourceType::Empty | SourceType::Unknown)
    }

    /// Get the effective playback duration considering play rate
    pub fn effective_duration(&self) -> Option<Duration> {
        self.source_length.map(|len| {
            let effective_seconds = len.as_seconds() / self.play_rate;
            Duration::from_seconds(effective_seconds)
        })
    }
}

impl Default for Take {
    fn default() -> Self {
        Self {
            guid: String::new(),
            item_guid: String::new(),
            index: 0,
            is_active: false,
            name: String::new(),
            color: None,
            volume: 1.0,
            play_rate: 1.0,
            pitch: 0.0,
            preserve_pitch: true,
            start_offset: Duration::ZERO,
            source_type: SourceType::Empty,
            source_file_path: None,
            source_length: None,
            source_sample_rate: None,
            source_channels: None,
            is_midi: false,
            midi_note_count: None,
        }
    }
}
