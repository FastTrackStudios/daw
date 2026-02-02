//! Transport handle and operations

use std::sync::Arc;

use crate::DawClients;
use daw_proto::{PlayState, TimeSignature, Transport as TransportState};
use eyre::Result;

/// Transport handle for a specific project
///
/// This handle provides access to transport control (play, stop, record, etc.)
/// for a specific project. Like reaper-rs, it's lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let transport = project.transport();
///
/// // Playback control
/// transport.play().await?;
/// transport.pause().await?;
/// transport.stop().await?;
///
/// // Recording
/// transport.record().await?;
/// transport.toggle_recording().await?;
///
/// // Position control
/// transport.set_position(10.5).await?;  // Jump to 10.5 seconds
/// let pos = transport.get_position().await?;
///
/// // Tempo control
/// let bpm = transport.get_tempo().await?;
/// transport.set_tempo(140.0).await?;
///
/// // Loop control
/// transport.toggle_loop().await?;
/// let looping = transport.is_looping().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Transport {
    project_id: String,
    clients: Arc<DawClients>,
}

impl Transport {
    /// Create a new transport handle for a project
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    // =========================================================================
    // Playback Control
    // =========================================================================

    /// Play this project's transport
    ///
    /// Starts playback from the current playhead position.
    pub async fn play(&self) -> Result<()> {
        self.clients
            .transport
            .play(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Pause playback
    ///
    /// Maintains the playhead position so playback can be resumed.
    pub async fn pause(&self) -> Result<()> {
        self.clients
            .transport
            .pause(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Stop playback
    ///
    /// Stops playback and typically resets to the edit cursor or start position.
    pub async fn stop(&self) -> Result<()> {
        self.clients
            .transport
            .stop(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Toggle between play and pause
    pub async fn play_pause(&self) -> Result<()> {
        self.clients
            .transport
            .play_pause(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Toggle between play and stop
    pub async fn play_stop(&self) -> Result<()> {
        self.clients
            .transport
            .play_stop(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    /// Start recording
    pub async fn record(&self) -> Result<()> {
        self.clients
            .transport
            .record(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Stop recording (also stops transport)
    pub async fn stop_recording(&self) -> Result<()> {
        self.clients
            .transport
            .stop_recording(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Toggle recording on/off
    pub async fn toggle_recording(&self) -> Result<()> {
        self.clients
            .transport
            .toggle_recording(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    // =========================================================================
    // Position Control
    // =========================================================================

    /// Set playhead position in seconds
    pub async fn set_position(&self, seconds: f64) -> Result<()> {
        self.clients
            .transport
            .set_position(Some(self.project_id.clone()), seconds)
            .await?;
        Ok(())
    }

    /// Get current playhead position in seconds
    pub async fn get_position(&self) -> Result<f64> {
        let pos = self
            .clients
            .transport
            .get_position(Some(self.project_id.clone()))
            .await?;
        Ok(pos)
    }

    /// Go to the start of the project (position 0)
    pub async fn goto_start(&self) -> Result<()> {
        self.clients
            .transport
            .goto_start(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Go to the end of the project
    pub async fn goto_end(&self) -> Result<()> {
        self.clients
            .transport
            .goto_end(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    /// Get complete transport state
    pub async fn get_state(&self) -> Result<TransportState> {
        let state = self
            .clients
            .transport
            .get_state(Some(self.project_id.clone()))
            .await?;
        Ok(state)
    }

    /// Get current play state
    pub async fn get_play_state(&self) -> Result<PlayState> {
        let state = self
            .clients
            .transport
            .get_play_state(Some(self.project_id.clone()))
            .await?;
        Ok(state)
    }

    /// Check if currently playing (includes recording)
    pub async fn is_playing(&self) -> Result<bool> {
        let playing = self
            .clients
            .transport
            .is_playing(Some(self.project_id.clone()))
            .await?;
        Ok(playing)
    }

    /// Check if currently recording
    pub async fn is_recording(&self) -> Result<bool> {
        let recording = self
            .clients
            .transport
            .is_recording(Some(self.project_id.clone()))
            .await?;
        Ok(recording)
    }

    // =========================================================================
    // Tempo Control
    // =========================================================================

    /// Get current tempo in BPM
    pub async fn get_tempo(&self) -> Result<f64> {
        let tempo = self
            .clients
            .transport
            .get_tempo(Some(self.project_id.clone()))
            .await?;
        Ok(tempo)
    }

    /// Set tempo in BPM
    pub async fn set_tempo(&self, bpm: f64) -> Result<()> {
        self.clients
            .transport
            .set_tempo(Some(self.project_id.clone()), bpm)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Loop Control
    // =========================================================================

    /// Toggle loop mode on/off
    pub async fn toggle_loop(&self) -> Result<()> {
        self.clients
            .transport
            .toggle_loop(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Get loop enabled state
    pub async fn is_looping(&self) -> Result<bool> {
        let looping = self
            .clients
            .transport
            .is_looping(Some(self.project_id.clone()))
            .await?;
        Ok(looping)
    }

    /// Set loop enabled state
    pub async fn set_loop(&self, enabled: bool) -> Result<()> {
        self.clients
            .transport
            .set_loop(Some(self.project_id.clone()), enabled)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    /// Get current playback rate (1.0 = normal speed)
    pub async fn get_playrate(&self) -> Result<f64> {
        let rate = self
            .clients
            .transport
            .get_playrate(Some(self.project_id.clone()))
            .await?;
        Ok(rate)
    }

    /// Set playback rate (0.25 to 4.0, where 1.0 = normal speed)
    pub async fn set_playrate(&self, rate: f64) -> Result<()> {
        self.clients
            .transport
            .set_playrate(Some(self.project_id.clone()), rate)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Time Signature
    // =========================================================================

    /// Get current time signature
    pub async fn get_time_signature(&self) -> Result<TimeSignature> {
        let ts = self
            .clients
            .transport
            .get_time_signature(Some(self.project_id.clone()))
            .await?;
        Ok(ts)
    }
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("project_id", &self.project_id)
            .finish()
    }
}
