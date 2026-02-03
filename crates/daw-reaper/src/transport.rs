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

use daw_proto::{PlayState, ProjectContext, TimeSignature, Transport, TransportService};
use reaper_high::{PlayRate, Reaper, TaskSupport, Tempo as ReaperTempo};
use reaper_medium::{
    CommandId, PositionInSeconds, ProjectContext as ReaperProjectContext, SetEditCurPosOptions,
    UndoBehavior,
};
use roam::{Context, Tx};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, info};

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

    async fn play(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: play");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1007: Transport: Play
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1007),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn pause(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: pause");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1008: Transport: Pause
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1008),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn stop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: stop");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1016: Transport: Stop
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1016),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn play_pause(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: play_pause");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40073: Transport: Play/pause
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40073),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn play_stop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: play_stop");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40044: Transport: Play/stop
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40044),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    async fn record(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: record");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1013: Transport: Record
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1013),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn stop_recording(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: stop_recording");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // Stop is also stop recording
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1016),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn toggle_recording(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: toggle_recording");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1013: Transport: Record (toggles)
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1013),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    // =========================================================================
    // Position Control
    // =========================================================================

    async fn set_position(&self, _cx: &Context, _project: ProjectContext, seconds: f64) {
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

    async fn get_position(&self, _cx: &Context, _project: ProjectContext) -> f64 {
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

    async fn goto_start(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: goto_start");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40042: Transport: Go to start of project
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40042),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn goto_end(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: goto_end");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 40043: Transport: Go to end of project
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(40043),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    async fn get_state(&self, _cx: &Context, _project: ProjectContext) -> Transport {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let project = reaper.current_project();
                let medium = reaper.medium_reaper();

                let play_state = get_play_state_internal(medium);
                let looping = medium.get_set_repeat_ex_get(ReaperProjectContext::CurrentProject);
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
                    tempo: daw_proto::primitives::Tempo::from_bpm(tempo_bpm),
                    playrate,
                    time_signature: TimeSignature::new(ts_num as u32, ts_denom as u32),
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

    async fn get_play_state(&self, _cx: &Context, _project: ProjectContext) -> PlayState {
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

    async fn is_playing(&self, _cx: &Context, _project: ProjectContext) -> bool {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let state = reaper
                    .medium_reaper()
                    .get_play_state_ex(ReaperProjectContext::CurrentProject);
                state.is_playing || state.is_recording
            })
            .await
            .unwrap_or(false)
        } else {
            false
        }
    }

    async fn is_recording(&self, _cx: &Context, _project: ProjectContext) -> bool {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let state = reaper
                    .medium_reaper()
                    .get_play_state_ex(ReaperProjectContext::CurrentProject);
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

    async fn get_tempo(&self, _cx: &Context, _project: ProjectContext) -> f64 {
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

    async fn set_tempo(&self, _cx: &Context, _project: ProjectContext, bpm: f64) {
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

    async fn toggle_loop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: toggle_loop");
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(|| {
                let reaper = Reaper::get();
                // 1068: Transport: Toggle repeat
                reaper.medium_reaper().main_on_command_ex(
                    CommandId::new(1068),
                    0,
                    ReaperProjectContext::CurrentProject,
                );
            });
        }
    }

    async fn is_looping(&self, _cx: &Context, _project: ProjectContext) -> bool {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                reaper
                    .medium_reaper()
                    .get_set_repeat_ex_get(ReaperProjectContext::CurrentProject)
            })
            .await
            .unwrap_or(false)
        } else {
            false
        }
    }

    async fn set_loop(&self, _cx: &Context, _project: ProjectContext, enabled: bool) {
        debug!("ReaperTransport: set_loop to {}", enabled);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                reaper
                    .medium_reaper()
                    .get_set_repeat_ex_set(ReaperProjectContext::CurrentProject, enabled);
            });
        }
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    async fn get_playrate(&self, _cx: &Context, _project: ProjectContext) -> f64 {
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

    async fn set_playrate(&self, _cx: &Context, _project: ProjectContext, rate: f64) {
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

    async fn get_time_signature(&self, _cx: &Context, _project: ProjectContext) -> TimeSignature {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = Reaper::get();
                let medium = reaper.medium_reaper();
                let (num, denom) = get_time_signature_internal(medium);
                TimeSignature::new(num as u32, denom as u32)
            })
            .await
            .unwrap_or_else(|_| TimeSignature::default())
        } else {
            TimeSignature::default()
        }
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
        debug!(
            "ReaperTransport: set_position_musical to {}.{}.{}",
            measure, beat, subdivision
        );
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                let medium = reaper.medium_reaper();

                // Convert beat and subdivision to PositionInBeats
                // beat is 0-indexed within measure, subdivision is 0-999
                let beats_within_measure = beat as f64 + subdivision as f64 / 1000.0;
                let Ok(beats) = reaper_medium::PositionInBeats::new(beats_within_measure) else {
                    return;
                };

                // Use TimeMap2_beatsToTime with MeasureMode to convert to time
                // MeasureMode::FromMeasureAtIndex uses 0-indexed measures
                let time_seconds = medium.time_map_2_beats_to_time(
                    ReaperProjectContext::CurrentProject,
                    reaper_medium::MeasureMode::FromMeasureAtIndex(measure),
                    beats,
                );

                if let Ok(pos) = PositionInSeconds::new(time_seconds.get()) {
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

    async fn goto_measure(&self, _cx: &Context, _project: ProjectContext, measure: i32) {
        debug!("ReaperTransport: goto_measure {}", measure);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = Reaper::get();
                let medium = reaper.medium_reaper();

                // Convert measure to time using TimeMap2_beatsToTime
                // MeasureMode::FromMeasureAtIndex uses 0-indexed measures
                // Beat 0 = start of the measure
                let beats = reaper_medium::PositionInBeats::new(0.0).unwrap();
                let time_seconds = medium.time_map_2_beats_to_time(
                    ReaperProjectContext::CurrentProject,
                    reaper_medium::MeasureMode::FromMeasureAtIndex(measure),
                    beats,
                );

                if let Ok(pos) = PositionInSeconds::new(time_seconds.get()) {
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

    // =========================================================================
    // Streaming
    // =========================================================================

    async fn subscribe_state(&self, _cx: &Context, _project: ProjectContext, tx: Tx<Transport>) {
        info!("ReaperTransport: subscribe_state - starting 60Hz stream");

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        tokio::spawn(async move {
            // ~16ms for 60Hz
            let interval = Duration::from_micros(16667);

            loop {
                tokio::time::sleep(interval).await;

                // Get transport state from main thread
                let state = if let Some(ts) = task_support() {
                    ts.main_thread_future(|| {
                        let reaper = Reaper::get();
                        let project = reaper.current_project();
                        let medium = reaper.medium_reaper();

                        let play_state = get_play_state_internal(medium);
                        let looping =
                            medium.get_set_repeat_ex_get(ReaperProjectContext::CurrentProject);
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
                            tempo: daw_proto::primitives::Tempo::from_bpm(tempo_bpm),
                            playrate,
                            time_signature: TimeSignature::new(ts_num as u32, ts_denom as u32),
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
                };

                // Send the state - exit loop when client disconnects
                if let Err(e) = tx.send(&state).await {
                    debug!("ReaperTransport: subscribe_state stream closed: {}", e);
                    break;
                }
            }

            info!("ReaperTransport: subscribe_state stream ended");
        });
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_play_state_internal(medium: &reaper_medium::Reaper) -> PlayState {
    let state = medium.get_play_state_ex(ReaperProjectContext::CurrentProject);
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
