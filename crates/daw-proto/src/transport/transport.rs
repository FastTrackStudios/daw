//! Transport state
//!
//! This module defines the transport state representation.

use crate::primitives::{Position, Tempo, TimeSignature};
use crate::transport::error::TransportError;
use facet::Facet;
use roam::service;

/// Current playback state
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Facet)]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
    Recording,
}

/// Recording mode
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Facet)]
pub enum RecordMode {
    #[default]
    Normal,
    TimeSelection,
    Item,
}

/// Complete transport state
#[derive(Clone, Facet)]
pub struct Transport {
    pub play_state: PlayState,
    pub record_mode: RecordMode,
    pub looping: bool,
    pub tempo: Tempo,
    pub playrate: f64,
    pub time_signature: TimeSignature,
    pub playhead_position: Position,
    pub edit_position: Position,
}

impl Transport {
    /// Create a new transport state with default values
    pub fn new() -> Self {
        Self {
            play_state: PlayState::default(),
            record_mode: RecordMode::default(),
            looping: false,
            tempo: Tempo::default(),
            playrate: 1.0,
            time_signature: TimeSignature::default(),
            playhead_position: Position::start(),
            edit_position: Position::start(),
        }
    }

    /// Check if transport is currently playing or recording
    pub fn is_playing(&self) -> bool {
        matches!(self.play_state, PlayState::Playing | PlayState::Recording)
    }

    /// Check if transport is currently recording
    pub fn is_recording(&self) -> bool {
        matches!(self.play_state, PlayState::Recording)
    }

    /// Check if transport is stopped
    pub fn is_stopped(&self) -> bool {
        matches!(self.play_state, PlayState::Stopped)
    }

    /// Set the tempo, validating it is within valid range
    pub fn set_tempo(&mut self, tempo: Tempo) -> Result<(), TransportError> {
        if !tempo.is_valid() {
            return Err(TransportError::InvalidTempo(format!("{} BPM", tempo.bpm)));
        }
        self.tempo = tempo;
        Ok(())
    }

    /// Reset transport to initial stopped state
    pub fn reset(&mut self) {
        self.play_state = PlayState::Stopped;
        self.playhead_position = Position::start();
        self.edit_position = Position::start();
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

/// Transport service for controlling playback
#[service]
pub trait TransportService {
    /// Start playback from current position
    async fn play(&self, project_id: Option<String>);

    /// Stop playback and maintain position
    async fn stop(&self, project_id: Option<String>);
}
