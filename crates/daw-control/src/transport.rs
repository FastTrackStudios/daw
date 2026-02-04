//! Transport handle and operations

use std::sync::Arc;

use crate::DawClients;
use daw_proto::{PlayState, ProjectContext, TimeSignature, Transport as TransportState};
use eyre::Result;
use roam::Rx;

/// Transport handle for a specific project
///
/// This handle provides access to transport control (play, stop, record, etc.)
/// for a specific project. Like reaper-rs, it's lightweight and cheap to clone.
///
/// All methods return `Result` so callers can use `?` for clean error propagation.
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
/// // All methods use ? for error propagation
/// transport.play().await?;
/// transport.pause().await?;
/// transport.stop().await?;
/// transport.set_position(10.5).await?;
///
/// let pos = transport.get_position().await?;
/// let bpm = transport.get_tempo().await?;
/// transport.set_tempo(140.0).await?;
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

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    // =========================================================================
    // Playback Control
    // =========================================================================

    /// Play this project's transport
    ///
    /// Starts playback from the current playhead position.
    pub async fn play(&self) -> Result<()> {
        self.clients.transport.play(self.context()).await?;
        Ok(())
    }

    /// Pause playback
    ///
    /// Maintains the playhead position so playback can be resumed.
    pub async fn pause(&self) -> Result<()> {
        self.clients.transport.pause(self.context()).await?;
        Ok(())
    }

    /// Stop playback
    ///
    /// Stops playback and typically resets to the edit cursor or start position.
    pub async fn stop(&self) -> Result<()> {
        self.clients.transport.stop(self.context()).await?;
        Ok(())
    }

    /// Toggle between play and pause
    pub async fn play_pause(&self) -> Result<()> {
        self.clients.transport.play_pause(self.context()).await?;
        Ok(())
    }

    /// Toggle between play and stop
    pub async fn play_stop(&self) -> Result<()> {
        self.clients.transport.play_stop(self.context()).await?;
        Ok(())
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    /// Start recording
    pub async fn record(&self) -> Result<()> {
        self.clients.transport.record(self.context()).await?;
        Ok(())
    }

    /// Stop recording (also stops transport)
    pub async fn stop_recording(&self) -> Result<()> {
        self.clients
            .transport
            .stop_recording(self.context())
            .await?;
        Ok(())
    }

    /// Toggle recording on/off
    pub async fn toggle_recording(&self) -> Result<()> {
        self.clients
            .transport
            .toggle_recording(self.context())
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
            .set_position(self.context(), seconds)
            .await?;
        Ok(())
    }

    /// Get current playhead position in seconds
    pub async fn get_position(&self) -> Result<f64> {
        let pos = self.clients.transport.get_position(self.context()).await?;
        Ok(pos)
    }

    /// Go to the start of the project (position 0)
    pub async fn goto_start(&self) -> Result<()> {
        self.clients.transport.goto_start(self.context()).await?;
        Ok(())
    }

    /// Go to the end of the project
    pub async fn goto_end(&self) -> Result<()> {
        self.clients.transport.goto_end(self.context()).await?;
        Ok(())
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    /// Get complete transport state
    pub async fn get_state(&self) -> Result<TransportState> {
        let state = self.clients.transport.get_state(self.context()).await?;
        Ok(state)
    }

    /// Get current play state
    pub async fn get_play_state(&self) -> Result<PlayState> {
        let state = self
            .clients
            .transport
            .get_play_state(self.context())
            .await?;
        Ok(state)
    }

    /// Check if currently playing (includes recording)
    pub async fn is_playing(&self) -> Result<bool> {
        let playing = self.clients.transport.is_playing(self.context()).await?;
        Ok(playing)
    }

    /// Check if currently recording
    pub async fn is_recording(&self) -> Result<bool> {
        let recording = self.clients.transport.is_recording(self.context()).await?;
        Ok(recording)
    }

    // =========================================================================
    // Tempo Control
    // =========================================================================

    /// Get current tempo in BPM
    pub async fn get_tempo(&self) -> Result<f64> {
        let tempo = self.clients.transport.get_tempo(self.context()).await?;
        Ok(tempo)
    }

    /// Set tempo in BPM
    pub async fn set_tempo(&self, bpm: f64) -> Result<()> {
        self.clients
            .transport
            .set_tempo(self.context(), bpm)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Loop Control
    // =========================================================================

    /// Toggle loop mode on/off
    pub async fn toggle_loop(&self) -> Result<()> {
        self.clients.transport.toggle_loop(self.context()).await?;
        Ok(())
    }

    /// Get loop enabled state
    pub async fn is_looping(&self) -> Result<bool> {
        let looping = self.clients.transport.is_looping(self.context()).await?;
        Ok(looping)
    }

    /// Set loop enabled state
    pub async fn set_loop(&self, enabled: bool) -> Result<()> {
        self.clients
            .transport
            .set_loop(self.context(), enabled)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    /// Get current playback rate (1.0 = normal speed)
    pub async fn get_playrate(&self) -> Result<f64> {
        let rate = self.clients.transport.get_playrate(self.context()).await?;
        Ok(rate)
    }

    /// Set playback rate (0.25 to 4.0, where 1.0 = normal speed)
    pub async fn set_playrate(&self, rate: f64) -> Result<()> {
        self.clients
            .transport
            .set_playrate(self.context(), rate)
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
            .get_time_signature(self.context())
            .await?;
        Ok(ts)
    }

    // =========================================================================
    // Musical Position Control
    // =========================================================================

    /// Set playhead position using musical position (measure, beat, subdivision)
    ///
    /// The musical position is converted to time using the project's tempo map.
    /// - measure: 0-indexed measure number
    /// - beat: 0-indexed beat within the measure
    /// - subdivision: 0-999 representing fractional beat (0.000 to 0.999)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example(transport: daw_control::Transport) -> eyre::Result<()> {
    /// // Go to measure 5, beat 1 (0-indexed: measure 4, beat 0)
    /// transport.set_position_musical(4, 0, 0).await?;
    ///
    /// // Go to measure 1, beat 3, half-beat subdivision
    /// transport.set_position_musical(0, 2, 500).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_position_musical(
        &self,
        measure: i32,
        beat: i32,
        subdivision: i32,
    ) -> Result<()> {
        self.clients
            .transport
            .set_position_musical(self.context(), measure, beat, subdivision)
            .await?;
        Ok(())
    }

    /// Go to a specific measure
    ///
    /// Seeks to the start of the specified measure (0-indexed).
    /// This uses the project's tempo map to convert measure number to time.
    ///
    /// # Arguments
    ///
    /// * `measure` - The measure number to seek to (0-indexed, so measure 0 is the first measure)
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
    /// // Go to measure 8 (9th measure, since 0-indexed)
    /// transport.goto_measure(8).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn goto_measure(&self, measure: i32) -> Result<()> {
        self.clients
            .transport
            .goto_measure(self.context(), measure)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to transport state changes at 60Hz
    ///
    /// Returns a receiver that streams transport state updates at high frequency.
    /// Updates are sent:
    /// - Immediately when play/pause/stop state changes
    /// - At ~60Hz intervals during playback
    /// - When tempo, time signature, or loop state changes
    ///
    /// The stream continues until the returned `Rx` is dropped.
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
    /// // Subscribe to transport state updates
    /// let mut rx = transport.subscribe_state().await?;
    ///
    /// // Receive updates
    /// while let Some(state) = rx.recv().await? {
    ///     println!("Position: {:?}", state.playhead_position);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe_state(&self) -> Result<Rx<TransportState>> {
        // Create a channel pair
        let (tx, rx) = roam::channel::<TransportState>();

        // Call the service method to start the stream
        self.clients
            .transport
            .subscribe_state(self.context(), tx)
            .await?;

        Ok(rx)
    }

    /// Subscribe to transport state changes for ALL open projects at ~30Hz
    ///
    /// Returns a receiver that streams transport state updates for every open project.
    /// This is much more efficient than subscribing to each project individually.
    ///
    /// Updates are only sent when a project's state actually changes (reactive).
    ///
    /// The stream continues until the returned `Rx` is dropped.
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
    /// // Subscribe to all projects' transport state updates
    /// let mut rx = transport.subscribe_all_projects().await?;
    ///
    /// // Receive updates
    /// while let Some(update) = rx.recv().await? {
    ///     for proj in update.projects {
    ///         println!("Project {}: {:?}", proj.project_guid, proj.transport.play_state);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe_all_projects(&self) -> Result<Rx<daw_proto::AllProjectsTransport>> {
        // Create a channel pair
        let (tx, rx) = roam::channel::<daw_proto::AllProjectsTransport>();

        // Call the service method to start the stream
        self.clients.transport.subscribe_all_projects(tx).await?;

        Ok(rx)
    }
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("project_id", &self.project_id)
            .finish()
    }
}
