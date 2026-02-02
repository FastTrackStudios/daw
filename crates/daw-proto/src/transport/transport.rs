//! Transport state and service
//!
//! This module defines the transport state representation and the TransportService trait
//! for controlling DAW playback, recording, and navigation.

use crate::primitives::{Position, Tempo, TimeSignature};
use crate::transport::error::TransportError;
use facet::Facet;
use roam::service;

/// Current playback state
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Facet, Debug)]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
    Recording,
}

/// Recording mode
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Facet, Debug)]
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

    /// Check if transport is paused
    pub fn is_paused(&self) -> bool {
        matches!(self.play_state, PlayState::Paused)
    }

    /// Check if transport is stopped
    pub fn is_stopped(&self) -> bool {
        matches!(self.play_state, PlayState::Stopped)
    }

    /// Get effective BPM (tempo * playrate)
    pub fn effective_bpm(&self) -> f64 {
        self.tempo.bpm * self.playrate
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

// ============================================================================
// Transport Service
// ============================================================================

/// Transport service for controlling playback, recording, and navigation
///
/// This service provides comprehensive control over the DAW's transport system,
/// including playback, recording, position control, tempo, and loop settings.
#[service]
pub trait TransportService {
    // =========================================================================
    // Playback Control (Priority 1)
    // =========================================================================

    /// Start playback from current position
    async fn play(&self, project_id: Option<String>);

    /// Pause playback (maintain position, can resume)
    async fn pause(&self, project_id: Option<String>);

    /// Stop playback (reset to edit cursor or start)
    async fn stop(&self, project_id: Option<String>);

    /// Toggle between play and pause
    async fn play_pause(&self, project_id: Option<String>);

    /// Toggle between play and stop
    async fn play_stop(&self, project_id: Option<String>);

    // =========================================================================
    // Recording Control (Priority 1)
    // =========================================================================

    /// Start recording
    async fn record(&self, project_id: Option<String>);

    /// Stop recording (stops transport)
    async fn stop_recording(&self, project_id: Option<String>);

    /// Toggle recording on/off
    async fn toggle_recording(&self, project_id: Option<String>);

    // =========================================================================
    // Position Control (Priority 1)
    // =========================================================================

    /// Set playhead position in seconds
    async fn set_position(&self, project_id: Option<String>, seconds: f64);

    /// Get current playhead position in seconds
    async fn get_position(&self, project_id: Option<String>) -> f64;

    /// Go to the start of the project (position 0)
    async fn goto_start(&self, project_id: Option<String>);

    /// Go to the end of the project
    async fn goto_end(&self, project_id: Option<String>);

    // =========================================================================
    // State Queries (Priority 1)
    // =========================================================================

    /// Get complete transport state
    async fn get_state(&self, project_id: Option<String>) -> Transport;

    /// Get current play state
    async fn get_play_state(&self, project_id: Option<String>) -> PlayState;

    /// Check if currently playing (includes recording)
    async fn is_playing(&self, project_id: Option<String>) -> bool;

    /// Check if currently recording
    async fn is_recording(&self, project_id: Option<String>) -> bool;

    // =========================================================================
    // Tempo Control (Priority 2)
    // =========================================================================

    /// Get current tempo in BPM
    async fn get_tempo(&self, project_id: Option<String>) -> f64;

    /// Set tempo in BPM
    async fn set_tempo(&self, project_id: Option<String>, bpm: f64);

    // =========================================================================
    // Loop Control (Priority 2)
    // =========================================================================

    /// Toggle loop mode on/off
    async fn toggle_loop(&self, project_id: Option<String>);

    /// Get loop enabled state
    async fn is_looping(&self, project_id: Option<String>) -> bool;

    /// Set loop enabled state
    async fn set_loop(&self, project_id: Option<String>, enabled: bool);

    // =========================================================================
    // Playrate Control (Priority 3)
    // =========================================================================

    /// Get current playback rate (1.0 = normal speed)
    async fn get_playrate(&self, project_id: Option<String>) -> f64;

    /// Set playback rate (0.25 to 4.0, where 1.0 = normal speed)
    async fn set_playrate(&self, project_id: Option<String>, rate: f64);

    // =========================================================================
    // Time Signature (Priority 3)
    // =========================================================================

    /// Get current time signature
    async fn get_time_signature(&self, project_id: Option<String>) -> TimeSignature;
}
