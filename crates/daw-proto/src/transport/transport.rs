//! Transport state and service
//!
//! This module defines the transport state representation and the TransportService trait
//! for controlling DAW playback, recording, and navigation.

use crate::ProjectContext;
use crate::primitives::{Position, Tempo, TimeSignature};
use crate::transport::error::TransportError;
use facet::Facet;
use vox::Tx;
use vox::service;

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

/// Loop region (start and end positions in seconds)
#[derive(Clone, Debug, Default, Facet, PartialEq)]
pub struct LoopRegion {
    /// Loop start position in seconds
    pub start_seconds: f64,
    /// Loop end position in seconds
    pub end_seconds: f64,
}

impl LoopRegion {
    /// Create a new loop region
    pub fn new(start_seconds: f64, end_seconds: f64) -> Self {
        Self {
            start_seconds,
            end_seconds,
        }
    }

    /// Get loop duration in seconds
    pub fn duration(&self) -> f64 {
        self.end_seconds - self.start_seconds
    }

    /// Check if the loop region is valid (end > start)
    pub fn is_valid(&self) -> bool {
        self.end_seconds > self.start_seconds
    }
}

/// Complete transport state
#[derive(Clone, Debug, Facet)]
pub struct Transport {
    pub play_state: PlayState,
    pub record_mode: RecordMode,
    pub looping: bool,
    /// Loop region (start/end positions). Only meaningful when looping is true.
    pub loop_region: Option<LoopRegion>,
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
            loop_region: None,
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
        self.tempo.bpm() * self.playrate
    }

    /// Set the tempo
    ///
    /// Note: Tempo validation happens at construction via `Tempo::from_bpm()` or `Tempo::try_from_bpm()`
    pub fn set_tempo(&mut self, tempo: Tempo) -> Result<(), TransportError> {
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
    async fn play(&self, project: ProjectContext);

    /// Pause playback (maintain position, can resume)
    async fn pause(&self, project: ProjectContext);

    /// Stop playback (reset to edit cursor or start)
    async fn stop(&self, project: ProjectContext);

    /// Toggle between play and pause
    async fn play_pause(&self, project: ProjectContext);

    /// Toggle between play and stop
    async fn play_stop(&self, project: ProjectContext);

    /// Start playback from the last position where playback was started.
    ///
    /// This is an FTS transport behavior that remembers the "play start point"
    /// even if the edit cursor moves later.
    async fn play_from_last_start_position(&self, project: ProjectContext);

    // =========================================================================
    // Recording Control (Priority 1)
    // =========================================================================

    /// Start recording
    async fn record(&self, project: ProjectContext);

    /// Stop recording (stops transport)
    async fn stop_recording(&self, project: ProjectContext);

    /// Toggle recording on/off
    async fn toggle_recording(&self, project: ProjectContext);

    // =========================================================================
    // Position Control (Priority 1)
    // =========================================================================

    /// Set playhead position in seconds
    async fn set_position(&self, project: ProjectContext, seconds: f64);

    /// Get current playhead position in seconds
    async fn get_position(&self, project: ProjectContext) -> f64;

    /// Go to the start of the project (position 0)
    async fn goto_start(&self, project: ProjectContext);

    /// Go to the end of the project
    async fn goto_end(&self, project: ProjectContext);

    // =========================================================================
    // State Queries (Priority 1)
    // =========================================================================

    /// Get complete transport state
    async fn get_state(&self, project: ProjectContext) -> Transport;

    /// Get current play state
    async fn get_play_state(&self, project: ProjectContext) -> PlayState;

    /// Check if currently playing (includes recording)
    async fn is_playing(&self, project: ProjectContext) -> bool;

    /// Check if currently recording
    async fn is_recording(&self, project: ProjectContext) -> bool;

    // =========================================================================
    // Tempo Control (Priority 2)
    // =========================================================================

    /// Get current tempo in BPM
    async fn get_tempo(&self, project: ProjectContext) -> f64;

    /// Set tempo in BPM
    async fn set_tempo(&self, project: ProjectContext, bpm: f64);

    // =========================================================================
    // Loop Control (Priority 2)
    // =========================================================================

    /// Toggle loop mode on/off
    async fn toggle_loop(&self, project: ProjectContext);

    /// Get loop enabled state
    async fn is_looping(&self, project: ProjectContext) -> bool;

    /// Set loop enabled state
    async fn set_loop(&self, project: ProjectContext, enabled: bool);

    // =========================================================================
    // Playrate Control (Priority 3)
    // =========================================================================

    /// Get current playback rate (1.0 = normal speed)
    async fn get_playrate(&self, project: ProjectContext) -> f64;

    /// Set playback rate (0.25 to 4.0, where 1.0 = normal speed)
    async fn set_playrate(&self, project: ProjectContext, rate: f64);

    // =========================================================================
    // Time Signature (Priority 3)
    // =========================================================================

    /// Get current time signature
    async fn get_time_signature(&self, project: ProjectContext) -> TimeSignature;

    // =========================================================================
    // Musical Position Control (Priority 2)
    // =========================================================================

    /// Set playhead position using musical position (measure, beat, subdivision)
    ///
    /// This converts the musical position to time using the project's tempo map
    /// and then sets the playhead position.
    async fn set_position_musical(
        &self,
        project: ProjectContext,
        measure: i32,
        beat: i32,
        subdivision: i32,
    );

    /// Go to a specific measure (0-indexed)
    ///
    /// This is a convenience method that seeks to the start of the specified measure.
    /// Equivalent to `set_position_musical(project, measure, 0, 0)`.
    async fn goto_measure(&self, project: ProjectContext, measure: i32);

    // =========================================================================
    // Streaming (Priority 1)
    // =========================================================================

    /// Subscribe to transport state changes
    ///
    /// Streams transport state updates at high frequency (up to 60Hz) when playing.
    /// The stream sends the complete Transport state on each update.
    ///
    /// The stream continues until the sender is dropped or the connection closes.
    /// Updates are sent:
    /// - Immediately when play/pause/stop state changes
    /// - At regular intervals (e.g., 60Hz) during playback
    /// - When tempo, time signature, or loop state changes
    async fn subscribe_state(&self, project: ProjectContext, tx: Tx<Transport>);

    /// Subscribe to transport state changes for ALL open projects
    ///
    /// Streams transport state updates for every open project at ~30Hz.
    /// Each update contains the project GUID and its transport state.
    ///
    /// This is much more efficient than subscribing to each project individually
    /// because it uses a single broadcast channel that's already being polled
    /// on the main thread.
    ///
    /// Updates are only sent when a project's state actually changes (reactive).
    async fn subscribe_all_projects(&self, tx: Tx<AllProjectsTransport>);
}

/// Transport state update for all projects
///
/// Contains a list of (project_guid, transport_state) pairs for all projects
/// whose state changed since the last update.
#[derive(Clone, Debug, Facet)]
pub struct AllProjectsTransport {
    /// List of project transport updates
    pub projects: Vec<ProjectTransportState>,
}

/// Transport state for a specific project
#[derive(Clone, Debug, Facet)]
pub struct ProjectTransportState {
    /// Project GUID (hash of file path)
    pub project_guid: String,
    /// Transport state for this project
    pub transport: Transport,
}
