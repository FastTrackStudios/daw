//! REAPER Transport Implementation
//!
//! Implements TransportService by dispatching REAPER API calls to the main thread
//! using TaskSupport from reaper-high.
//!
//! # Main Thread Safety
//!
//! REAPER APIs can only be called from the main thread. This implementation uses
//! TaskSupport to:
//!
//! 1. **Fire-and-forget operations** (play, stop, etc.): Use `do_later_in_main_thread_asap()`
//! 2. **Query operations** (get_tempo, is_playing, etc.): Use `main_thread_future()` which
//!    returns a Future that resolves with the result
//!
//! # Architecture
//!
//! ```text
//! RPC Handler (async, any thread)
//!     ↓
//!     TransportService method
//!     ↓
//!     TaskSupport::main_thread_future() or do_later_in_main_thread_asap()
//!     ↓
//!     Closure queued to crossbeam channel
//!     ↓
//! Timer Callback (main thread, ~30Hz)
//!     ↓
//!     MainTaskMiddleware::run() executes closure
//!     ↓
//!     REAPER API called, result sent via oneshot channel
//! ```

use daw_proto::{PlayState, TimeSignature, Transport, TransportService};
use reaper_high::{PlayRate, Reaper, TaskSupport, Tempo as ReaperTempo};
use reaper_medium::{
    CommandId, PositionInSeconds, ProjectContext, SetEditCurPosOptions, UndoBehavior,
};
use roam::Context;
use std::sync::OnceLock;
use tracing::debug;

/// Global TaskSupport instance - set by the extension during initialization
static TASK_SUPPORT: OnceLock<&'static TaskSupport> = OnceLock::new();

/// Set the global TaskSupport reference.
/// Called by the extension during initialization.
pub fn set_task_support(task_support: &'static TaskSupport) {
    let _ = TASK_SUPPORT.set(task_support);
}

/// Get the global TaskSupport reference.
pub(crate) fn task_support() -> Option<&'static TaskSupport> {
    TASK_SUPPORT.get().copied()
}

/// REAPER transport implementation.
///
/// All methods dispatch to the main thread via TaskSupport.
#[derive(Clone)]
pub struct ReaperTransport;

impl ReaperTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportService for ReaperTransport {
    // =========================================================================
    // Playback Control
    // =========================================================================

    async fn play(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: play");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1007: Transport: Play
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1007),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn pause(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: pause");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1008: Transport: Pause
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1008),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn stop(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: stop");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1016: Transport: Stop
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1016),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn play_pause(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: play_pause");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40073: Transport: Play/pause
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40073),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn play_stop(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: play_stop");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40044: Transport: Play/stop
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40044),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    async fn record(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: record");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1013: Transport: Record
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1013),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn stop_recording(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: stop_recording");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // Stop is also stop recording
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1016),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn toggle_recording(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: toggle_recording");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1013: Transport: Record (toggles)
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1013),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    // =========================================================================
    // Position Control
    // =========================================================================

    async fn set_position(&self, _cx: &Context, _project_id: Option<String>, seconds: f64) {
        debug!("ReaperTransport: set_position to {} seconds", seconds);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                if let Ok(pos) = PositionInSeconds::new(seconds) {
                    reaper.current_project().set_edit_cursor_position(
                        pos,
                        SetEditCurPosOptions {
                            move_view: false,
                            seek_play: true,
                        },
                    );
                }
            });
        }
    }

    async fn get_position(&self, _cx: &Context, _project_id: Option<String>) -> f64 {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                reaper
                    .current_project()
                    .play_or_edit_cursor_position()
                    .map(|p| p.get())
                    .unwrap_or(0.0)
            })
            .await
            .unwrap_or(0.0)
        } else {
            0.0
        }
    }

    async fn goto_start(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: goto_start");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40042: Transport: Go to start of project
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40042),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn goto_end(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: goto_end");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40043: Transport: Go to end of project
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40043),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    async fn get_state(&self, _cx: &Context, _project_id: Option<String>) -> Transport {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let project = reaper.current_project();
                let medium = reaper.medium_reaper();

                let play_state = get_play_state_internal(medium);
                let looping = medium.get_set_repeat_ex_get(ProjectContext::CurrentProject);
                let tempo_bpm = project.tempo().bpm().get();
                let playrate = project.play_rate().playback_speed_factor().get();
                let pos_seconds = project
                    .play_or_edit_cursor_position()
                    .map(|p| p.get())
                    .unwrap_or(0.0);
                let edit_pos = project
                    .edit_cursor_position()
                    .map(|p| p.get())
                    .unwrap_or(0.0);
                let (ts_num, ts_denom) = get_time_signature_internal(medium);

                Transport {
                    play_state,
                    record_mode: daw_proto::RecordMode::Normal,
                    looping,
                    tempo: daw_proto::primitives::Tempo::new(tempo_bpm),
                    playrate,
                    time_signature: TimeSignature::new(ts_num, ts_denom),
                    playhead_position: daw_proto::primitives::Position::from_time(
                        daw_proto::primitives::TimePosition::from_seconds(pos_seconds),
                    ),
                    edit_position: daw_proto::primitives::Position::from_time(
                        daw_proto::primitives::TimePosition::from_seconds(edit_pos),
                    ),
                }
            })
            .await
            .unwrap_or_else(|_| Transport::new())
        } else {
            Transport::new()
        }
    }

    async fn get_play_state(&self, _cx: &Context, _project_id: Option<String>) -> PlayState {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                get_play_state_internal(reaper.medium_reaper())
            })
            .await
            .unwrap_or(PlayState::Stopped)
        } else {
            PlayState::Stopped
        }
    }

    async fn is_playing(&self, _cx: &Context, _project_id: Option<String>) -> bool {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let state = reaper
                    .medium_reaper()
                    .get_play_state_ex(ProjectContext::CurrentProject);
                state.is_playing || state.is_recording
            })
            .await
            .unwrap_or(false)
        } else {
            false
        }
    }

    async fn is_recording(&self, _cx: &Context, _project_id: Option<String>) -> bool {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let state = reaper
                    .medium_reaper()
                    .get_play_state_ex(ProjectContext::CurrentProject);
                state.is_recording
            })
            .await
            .unwrap_or(false)
        } else {
            false
        }
    }

    // =========================================================================
    // Tempo Control
    // =========================================================================

    async fn get_tempo(&self, _cx: &Context, _project_id: Option<String>) -> f64 {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                reaper.current_project().tempo().bpm().get()
            })
            .await
            .unwrap_or(120.0)
        } else {
            120.0
        }
    }

    async fn set_tempo(&self, _cx: &Context, _project_id: Option<String>, bpm: f64) {
        debug!("ReaperTransport: set_tempo to {} BPM", bpm);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                if let Ok(bpm_value) = reaper_medium::Bpm::new(bpm) {
                    let tempo = ReaperTempo::from_bpm(bpm_value);
                    let _ = reaper
                        .current_project()
                        .set_tempo(tempo, UndoBehavior::OmitUndoPoint);
                }
            });
        }
    }

    // =========================================================================
    // Loop Control
    // =========================================================================

    async fn toggle_loop(&self, _cx: &Context, _project_id: Option<String>) {
        debug!("ReaperTransport: toggle_loop");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1068: Transport: Toggle repeat
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1068),
                    0,
                    ProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn is_looping(&self, _cx: &Context, _project_id: Option<String>) -> bool {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                reaper
                    .medium_reaper()
                    .get_set_repeat_ex_get(ProjectContext::CurrentProject)
            })
            .await
            .unwrap_or(false)
        } else {
            false
        }
    }

    async fn set_loop(&self, _cx: &Context, _project_id: Option<String>, enabled: bool) {
        debug!("ReaperTransport: set_loop to {}", enabled);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                reaper
                    .medium_reaper()
                    .get_set_repeat_ex_set(ProjectContext::CurrentProject, enabled);
            });
        }
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    async fn get_playrate(&self, _cx: &Context, _project_id: Option<String>) -> f64 {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                reaper
                    .current_project()
                    .play_rate()
                    .playback_speed_factor()
                    .get()
            })
            .await
            .unwrap_or(1.0)
        } else {
            1.0
        }
    }

    async fn set_playrate(&self, _cx: &Context, _project_id: Option<String>, rate: f64) {
        debug!("ReaperTransport: set_playrate to {}", rate);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                // Clamp to valid range (0.25 to 4.0)
                let clamped = rate.clamp(0.25, 4.0);
                let factor = reaper_medium::PlaybackSpeedFactor::new(clamped);
                let play_rate = PlayRate::from_playback_speed_factor(factor);
                reaper.current_project().set_play_rate(play_rate);
            });
        }
    }

    // =========================================================================
    // Time Signature
    // =========================================================================

    async fn get_time_signature(
        &self,
        _cx: &Context,
        _project_id: Option<String>,
    ) -> TimeSignature {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let medium = reaper.medium_reaper();
                let (num, denom) = get_time_signature_internal(medium);
                TimeSignature::new(num, denom)
            })
            .await
            .unwrap_or_else(|_| TimeSignature::default())
        } else {
            TimeSignature::default()
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_play_state_internal(medium: &reaper_medium::Reaper) -> PlayState {
    let state = medium.get_play_state_ex(ProjectContext::CurrentProject);
    if state.is_recording {
        PlayState::Recording
    } else if state.is_playing {
        PlayState::Playing
    } else if state.is_paused {
        PlayState::Paused
    } else {
        PlayState::Stopped
    }
}

fn get_time_signature_internal(_medium: &reaper_medium::Reaper) -> (i32, i32) {
    // Default to 4/4 for now
    // A more complete implementation would query the tempo map at the current position
    // using GetTempoTimeSigMarker or similar APIs
    (4, 4)
}
