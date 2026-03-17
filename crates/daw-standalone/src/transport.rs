//! Standalone transport implementation
//!
//! Provides a simulated transport that advances position when playing.
//! Each project has its own independent transport state, allowing multiple
//! projects to play simultaneously with independent positions.

use daw_proto::{
    PlayState, ProjectContext, Transport, TransportService,
    primitives::{Position, PositionInSeconds, Tempo, TimeSignature},
};
use roam::Tx;
use std::collections::HashMap;
use crate::platform::RwLock;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;
use tracing::{debug, info, warn};

/// Runtime state wrapping Transport with playback timing for a single project
#[derive(Clone)]
struct RuntimeTransport {
    /// The canonical transport state from daw-proto
    transport: Transport,
    /// When playback started (None if not playing) - used to compute elapsed time
    playback_start_instant: Option<Instant>,
    /// Position in seconds when playback started or was last seeked
    /// (needed because Transport.playhead_position may use different representations)
    base_position_seconds: f64,
    /// Last position where playback was started.
    last_play_start_seconds: Option<f64>,
}

impl Default for RuntimeTransport {
    fn default() -> Self {
        Self {
            transport: Transport::new(),
            playback_start_instant: None,
            base_position_seconds: 0.0,
            last_play_start_seconds: None,
        }
    }
}

impl RuntimeTransport {
    /// Get the current position in seconds, accounting for playback time
    fn current_position_seconds(&self) -> f64 {
        match self.playback_start_instant {
            Some(start_instant) => {
                let elapsed = start_instant.elapsed().as_secs_f64();
                self.base_position_seconds + (elapsed * self.transport.playrate)
            }
            None => self.base_position_seconds,
        }
    }

    /// Start playback from current position
    fn start_playback(&mut self) {
        self.base_position_seconds = self.current_position_seconds();
        self.last_play_start_seconds = Some(self.base_position_seconds);
        self.playback_start_instant = Some(Instant::now());
        self.transport.play_state = PlayState::Playing;
    }

    /// Start playback from the remembered last start point.
    fn start_playback_from_last_start(&mut self) {
        if let Some(last_start) = self.last_play_start_seconds {
            self.seek_to(last_start);
        }
        self.start_playback();
    }

    /// Pause playback, preserving position
    fn pause_playback(&mut self) {
        self.base_position_seconds = self.current_position_seconds();
        self.playback_start_instant = None;
        self.transport.play_state = PlayState::Paused;
    }

    /// Stop playback
    fn stop_playback(&mut self) {
        self.base_position_seconds = self.current_position_seconds();
        self.playback_start_instant = None;
        self.transport.play_state = PlayState::Stopped;
    }

    /// Seek to a specific position in seconds
    fn seek_to(&mut self, seconds: f64) {
        let was_playing = self.playback_start_instant.is_some();
        self.base_position_seconds = seconds;
        if was_playing {
            self.playback_start_instant = Some(Instant::now());
        }
        self.update_playhead_position();
    }

    /// Seek to a specific measure using current tempo and time signature
    fn seek_to_measure(&mut self, measure: i32) {
        let beats_per_measure = self.transport.time_signature.numerator() as f64;
        let total_beats = measure as f64 * beats_per_measure;
        let seconds_per_beat = 60.0 / self.transport.tempo.bpm();
        let seconds = total_beats * seconds_per_beat;
        self.seek_to(seconds);
    }

    /// Update the playhead_position field in Transport to match current seconds
    fn update_playhead_position(&mut self) {
        let seconds = self.current_position_seconds();
        self.transport.playhead_position =
            Position::from_time(PositionInSeconds::from_seconds(seconds));
    }

    /// Get a Transport snapshot with current position
    fn snapshot(&self) -> Transport {
        let mut transport = self.transport.clone();
        let seconds = self.current_position_seconds();
        transport.playhead_position = Position::from_time(PositionInSeconds::from_seconds(seconds));
        transport.edit_position = Position::from_time(PositionInSeconds::from_seconds(seconds));
        transport
    }
}

/// Shared state between StandaloneProject and StandaloneTransport
///
/// This allows the transport service to resolve `ProjectContext::Current`
/// to the actual current project GUID.
#[derive(Clone)]
pub struct SharedProjectState {
    /// GUIDs of all available projects
    pub project_guids: Arc<Vec<String>>,
    /// Index of the currently selected project
    pub current_index: Arc<RwLock<usize>>,
}

impl SharedProjectState {
    /// Create a new shared project state
    pub fn new(project_guids: Vec<String>) -> Self {
        Self {
            project_guids: Arc::new(project_guids),
            current_index: Arc::new(RwLock::new("shared-project-current-index", 0)),
        }
    }
}

impl SharedProjectState {
    /// Get the GUID of the current project
    pub async fn current_guid(&self) -> Option<String> {
        let index = *self.current_index.read().await;
        self.project_guids.get(index).cloned()
    }
}

/// Standalone DAW transport implementation.
///
/// This is a simulated transport that tracks play/stop state and advances
/// position in real-time when playing. Each project has its own independent
/// transport state, allowing multiple projects to play simultaneously.
///
/// When playing, the position advances in real-time based on the playrate.
#[derive(Clone)]
pub struct StandaloneTransport {
    /// Per-project transport state, keyed by project GUID
    states: Arc<RwLock<HashMap<String, RuntimeTransport>>>,
    /// Shared project state for resolving ProjectContext::Current
    project_state: SharedProjectState,
}

impl StandaloneTransport {
    /// Create a new transport with shared project state
    pub fn new(project_state: SharedProjectState) -> Self {
        Self {
            states: Arc::new(RwLock::new("standalone-transport-states", HashMap::new())),
            project_state,
        }
    }

    /// Resolve a ProjectContext to an actual project GUID
    async fn resolve_project(&self, context: &ProjectContext) -> Option<String> {
        match context {
            ProjectContext::Current => self.project_state.current_guid().await,
            ProjectContext::Project(guid) => Some(guid.clone()),
        }
    }

    /// Get or create transport state for a project
    async fn get_or_create_state(&self, guid: &str) -> RuntimeTransport {
        let states = self.states.read().await;
        if let Some(state) = states.get(guid) {
            return state.clone();
        }
        drop(states);

        // Create new state for this project
        let mut states = self.states.write().await;
        states
            .entry(guid.to_string())
            .or_insert_with(RuntimeTransport::default)
            .clone()
    }

    /// Execute a mutation on a project's transport state
    async fn with_state_mut<F, R>(&self, guid: &str, f: F) -> R
    where
        F: FnOnce(&mut RuntimeTransport) -> R,
    {
        let mut states = self.states.write().await;
        let state = states
            .entry(guid.to_string())
            .or_insert_with(RuntimeTransport::default);
        f(state)
    }

    /// Get the current play state for a project (for testing assertions)
    pub async fn get_play_state_for(&self, guid: &str) -> PlayState {
        self.get_or_create_state(guid).await.transport.play_state
    }

    /// Check if a project is currently playing (convenience method for tests)
    pub async fn is_playing_for(&self, guid: &str) -> bool {
        self.get_or_create_state(guid).await.transport.play_state == PlayState::Playing
    }
}

impl TransportService for StandaloneTransport {
    // =========================================================================
    // Playback Control
    // =========================================================================

    async fn play(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::play - could not resolve project");
            return;
        };
        info!("StandaloneTransport: play for project {}", guid);
        self.with_state_mut(&guid, |state| state.start_playback())
            .await;
    }

    async fn pause(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::pause - could not resolve project");
            return;
        };
        info!("StandaloneTransport: pause for project {}", guid);
        self.with_state_mut(&guid, |state| state.pause_playback())
            .await;
    }

    async fn stop(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::stop - could not resolve project");
            return;
        };
        info!("StandaloneTransport: stop for project {}", guid);
        self.with_state_mut(&guid, |state| state.stop_playback())
            .await;
    }

    async fn play_pause(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::play_pause - could not resolve project");
            return;
        };
        info!("StandaloneTransport: play_pause for project {}", guid);
        self.with_state_mut(&guid, |state| match state.transport.play_state {
            PlayState::Playing => state.pause_playback(),
            _ => state.start_playback(),
        })
        .await;
    }

    async fn play_stop(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::play_stop - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: play_stop for project {}", guid);
        self.with_state_mut(&guid, |state| match state.transport.play_state {
            PlayState::Playing => state.stop_playback(),
            _ => state.start_playback(),
        })
        .await;
    }

    async fn play_from_last_start_position(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::play_from_last_start_position - could not resolve project");
            return;
        };
        info!(
            "StandaloneTransport: play_from_last_start_position for project {}",
            guid
        );
        self.with_state_mut(&guid, |state| state.start_playback_from_last_start())
            .await;
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    async fn record(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::record - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: record for project {}", guid);
        self.with_state_mut(&guid, |state| {
            state.start_playback();
            state.transport.play_state = PlayState::Recording;
        })
        .await;
    }

    async fn stop_recording(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::stop_recording - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: stop_recording for project {}", guid);
        self.with_state_mut(&guid, |state| state.stop_playback())
            .await;
    }

    async fn toggle_recording(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::toggle_recording - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: toggle_recording for project {}", guid);
        self.with_state_mut(&guid, |state| match state.transport.play_state {
            PlayState::Recording => state.stop_playback(),
            _ => {
                state.start_playback();
                state.transport.play_state = PlayState::Recording;
            }
        })
        .await;
    }

    // =========================================================================
    // Position Control
    // =========================================================================

    async fn set_position(&self, project: ProjectContext, seconds: f64) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::set_position - could not resolve project");
            return;
        };
        info!(
            "StandaloneTransport: set_position to {:.2}s for project {}",
            seconds, guid
        );
        self.with_state_mut(&guid, |state| state.seek_to(seconds))
            .await;
    }

    async fn get_position(&self, project: ProjectContext) -> f64 {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::get_position - could not resolve project");
            return 0.0;
        };
        self.get_or_create_state(&guid)
            .await
            .current_position_seconds()
    }

    async fn goto_start(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::goto_start - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: goto_start for project {}", guid);
        self.with_state_mut(&guid, |state| state.seek_to(0.0)).await;
    }

    async fn goto_end(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::goto_end - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: goto_end for project {}", guid);
        // In a real implementation, this would go to project end
        self.with_state_mut(&guid, |state| state.seek_to(3600.0))
            .await;
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    async fn get_state(&self, project: ProjectContext) -> Transport {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::get_state - could not resolve project");
            return Transport::new();
        };
        self.get_or_create_state(&guid).await.snapshot()
    }

    async fn get_play_state(&self, project: ProjectContext) -> PlayState {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::get_play_state - could not resolve project");
            return PlayState::Stopped;
        };
        self.get_or_create_state(&guid).await.transport.play_state
    }

    async fn is_playing(&self, project: ProjectContext) -> bool {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::is_playing - could not resolve project");
            return false;
        };
        self.get_or_create_state(&guid).await.transport.is_playing()
    }

    async fn is_recording(&self, project: ProjectContext) -> bool {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::is_recording - could not resolve project");
            return false;
        };
        self.get_or_create_state(&guid)
            .await
            .transport
            .is_recording()
    }

    // =========================================================================
    // Tempo Control
    // =========================================================================

    async fn get_tempo(&self, project: ProjectContext) -> f64 {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::get_tempo - could not resolve project");
            return 120.0;
        };
        self.get_or_create_state(&guid).await.transport.tempo.bpm()
    }

    async fn set_tempo(&self, project: ProjectContext, bpm: f64) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::set_tempo - could not resolve project");
            return;
        };
        debug!(
            "StandaloneTransport: set_tempo to {} for project {}",
            bpm, guid
        );
        self.with_state_mut(&guid, |state| {
            state.transport.tempo = Tempo::from_bpm(bpm);
        })
        .await;
    }

    // =========================================================================
    // Loop Control
    // =========================================================================

    async fn toggle_loop(&self, project: ProjectContext) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::toggle_loop - could not resolve project");
            return;
        };
        debug!("StandaloneTransport: toggle_loop for project {}", guid);
        self.with_state_mut(&guid, |state| {
            state.transport.looping = !state.transport.looping;
        })
        .await;
    }

    async fn is_looping(&self, project: ProjectContext) -> bool {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::is_looping - could not resolve project");
            return false;
        };
        self.get_or_create_state(&guid).await.transport.looping
    }

    async fn set_loop(&self, project: ProjectContext, enabled: bool) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::set_loop - could not resolve project");
            return;
        };
        debug!(
            "StandaloneTransport: set_loop to {} for project {}",
            enabled, guid
        );
        self.with_state_mut(&guid, |state| {
            state.transport.looping = enabled;
        })
        .await;
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    async fn get_playrate(&self, project: ProjectContext) -> f64 {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::get_playrate - could not resolve project");
            return 1.0;
        };
        self.get_or_create_state(&guid).await.transport.playrate
    }

    async fn set_playrate(&self, project: ProjectContext, rate: f64) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::set_playrate - could not resolve project");
            return;
        };
        debug!(
            "StandaloneTransport: set_playrate to {} for project {}",
            rate, guid
        );
        self.with_state_mut(&guid, |state| {
            // Capture current position before changing playrate
            state.base_position_seconds = state.current_position_seconds();
            if state.playback_start_instant.is_some() {
                state.playback_start_instant = Some(Instant::now());
            }
            state.transport.playrate = rate.clamp(0.25, 4.0);
        })
        .await;
    }

    // =========================================================================
    // Time Signature
    // =========================================================================

    async fn get_time_signature(&self, project: ProjectContext) -> TimeSignature {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::get_time_signature - could not resolve project");
            return TimeSignature::default();
        };
        self.get_or_create_state(&guid)
            .await
            .transport
            .time_signature
    }

    // =========================================================================
    // Musical Position Control
    // =========================================================================

    async fn set_position_musical(
        &self,
        project: ProjectContext,
        measure: i32,
        beat: i32,
        subdivision: i32,
    ) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::set_position_musical - could not resolve project");
            return;
        };
        info!(
            "StandaloneTransport: set_position_musical to {}.{}.{} for project {}",
            measure, beat, subdivision, guid
        );
        self.with_state_mut(&guid, |state| {
            let beats_per_measure = state.transport.time_signature.numerator() as f64;
            let total_beats =
                measure as f64 * beats_per_measure + beat as f64 + subdivision as f64 / 1000.0;
            let seconds_per_beat = 60.0 / state.transport.tempo.bpm();
            let seconds = total_beats * seconds_per_beat;
            state.seek_to(seconds);
        })
        .await;
    }

    async fn goto_measure(&self, project: ProjectContext, measure: i32) {
        let Some(guid) = self.resolve_project(&project).await else {
            warn!("StandaloneTransport::goto_measure - could not resolve project");
            return;
        };
        info!(
            "StandaloneTransport: goto_measure {} for project {}",
            measure, guid
        );
        self.with_state_mut(&guid, |state| {
            let tempo = state.transport.tempo.bpm();
            let time_sig = state.transport.time_signature;
            info!(
                "StandaloneTransport: tempo={}, time_sig={}/{}",
                tempo,
                time_sig.numerator(),
                time_sig.denominator()
            );
            state.seek_to_measure(measure);
            info!(
                "StandaloneTransport: seeked to {:.2}s",
                state.current_position_seconds()
            );
        })
        .await;
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    async fn subscribe_state(&self, _project: ProjectContext, _tx: Tx<Transport>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
        let Some(guid) = self.resolve_project(&_project).await else {
            warn!("StandaloneTransport::subscribe_state - could not resolve project");
            return;
        };
        info!(
            "StandaloneTransport: subscribe_state for project {} - starting 60Hz stream",
            guid
        );

        // Clone states for the spawned task
        let states = self.states.clone();
        let guid = guid.clone();
        let tx = _tx;

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        moire::task::spawn(async move {
            // ~16ms for 60Hz
            let interval = Duration::from_micros(16667);
            let mut last_send = Instant::now();

            loop {
                // Sleep until next frame
                let elapsed = last_send.elapsed();
                if elapsed < interval {
                    crate::platform::sleep(interval - elapsed).await;
                }
                last_send = Instant::now();

                // Get current transport snapshot for this project
                let snapshot = {
                    let states = states.read().await;
                    states
                        .get(&guid)
                        .map(|s| s.snapshot())
                        .unwrap_or_else(Transport::new)
                };

                // Send the state - exit loop when client disconnects
                if let Err(e) = tx.send(snapshot).await {
                    debug!(
                        "StandaloneTransport: subscribe_state stream closed for project {}: {}",
                        guid, e
                    );
                    break;
                }
            }

            info!(
                "StandaloneTransport: subscribe_state stream ended for project {}",
                guid
            );
        });
    }

    async fn subscribe_all_projects(&self, tx: Tx<daw_proto::AllProjectsTransport>) {
        info!("StandaloneTransport: subscribe_all_projects - starting stream for all projects");

        // Clone states for the spawned task
        let states = self.states.clone();

        // Spawn the streaming loop so this method returns immediately
        moire::task::spawn(async move {
            // ~16ms for 60Hz batched updates
            let interval = Duration::from_millis(16);

            loop {
                crate::platform::sleep(interval).await;

                // Get transport snapshots for all projects
                let projects: Vec<daw_proto::ProjectTransportState> = {
                    let states = states.read().await;
                    states
                        .iter()
                        .map(|(guid, state)| daw_proto::ProjectTransportState {
                            project_guid: guid.clone(),
                            transport: state.snapshot(),
                        })
                        .collect()
                };

                if projects.is_empty() {
                    continue;
                }

                let update = daw_proto::AllProjectsTransport { projects };

                // Send the state - exit loop when client disconnects
                if let Err(e) = tx.send(update).await {
                    debug!(
                        "StandaloneTransport: subscribe_all_projects stream closed: {}",
                        e
                    );
                    break;
                }
            }

            info!("StandaloneTransport: subscribe_all_projects stream ended");
        });
    }
}
