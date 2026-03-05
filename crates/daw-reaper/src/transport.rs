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
//! # Per-Project Transport State Broadcasting
//!
//! For low-latency state streaming with multiple projects, the transport uses a
//! reactive push-based architecture:
//!
//! ```text
//! Timer Callback (main thread, ~30Hz)
//!     ↓
//!     poll_and_broadcast() called directly
//!     ↓
//!     For each open project:
//!       - Read transport state from REAPER
//!       - Compare with cached state
//!       - Only broadcast if changed (reactive)
//!     ↓
//! Subscribers receive (project_guid, Transport) updates
//! ```
//!
//! This avoids:
//! - Round-trip latency of `main_thread_future()` for streaming
//! - Flooding with updates for projects that aren't playing
//! - SHM slot exhaustion from constant polling

use crate::project_context::{find_project_by_guid, project_guid as project_guid_from};
use daw_proto::{PlayState, ProjectContext, TimeSignature, Transport, TransportService};
use reaper_high::{PlayRate, Project, Reaper, TaskSupport, Tempo as ReaperTempo};
use reaper_medium::{
    CommandId, PositionInSeconds, ProjectContext as ReaperProjectContext, ProjectRef,
    SetEditCurPosOptions, TimeRangeType, UndoBehavior,
};
use roam::{Context, Tx};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use tracing::{debug, info};

use crate::main_thread;

/// Per-project transport update - includes project GUID so subscribers know which project changed
#[derive(Clone, Debug)]
pub struct ProjectTransportUpdate {
    /// Project GUID (hash of file path)
    pub project_guid: String,
    /// Transport state for this project
    pub transport: Transport,
}

/// Global transport state broadcaster - sends per-project state updates
static TRANSPORT_BROADCASTER: OnceLock<broadcast::Sender<ProjectTransportUpdate>> = OnceLock::new();

/// Legacy broadcaster for single-project compatibility (current project only)
static LEGACY_TRANSPORT_BROADCASTER: OnceLock<broadcast::Sender<Transport>> = OnceLock::new();

/// Cached per-project transport states for change detection
/// Key is project GUID, value is last known transport state
/// Only broadcasts when state actually changes (reactive pattern)
static PROJECT_TRANSPORT_CACHE: OnceLock<Mutex<HashMap<String, Transport>>> = OnceLock::new();

/// Get the global TaskSupport reference.
///
/// Delegates to [`main_thread::task_support`]. Kept for use by transport-internal
/// code (polling, broadcasting) that needs direct TaskSupport access.
#[allow(dead_code)]
pub(crate) fn task_support() -> Option<&'static TaskSupport> {
    main_thread::task_support()
}

/// Initialize the transport broadcaster.
/// Called by the extension during initialization.
/// Returns the legacy broadcast sender for backward compatibility.
pub fn init_transport_broadcaster() -> broadcast::Sender<Transport> {
    // Create per-project broadcast channel with enough capacity to handle bursts
    // 32 slots for ~1s of buffering at 30Hz with multiple projects
    let (tx, _rx) = broadcast::channel::<ProjectTransportUpdate>(32);
    let _ = TRANSPORT_BROADCASTER.set(tx);

    // Create legacy broadcast channel for backward compatibility
    let (legacy_tx, _rx) = broadcast::channel::<Transport>(16);
    let legacy_tx_clone = legacy_tx.clone();
    let _ = LEGACY_TRANSPORT_BROADCASTER.set(legacy_tx);

    // Initialize the per-project state cache for change detection
    let _ = PROJECT_TRANSPORT_CACHE.set(Mutex::new(HashMap::new()));

    legacy_tx_clone
}

/// Get a receiver for per-project transport state updates.
/// Returns updates for ALL projects, only when their state changes.
pub fn project_transport_receiver() -> Option<broadcast::Receiver<ProjectTransportUpdate>> {
    TRANSPORT_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Get a receiver for legacy transport state updates (current project only).
/// Used by subscribe_state for backward compatibility.
fn transport_receiver() -> Option<broadcast::Receiver<Transport>> {
    LEGACY_TRANSPORT_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Get project GUID from a REAPER project (delegates to shared implementation).
fn project_guid(project: &Project) -> String {
    project_guid_from(project)
}

/// Threshold for position change detection (in seconds)
/// Only emit position changes larger than this to avoid flooding with micro-updates
const POSITION_CHANGE_THRESHOLD: f64 = 0.001; // 1ms

/// Poll REAPER transport state for ALL open projects and broadcast changes.
/// **MUST be called from the main thread** (e.g., from timer callback).
///
/// This function reads REAPER state directly without async overhead,
/// enabling low-latency state streaming.
///
/// **Reactive Pattern**: Only broadcasts when a project's state actually changes.
/// Projects that are stopped/idle won't generate any updates, preventing
/// SHM slot exhaustion from constant polling.
pub fn poll_and_broadcast() {
    let tx = TRANSPORT_BROADCASTER.get();
    let legacy_tx = LEGACY_TRANSPORT_BROADCASTER.get();

    // Skip if no subscribers on either channel
    let has_project_subscribers = tx.map(|t| t.receiver_count() > 0).unwrap_or(false);
    let has_legacy_subscribers = legacy_tx.map(|t| t.receiver_count() > 0).unwrap_or(false);

    if !has_project_subscribers && !has_legacy_subscribers {
        return;
    }

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    // Get current project for legacy broadcaster
    let current_project = reaper.current_project();
    let current_guid = project_guid(&current_project);

    // Get cache for change detection
    let Some(cache) = PROJECT_TRANSPORT_CACHE.get() else {
        return;
    };
    let mut cache_guard = cache.lock().unwrap();

    // Track which projects we've seen this poll (for cleanup of closed projects)
    let mut seen_guids = Vec::new();

    // Iterate through all open projects
    for tab_index in 0..128u32 {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            // No more projects
            break;
        };

        let project = Project::new(result.project);
        let guid = project_guid(&project);
        seen_guids.push(guid.clone());

        // Read transport state for this specific project
        let reaper_ctx = ReaperProjectContext::Proj(result.project);
        let state = read_transport_state_for_project(&project, reaper_ctx, medium);

        // Check if state has changed for this project (reactive pattern)
        let should_broadcast = match cache_guard.get(&guid) {
            None => {
                // First time seeing this project - always broadcast
                cache_guard.insert(guid.clone(), state.clone());
                true
            }
            Some(prev) => {
                if transport_changed(prev, &state) {
                    cache_guard.insert(guid.clone(), state.clone());
                    true
                } else {
                    false
                }
            }
        };

        if should_broadcast {
            // Broadcast per-project update
            if let Some(tx) = tx {
                let _ = tx.send(ProjectTransportUpdate {
                    project_guid: guid.clone(),
                    transport: state.clone(),
                });
            }

            // Also broadcast to legacy channel if this is the current project
            if guid == current_guid
                && let Some(legacy_tx) = legacy_tx
            {
                let _ = legacy_tx.send(state);
            }
        }
    }

    // Clean up cache entries for projects that are no longer open
    cache_guard.retain(|guid, _| seen_guids.contains(guid));
}

/// Check if transport state has changed meaningfully.
/// Uses threshold for position to avoid flooding with playhead micro-updates.
fn transport_changed(prev: &Transport, curr: &Transport) -> bool {
    // Check discrete state changes first (these should always trigger)
    if prev.play_state != curr.play_state {
        return true;
    }
    if prev.record_mode != curr.record_mode {
        return true;
    }
    if prev.looping != curr.looping {
        return true;
    }
    if prev.loop_region != curr.loop_region {
        return true;
    }
    if prev.time_signature != curr.time_signature {
        return true;
    }

    // Check tempo change (use small threshold for floating point comparison)
    let prev_tempo = prev.tempo.bpm();
    let curr_tempo = curr.tempo.bpm();
    if (prev_tempo - curr_tempo).abs() > 0.01 {
        return true;
    }

    // Check playrate change
    if (prev.playrate - curr.playrate).abs() > 0.001 {
        return true;
    }

    // Check playhead position change with threshold
    // Only send position updates if changed by more than threshold
    // This is the main source of flooding - playhead moves constantly during playback
    let prev_pos = prev
        .playhead_position
        .time
        .map(|t| t.as_seconds())
        .unwrap_or(0.0);
    let curr_pos = curr
        .playhead_position
        .time
        .map(|t| t.as_seconds())
        .unwrap_or(0.0);
    if (prev_pos - curr_pos).abs() > POSITION_CHANGE_THRESHOLD {
        return true;
    }

    // Check edit position change with threshold
    let prev_edit = prev
        .edit_position
        .time
        .map(|t| t.as_seconds())
        .unwrap_or(0.0);
    let curr_edit = curr
        .edit_position
        .time
        .map(|t| t.as_seconds())
        .unwrap_or(0.0);
    if (prev_edit - curr_edit).abs() > POSITION_CHANGE_THRESHOLD {
        return true;
    }

    false
}

/// Read transport state from REAPER for a specific project.
/// **MUST be called from the main thread.**
fn read_transport_state_for_project(
    project: &Project,
    reaper_ctx: ReaperProjectContext,
    medium: &reaper_medium::Reaper,
) -> Transport {
    let play_state = get_play_state_for_project(medium, reaper_ctx);
    let looping = medium.get_set_repeat_ex_get(reaper_ctx);
    let tempo_bpm = project.tempo().bpm().get();
    let playrate = project.play_rate().playback_speed_factor().get();

    // Use the project-specific position APIs
    let pos_seconds = medium.get_play_position_ex(reaper_ctx).get();
    let edit_pos = medium
        .get_cursor_position_ex(reaper_ctx)
        .map(|p| p.get())
        .unwrap_or(0.0);

    let (ts_num, ts_denom) = get_time_signature_for_project(medium, reaper_ctx);

    // Read loop region (loop points, not time selection)
    let loop_region = medium
        .get_set_loop_time_range_2_get(reaper_ctx, TimeRangeType::LoopPoints)
        .map(|range| daw_proto::LoopRegion::new(range.start.get(), range.end.get()));

    // Convert time positions to musical positions using REAPER's tempo map
    // This properly handles tempo changes throughout the project
    // Also applies project measure offset for correct display
    let playhead_musical = time_to_musical_position(project, medium, reaper_ctx, pos_seconds);
    let edit_musical = time_to_musical_position(project, medium, reaper_ctx, edit_pos);

    Transport {
        play_state,
        record_mode: daw_proto::RecordMode::Normal,
        looping,
        loop_region,
        tempo: daw_proto::primitives::Tempo::from_bpm(tempo_bpm),
        playrate,
        time_signature: TimeSignature::new(ts_num as u32, ts_denom as u32),
        playhead_position: daw_proto::primitives::Position::new(
            Some(playhead_musical),
            Some(daw_proto::primitives::TimePosition::from_seconds(
                pos_seconds,
            )),
            None,
        ),
        edit_position: daw_proto::primitives::Position::new(
            Some(edit_musical),
            Some(daw_proto::primitives::TimePosition::from_seconds(edit_pos)),
            None,
        ),
    }
}

/// Convert a time position to a musical position using REAPER's TimeMap2_timeToBeats.
/// This properly handles tempo and time signature changes throughout the project.
/// Also applies the project measure offset (projmeasoffs) for display.
/// **MUST be called from the main thread.**
fn time_to_musical_position(
    project: &Project,
    medium: &reaper_medium::Reaper,
    reaper_ctx: ReaperProjectContext,
    time_seconds: f64,
) -> daw_proto::primitives::MusicalPosition {
    use reaper_medium::PositionInSeconds;

    // PositionInSeconds::new returns a Result, handle the error case
    let Some(pos) = PositionInSeconds::new(time_seconds).ok() else {
        return daw_proto::primitives::MusicalPosition::new(1, 1, 0);
    };

    let result = medium.time_map_2_time_to_beats(reaper_ctx, pos);

    // Get project measure offset - this is the "projmeasoffs" setting that affects
    // how measure numbers are displayed (e.g., if project starts at measure 5)
    let measure_offset = project.measure_offset();

    // REAPER returns 0-based measure index, apply offset and convert to 1-based for display
    // The measure_offset is already the adjustment value (can be positive or negative)
    let measure = result.measure_index + measure_offset + 1;

    // beats_since_measure is fractional beats since the start of the measure
    let beats_since = result.beats_since_measure.get();
    let beat = beats_since.floor() as i32 + 1; // 1-based beat within measure

    // Subdivision is the fractional part of the beat (0-999)
    let subdivision = ((beats_since.fract()) * 1000.0).round() as i32;

    daw_proto::primitives::MusicalPosition::new(measure, beat, subdivision.clamp(0, 999))
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
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 1007: Transport: Play
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1007),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn pause(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: pause");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 1008: Transport: Pause
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1008),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn stop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: stop");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 1016: Transport: Stop
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1016),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn play_pause(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: play_pause");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 40073: Transport: Play/pause
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(40073),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn play_stop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: play_stop");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 40044: Transport: Play/stop
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(40044),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn play_from_last_start_position(&self, _cx: &Context, project: ProjectContext) {
        debug!("ReaperTransport: play_from_last_start_position (fallback to play)");
        self.play(_cx, project).await;
    }

    // =========================================================================
    // Recording Control
    // =========================================================================

    async fn record(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: record");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 1013: Transport: Record
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1013),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn stop_recording(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: stop_recording");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // Stop is also stop recording
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1016),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn toggle_recording(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: toggle_recording");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 1013: Transport: Record (toggles)
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1013),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    // =========================================================================
    // Position Control
    // =========================================================================

    async fn set_position(&self, _cx: &Context, _project: ProjectContext, seconds: f64) {
        debug!("ReaperTransport: set_position to {} seconds", seconds);
        let _ = main_thread::query(move || {
            let reaper = Reaper::get();
            match PositionInSeconds::new(seconds) {
                Ok(pos) => {
                    reaper.current_project().set_edit_cursor_position(
                        pos,
                        SetEditCurPosOptions {
                            move_view: false,
                            seek_play: true,
                        },
                    );
                    let actual = reaper
                        .current_project()
                        .play_or_edit_cursor_position()
                        .map(|p| p.get())
                        .unwrap_or(f64::NAN);
                    debug!(
                        "ReaperTransport: set_position requested={:.3}, actual={:.3}",
                        seconds, actual
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "ReaperTransport: set_position failed — PositionInSeconds::new({}) error: {:?}",
                        seconds, e
                    );
                }
            }
        })
        .await;
    }

    async fn get_position(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper
                .current_project()
                .play_or_edit_cursor_position()
                .map(|p| p.get())
                .unwrap_or(0.0)
        })
        .await
        .unwrap_or(0.0)
    }

    async fn goto_start(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: goto_start");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 40042: Transport: Go to start of project
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(40042),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn goto_end(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: goto_end");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 40043: Transport: Go to end of project
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(40043),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    // =========================================================================
    // State Queries
    // =========================================================================

    async fn get_state(&self, _cx: &Context, project: ProjectContext) -> Transport {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Resolve the project context to find the correct REAPER project
            let (rea_project, reaper_ctx) = match &project {
                ProjectContext::Current => {
                    let proj = reaper.current_project();
                    (proj, ReaperProjectContext::CurrentProject)
                }
                ProjectContext::Project(guid) => {
                    // Find project by GUID
                    if let Some(proj) = find_project_by_guid(guid) {
                        let ctx = ReaperProjectContext::Proj(proj.raw());
                        (proj, ctx)
                    } else {
                        // Fallback to current project if not found
                        let proj = reaper.current_project();
                        (proj, ReaperProjectContext::CurrentProject)
                    }
                }
            };

            read_transport_state_for_project(&rea_project, reaper_ctx, medium)
        })
        .await
        .unwrap_or_default()
    }

    async fn get_play_state(&self, _cx: &Context, _project: ProjectContext) -> PlayState {
        main_thread::query(|| {
            let reaper = Reaper::get();
            get_play_state_internal(reaper.medium_reaper())
        })
        .await
        .unwrap_or(PlayState::Stopped)
    }

    async fn is_playing(&self, _cx: &Context, _project: ProjectContext) -> bool {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let state = reaper
                .medium_reaper()
                .get_play_state_ex(ReaperProjectContext::CurrentProject);
            state.is_playing || state.is_recording
        })
        .await
        .unwrap_or(false)
    }

    async fn is_recording(&self, _cx: &Context, _project: ProjectContext) -> bool {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let state = reaper
                .medium_reaper()
                .get_play_state_ex(ReaperProjectContext::CurrentProject);
            state.is_recording
        })
        .await
        .unwrap_or(false)
    }

    // =========================================================================
    // Tempo Control
    // =========================================================================

    async fn get_tempo(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper.current_project().tempo().bpm().get()
        })
        .await
        .unwrap_or(120.0)
    }

    async fn set_tempo(&self, _cx: &Context, _project: ProjectContext, bpm: f64) {
        debug!("ReaperTransport: set_tempo to {} BPM", bpm);
        main_thread::run(move || {
            let reaper = Reaper::get();
            if let Ok(bpm_value) = reaper_medium::Bpm::new(bpm) {
                let tempo = ReaperTempo::from_bpm(bpm_value);
                let _ = reaper
                    .current_project()
                    .set_tempo(tempo, UndoBehavior::OmitUndoPoint);
            }
        });
    }

    // =========================================================================
    // Loop Control
    // =========================================================================

    async fn toggle_loop(&self, _cx: &Context, _project: ProjectContext) {
        debug!("ReaperTransport: toggle_loop");
        main_thread::run(|| {
            let reaper = Reaper::get();
            // 1068: Transport: Toggle repeat
            reaper.medium_reaper().main_on_command_ex(
                CommandId::new(1068),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn is_looping(&self, _cx: &Context, _project: ProjectContext) -> bool {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper
                .medium_reaper()
                .get_set_repeat_ex_get(ReaperProjectContext::CurrentProject)
        })
        .await
        .unwrap_or(false)
    }

    async fn set_loop(&self, _cx: &Context, _project: ProjectContext, enabled: bool) {
        debug!("ReaperTransport: set_loop to {}", enabled);
        main_thread::run(move || {
            let reaper = Reaper::get();
            reaper
                .medium_reaper()
                .get_set_repeat_ex_set(ReaperProjectContext::CurrentProject, enabled);
        });
    }

    // =========================================================================
    // Playrate Control
    // =========================================================================

    async fn get_playrate(&self, _cx: &Context, _project: ProjectContext) -> f64 {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper
                .current_project()
                .play_rate()
                .playback_speed_factor()
                .get()
        })
        .await
        .unwrap_or(1.0)
    }

    async fn set_playrate(&self, _cx: &Context, _project: ProjectContext, rate: f64) {
        debug!("ReaperTransport: set_playrate to {}", rate);
        main_thread::run(move || {
            let reaper = Reaper::get();
            // Clamp to valid range (0.25 to 4.0)
            let clamped = rate.clamp(0.25, 4.0);
            let factor = reaper_medium::PlaybackSpeedFactor::new(clamped);
            let play_rate = PlayRate::from_playback_speed_factor(factor);
            reaper.current_project().set_play_rate(play_rate);
        });
    }

    // =========================================================================
    // Time Signature
    // =========================================================================

    async fn get_time_signature(&self, _cx: &Context, _project: ProjectContext) -> TimeSignature {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let (num, denom) = get_time_signature_internal(medium);
            TimeSignature::new(num as u32, denom as u32)
        })
        .await
        .unwrap_or_default()
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
        main_thread::run(move || {
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

    async fn goto_measure(&self, _cx: &Context, _project: ProjectContext, measure: i32) {
        debug!("ReaperTransport: goto_measure {}", measure);
        main_thread::run(move || {
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

    // =========================================================================
    // Streaming
    // =========================================================================

    async fn subscribe_state(&self, _cx: &Context, _project: ProjectContext, tx: Tx<Transport>) {
        info!("ReaperTransport: subscribe_state - subscribing to broadcast channel");

        // Get a receiver for the broadcast channel
        let Some(mut rx) = transport_receiver() else {
            info!(
                "ReaperTransport: broadcast channel not initialized, subscriber will not receive updates"
            );
            return;
        };

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        peeps::spawn_tracked!("reaper-transport-subscribe", async move {
            loop {
                // Wait for the next state update from the broadcaster
                match rx.recv().await {
                    Ok(state) => {
                        // Forward state to the RPC stream
                        if let Err(e) = tx.send(&state).await {
                            debug!("ReaperTransport: subscribe_state stream closed: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        // We missed some messages due to slow consumption - that's fine,
                        // just continue with the next one
                        debug!(
                            "ReaperTransport: subscribe_state lagged by {} messages",
                            count
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Broadcaster was dropped - this shouldn't happen during normal operation
                        info!("ReaperTransport: broadcast channel closed");
                        break;
                    }
                }
            }

            info!("ReaperTransport: subscribe_state stream ended");
        });
    }

    async fn subscribe_all_projects(&self, _cx: &Context, tx: Tx<daw_proto::AllProjectsTransport>) {
        info!(
            "ReaperTransport: subscribe_all_projects - subscribing to per-project broadcast channel"
        );

        // Get a receiver for the per-project broadcast channel
        let Some(mut rx) = project_transport_receiver() else {
            info!(
                "ReaperTransport: per-project broadcast channel not initialized, subscriber will not receive updates"
            );
            return;
        };

        // Spawn the streaming loop so this method returns immediately
        peeps::spawn_tracked!("reaper-transport-subscribe-all", async move {
            // Buffer to collect updates within a small window
            // This batches multiple project updates into a single message
            let mut pending_updates: HashMap<String, Transport> = HashMap::new();
            let batch_interval = tokio::time::Duration::from_millis(16); // ~60Hz output
            let mut batch_timer = tokio::time::interval(batch_interval);

            loop {
                tokio::select! {
                    // Receive updates from the broadcast channel
                    result = rx.recv() => {
                        match result {
                            Ok(update) => {
                                // Buffer the update (overwrites previous for same project)
                                pending_updates.insert(update.project_guid, update.transport);
                            }
                            Err(broadcast::error::RecvError::Lagged(count)) => {
                                debug!(
                                    "ReaperTransport: subscribe_all_projects lagged by {} messages",
                                    count
                                );
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("ReaperTransport: per-project broadcast channel closed");
                                break;
                            }
                        }
                    }
                    // Send batched updates at regular intervals
                    _ = batch_timer.tick() => {
                        if !pending_updates.is_empty() {
                            let projects: Vec<daw_proto::ProjectTransportState> = pending_updates
                                .drain()
                                .map(|(guid, transport)| daw_proto::ProjectTransportState {
                                    project_guid: guid,
                                    transport,
                                })
                                .collect();

                            let update = daw_proto::AllProjectsTransport { projects };

                            if let Err(e) = tx.send(&update).await {
                                debug!("ReaperTransport: subscribe_all_projects stream closed: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            info!("ReaperTransport: subscribe_all_projects stream ended");
        });
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_play_state_internal(medium: &reaper_medium::Reaper) -> PlayState {
    get_play_state_for_project(medium, ReaperProjectContext::CurrentProject)
}

fn get_play_state_for_project(
    medium: &reaper_medium::Reaper,
    reaper_ctx: ReaperProjectContext,
) -> PlayState {
    let state = medium.get_play_state_ex(reaper_ctx);
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

fn get_time_signature_internal(medium: &reaper_medium::Reaper) -> (i32, i32) {
    get_time_signature_for_project(medium, ReaperProjectContext::CurrentProject)
}

fn get_time_signature_for_project(
    medium: &reaper_medium::Reaper,
    reaper_ctx: ReaperProjectContext,
) -> (i32, i32) {
    // Query the tempo map at the current play/edit cursor position
    // to get the time signature that's actually active
    let pos = medium.get_play_position_ex(reaper_ctx);
    let result = medium.time_map_2_time_to_beats(reaper_ctx, pos);
    (
        result.time_signature.numerator.get() as i32,
        result.time_signature.denominator.get() as i32,
    )
}
