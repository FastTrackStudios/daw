//! Standalone transport implementation
//!
//! Provides a simulated transport that advances position when playing.
//! Uses the `Transport` struct from daw-proto as the canonical state,
//! with an additional `Instant` to track real-time playback.

use daw_proto::{
    PlayState, ProjectContext, RecordMode, Transport, TransportService,
    primitives::{Position, PositionInSeconds, Tempo, TimeSignature},
};
use roam::{Context, Tx};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Runtime state wrapping Transport with playback timing
struct RuntimeTransport {
    /// The canonical transport state from daw-proto
    transport: Transport,
    /// When playback started (None if not playing) - used to compute elapsed time
    playback_start_instant: Option<Instant>,
    /// Position in seconds when playback started or was last seeked
    /// (needed because Transport.playhead_position may use different representations)
    base_position_seconds: f64,
}

impl Default for RuntimeTransport {
    fn default() -> Self {
        Self {
            transport: Transport::new(),
            playback_start_instant: None,
            base_position_seconds: 0.0,
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
        self.playback_start_instant = Some(Instant::now());
        self.transport.play_state = PlayState::Playing;
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

/// Standalone DAW transport implementation.
///
/// This is a simulated transport that tracks play/stop state and advances
/// position in real-time when playing. It uses the `Transport` struct from
/// daw-proto as the canonical state representation.
///
/// When playing, the position advances in real-time based on the playrate.
/// This will eventually support actual audio playback.
#[derive(Clone)]
pub struct StandaloneTransport {
    state: Arc<RwLock<RuntimeTransport>>,
}

impl Default for StandaloneTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneTransport {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeTransport::default())),
        }
    }

    /// Get the current play state (for testing assertions)
    pub async fn get_play_state(&self) -> PlayState {
        self.state.read().await.transport.play_state
    }

    /// Check if currently playing (convenience method for tests)
    pub async fn currently_playing(&self) -> bool {
        self.state.read().await.transport.play_state == PlayState::Playing
    }
}

impl TransportService for StandaloneTransport {
    // =========================================================================
    // Playback Control
    // =========================================================================

    async fn play(&self, _cx: &Context, _project: ProjectContext) {
        info!("StandaloneTransport: play");
        self.state.write().await.start_playback();
    }

    async fn pause(&self, _cx: &Context, _project: ProjectContext) {
        info!("StandaloneTransport: pause");
        self.state.write().await.pause_playback();
    }

    async fn stop(&self, _cx: &Context, _project: ProjectContext) {
        info!("StandaloneTransport: stop");
        self.state.write().await.stop_playback();
    }

    async fn play_pause(&self, _cx: &Context, _project: ProjectContext) {
        info!("StandaloneTransport: play_pause");
        let mut state = self.state.write().await;
        match state.transport.play_state {
            PlayState::Playing => state.pause_playback(),
            _ => state.start_playback(),
        }
    }

    async fn play_stop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: play_stop");
        let mut state = self.state.write().await;
        match state.transport.play_state {
            PlayState::Playing => state.stop_playback(),
            _ => state.start_playback(),
        }
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    async fn record(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: record");
        let mut state = self.state.write().await;
        state.start_playback();
        state.transport.play_state = PlayState::Recording;
    }

    async fn stop_recording(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: stop_recording");
        self.state.write().await.stop_playback();
    }

    async fn toggle_recording(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: toggle_recording");
        let mut state = self.state.write().await;
        match state.transport.play_state {
            PlayState::Recording => state.stop_playback(),
            _ => {
                state.start_playback();
                state.transport.play_state = PlayState::Recording;
            }
        }
    }

    // =========================================================================
    // Position Control
    // =========================================================================

    async fn set_position(&self, _cx: &Context, _project: ProjectContext, seconds: f64) {
        info!("StandaloneTransport: set_position to {:.2}s", seconds);
        self.state.write().await.seek_to(seconds);
    }

    async fn get_position(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        self.state.read().await.current_position_seconds()
    }

    async fn goto_start(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: goto_start");
        self.state.write().await.seek_to(0.0);
    }

    async fn goto_end(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: goto_end");
        // In a real implementation, this would go to project end
        self.state.write().await.seek_to(3600.0);
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    async fn get_state(&self, _cx: &Context, _project: ProjectContext) -> Transport {
        self.state.read().await.snapshot()
    }

    async fn get_play_state(&self, _cx: &Context, _project: ProjectContext) -> PlayState {
        self.state.read().await.transport.play_state
    }

    async fn is_playing(&self, _cx: &Context, _project: ProjectContext) -> bool {
        self.state.read().await.transport.is_playing()
    }

    async fn is_recording(&self, _cx: &Context, _project: ProjectContext) -> bool {
        self.state.read().await.transport.is_recording()
    }

    // =========================================================================
    // Tempo Control
    // =========================================================================

    async fn get_tempo(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        self.state.read().await.transport.tempo.bpm()
    }

    async fn set_tempo(&self, _cx: &Context, _project: ProjectContext, bpm: f64) {
        debug!("StandaloneTransport: set_tempo to {}", bpm);
        self.state.write().await.transport.tempo = Tempo::from_bpm(bpm);
    }

    // =========================================================================
    // Loop Control
    // =========================================================================

    async fn toggle_loop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("StandaloneTransport: toggle_loop");
        let mut state = self.state.write().await;
        state.transport.looping = !state.transport.looping;
    }

    async fn is_looping(&self, _cx: &Context, _project: ProjectContext) -> bool {
        self.state.read().await.transport.looping
    }

    async fn set_loop(&self, _cx: &Context, _project: ProjectContext, enabled: bool) {
        debug!("StandaloneTransport: set_loop to {}", enabled);
        self.state.write().await.transport.looping = enabled;
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    async fn get_playrate(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        self.state.read().await.transport.playrate
    }

    async fn set_playrate(&self, _cx: &Context, _project: ProjectContext, rate: f64) {
        debug!("StandaloneTransport: set_playrate to {}", rate);
        let mut state = self.state.write().await;
        // Capture current position before changing playrate
        state.base_position_seconds = state.current_position_seconds();
        if state.playback_start_instant.is_some() {
            state.playback_start_instant = Some(Instant::now());
        }
        state.transport.playrate = rate.clamp(0.25, 4.0);
    }

    // =========================================================================
    // Time Signature
    // =========================================================================

    async fn get_time_signature(&self, _cx: &Context, _project: ProjectContext) -> TimeSignature {
        self.state.read().await.transport.time_signature
    }

    // =========================================================================
    // Musical Position Control
    // =========================================================================

    async fn set_position_musical(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        measure: i32,
        beat: i32,
        subdivision: i32,
    ) {
        info!(
            "StandaloneTransport: set_position_musical to {}.{}.{}",
            measure, beat, subdivision
        );
        let mut state = self.state.write().await;
        let beats_per_measure = state.transport.time_signature.numerator() as f64;
        let total_beats =
            measure as f64 * beats_per_measure + beat as f64 + subdivision as f64 / 1000.0;
        let seconds_per_beat = 60.0 / state.transport.tempo.bpm();
        let seconds = total_beats * seconds_per_beat;
        state.seek_to(seconds);
    }

    async fn goto_measure(&self, _cx: &Context, _project: ProjectContext, measure: i32) {
        info!("StandaloneTransport: goto_measure {}", measure);
        let mut state = self.state.write().await;
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
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    async fn subscribe_state(&self, _cx: &Context, _project: ProjectContext, tx: Tx<Transport>) {
        info!("StandaloneTransport: subscribe_state - starting 60Hz stream");

        // Clone state for the spawned task
        let state = self.state.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        tokio::spawn(async move {
            // ~16ms for 60Hz
            let interval = Duration::from_micros(16667);
            let mut last_send = Instant::now();

            loop {
                // Sleep until next frame
                let elapsed = last_send.elapsed();
                if elapsed < interval {
                    tokio::time::sleep(interval - elapsed).await;
                }
                last_send = Instant::now();

                // Get current transport snapshot
                let snapshot = state.read().await.snapshot();

                // Send the state - exit loop when client disconnects
                if let Err(e) = tx.send(&snapshot).await {
                    debug!("StandaloneTransport: subscribe_state stream closed: {}", e);
                    break;
                }
            }

            info!("StandaloneTransport: subscribe_state stream ended");
        });
    }
}
