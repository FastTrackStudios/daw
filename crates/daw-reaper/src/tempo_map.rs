//! REAPER Tempo Map Implementation
//!
//! Implements TempoMapService for REAPER's tempo/time signature system.
//! Uses low-level REAPER APIs via medium_reaper().low() for tempo marker access.
//!
//! # Tempo Map Change Detection
//!
//! Tempo maps are small (usually <50 points), so full comparison on each poll
//! is cheap. The broadcaster follows the same reactive pattern as transport:
//!
//! ```text
//! Timer Callback (main thread, ~30Hz)
//!     ↓
//!     poll_and_broadcast_tempo_map() called directly
//!     ↓
//!     For each open project:
//!       - Read all tempo markers from REAPER
//!       - Compare with cached markers
//!       - Only broadcast TempoMapEvent::MapChanged if different
//! ```

use crate::main_thread;
use crate::project_context::{MAX_PROJECT_TABS, project_guid as project_guid_from};
use crate::safe_wrappers::tempo as sw;
use crate::safe_wrappers::time_map as tw;
use daw_proto::{
    Position, ProjectContext, TempoMapEvent, TempoMapService, TempoPoint, TimePosition,
    TimeSignature,
};
use reaper_high::{Project, Reaper};
use reaper_medium::{MeasureMode, ProjectContext as ReaperProjectContext, ProjectRef};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use tracing::{debug, info};
use vox::Tx;

// =============================================================================
// Tempo Map Change Detection — broadcaster + cache + poll
// =============================================================================

/// Global tempo map event broadcaster
static TEMPO_MAP_BROADCASTER: OnceLock<broadcast::Sender<TempoMapEvent>> = OnceLock::new();

/// Cached per-project tempo map state for change detection.
/// Key is project GUID, value is the list of cached tempo points.
static TEMPO_MAP_CACHE: OnceLock<Mutex<HashMap<String, Vec<CachedTempoPoint>>>> = OnceLock::new();

/// Lightweight representation of a tempo point for cache comparison.
/// Uses a custom PartialEq with threshold for BPM to handle floating-point noise.
#[derive(Clone, Debug)]
struct CachedTempoPoint {
    position: f64, // seconds
    bpm: f64,
    numerator: i32,
    denominator: i32,
}

impl PartialEq for CachedTempoPoint {
    fn eq(&self, other: &Self) -> bool {
        // BPM: use threshold for floating-point comparison
        if (self.bpm - other.bpm).abs() > 0.01 {
            return false;
        }
        // Position: use 1ms threshold
        if (self.position - other.position).abs() > 0.001 {
            return false;
        }
        self.numerator == other.numerator && self.denominator == other.denominator
    }
}

/// Initialize the tempo map broadcaster and cache.
/// Called by the extension during initialization.
pub fn init_tempo_map_broadcaster() {
    let (tx, _rx) = broadcast::channel::<TempoMapEvent>(256);
    let _ = TEMPO_MAP_BROADCASTER.set(tx);
    let _ = TEMPO_MAP_CACHE.set(Mutex::new(HashMap::new()));
}

/// Get a receiver for tempo map change events.
fn tempo_map_receiver() -> Option<broadcast::Receiver<TempoMapEvent>> {
    TEMPO_MAP_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Get project GUID from a REAPER project (delegates to shared implementation).
fn project_guid(project: &Project) -> String {
    project_guid_from(project)
}

/// Read the current tempo map for a project as a Vec of CachedTempoPoints.
/// **MUST be called from the main thread.**
fn read_cached_tempo_points(
    low: &reaper_low::Reaper,
    medium: &reaper_medium::Reaper,
    reaper_ctx: ReaperProjectContext,
) -> Vec<CachedTempoPoint> {
    let count = medium.count_tempo_time_sig_markers(reaper_ctx);
    let mut points = Vec::with_capacity(count as usize);

    for i in 0..count {
        if let Some(m) = sw::get_tempo_marker(low, reaper_ctx, i as i32) {
            points.push(CachedTempoPoint {
                position: m.timepos,
                bpm: m.bpm,
                numerator: m.timesig_num,
                denominator: m.timesig_denom,
            });
        }
    }

    points
}

/// Read the current tempo map for a project as a Vec of TempoPoints (proto type).
/// **MUST be called from the main thread.**
fn read_tempo_points(
    low: &reaper_low::Reaper,
    medium: &reaper_medium::Reaper,
    reaper_ctx: ReaperProjectContext,
) -> Vec<TempoPoint> {
    let count = medium.count_tempo_time_sig_markers(reaper_ctx);
    let mut points = Vec::with_capacity(count as usize);

    for i in 0..count {
        if let Some(m) = sw::get_tempo_marker(low, reaper_ctx, i as i32) {
            points.push(marker_to_point(&m));
        }
    }

    points
}

/// Poll REAPER tempo map state for ALL open projects and broadcast changes.
/// **MUST be called from the main thread** (e.g., from timer callback).
///
/// Tempo maps are usually small (<50 points), so full comparison each poll is cheap.
/// Only broadcasts `TempoMapEvent::MapChanged` when the tempo map actually differs.
pub fn poll_and_broadcast_tempo_map() {
    let Some(tx) = TEMPO_MAP_BROADCASTER.get() else {
        return;
    };

    // Skip if no subscribers
    if tx.receiver_count() == 0 {
        return;
    }

    let Some(cache) = TEMPO_MAP_CACHE.get() else {
        return;
    };

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let low = medium.low();

    let mut cache_guard = cache.lock().unwrap();
    let mut seen_guids = Vec::new();

    // Iterate through all open projects
    for tab_index in 0..MAX_PROJECT_TABS {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            break;
        };

        let project = Project::new(result.project);
        let guid = project_guid(&project);
        seen_guids.push(guid.clone());

        let reaper_ctx = ReaperProjectContext::Proj(result.project);
        let current_points = read_cached_tempo_points(low, medium, reaper_ctx);

        let should_broadcast = match cache_guard.get(&guid) {
            None => {
                // First time seeing this project — always broadcast
                cache_guard.insert(guid.clone(), current_points.clone());
                true
            }
            Some(prev) => {
                if prev.len() != current_points.len() || prev != &current_points {
                    cache_guard.insert(guid.clone(), current_points.clone());
                    true
                } else {
                    false
                }
            }
        };

        if should_broadcast {
            // Read the full TempoPoint data for the event payload
            let proto_points = read_tempo_points(low, medium, reaper_ctx);
            let _ = tx.send(TempoMapEvent::MapChanged(proto_points));
        }
    }

    // Clean up cache entries for projects that are no longer open
    cache_guard.retain(|guid, _| seen_guids.contains(guid));
}

// =============================================================================
// Public sync helpers — callable directly from the main thread
// =============================================================================

/// Convert a time position (seconds) to quarter-note position.
///
/// Must be called from the main thread.
pub fn time_to_qn_on_main_thread(seconds: f64) -> f64 {
    let low = Reaper::get().medium_reaper().low();
    tw::time_to_qn(low, ReaperProjectContext::CurrentProject, seconds)
}

/// Convert a quarter-note position to time position (seconds).
///
/// Must be called from the main thread.
pub fn qn_to_time_on_main_thread(qn: f64) -> f64 {
    let low = Reaper::get().medium_reaper().low();
    tw::qn_to_time_current(low, qn)
}

/// Get the tempo (BPM) and time signature (numerator, denominator) at a given
/// time position.
///
/// Must be called from the main thread.
pub fn get_tempo_and_time_sig_at_on_main_thread(seconds: f64) -> (f64, i32, i32) {
    let low = Reaper::get().medium_reaper().low();
    let ts = tw::get_time_sig_at_time(low, ReaperProjectContext::CurrentProject, seconds);
    (ts.tempo, ts.num, ts.denom)
}

/// REAPER tempo map implementation.
///
/// Provides full access to REAPER's tempo envelope and time signature markers
/// using low-level APIs.
#[derive(Clone)]
pub struct ReaperTempoMap;

impl ReaperTempoMap {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperTempoMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a `TempoMarkerRaw` to a `TempoPoint`.
fn marker_to_point(m: &sw::TempoMarkerRaw) -> TempoPoint {
    let time_sig = if m.timesig_num > 0 && m.timesig_denom > 0 {
        Some(TimeSignature::new(
            m.timesig_num as u32,
            m.timesig_denom as u32,
        ))
    } else {
        None
    };

    TempoPoint {
        position: Position::from_time(TimePosition::from_seconds(m.timepos)),
        bpm: m.bpm,
        time_signature: time_sig,
        shape: None,
        bezier_tension: None,
        selected: None,
        linear: Some(m.lineartempo),
    }
}

impl TempoMapService for ReaperTempoMap {
    // =========================================================================
    // Query Methods
    // =========================================================================

    async fn get_tempo_points(&self, _project: ProjectContext) -> Vec<TempoPoint> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();
            let count = medium.count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);

            let mut points = Vec::with_capacity(count as usize);

            for i in 0..count {
                if let Some(m) =
                    sw::get_tempo_marker(low, ReaperProjectContext::CurrentProject, i as i32)
                {
                    points.push(marker_to_point(&m));
                }
            }

            points
        })
        .await
        .unwrap_or_default()
    }

    async fn get_tempo_point(&self, _project: ProjectContext, index: u32) -> Option<TempoPoint> {
        main_thread::query(move || {
            let low = Reaper::get().medium_reaper().low();
            sw::get_tempo_marker(low, ReaperProjectContext::CurrentProject, index as i32)
                .map(|m| marker_to_point(&m))
        })
        .await
        .unwrap_or(None)
    }

    async fn tempo_point_count(&self, _project: ProjectContext) -> usize {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper
                .medium_reaper()
                .count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject)
                as usize
        })
        .await
        .unwrap_or(0)
    }

    async fn get_tempo_at(&self, _project: ProjectContext, seconds: f64) -> f64 {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Ok(pos) = reaper_medium::PositionInSeconds::new(seconds) {
                medium
                    .time_map_2_get_divided_bpm_at_time(ReaperProjectContext::CurrentProject, pos)
                    .get()
            } else {
                reaper.current_project().tempo().bpm().get()
            }
        })
        .await
        .unwrap_or(120.0)
    }

    async fn get_time_signature_at(&self, _project: ProjectContext, seconds: f64) -> (i32, i32) {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Ok(pos) = reaper_medium::PositionInSeconds::new(seconds) {
                let beat_info =
                    medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
                (
                    beat_info.time_signature.numerator.get() as i32,
                    beat_info.time_signature.denominator.get() as i32,
                )
            } else {
                (4, 4)
            }
        })
        .await
        .unwrap_or((4, 4))
    }

    async fn time_to_qn(&self, _project: ProjectContext, seconds: f64) -> f64 {
        main_thread::query(move || time_to_qn_on_main_thread(seconds))
            .await
            .unwrap_or(0.0)
    }

    async fn qn_to_time(&self, _project: ProjectContext, qn: f64) -> f64 {
        main_thread::query(move || qn_to_time_on_main_thread(qn))
            .await
            .unwrap_or(0.0)
    }

    async fn time_to_musical(&self, _project: ProjectContext, seconds: f64) -> (i32, i32, f64) {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Ok(pos) = reaper_medium::PositionInSeconds::new(seconds) {
                let result =
                    medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
                let measure = result.measure_index + 1;
                let beats_since = result.beats_since_measure.get();
                let beat_in_measure = beats_since.floor() as i32 + 1;
                let fraction = beats_since.fract();
                (measure, beat_in_measure, fraction)
            } else {
                (1, 1, 0.0)
            }
        })
        .await
        .unwrap_or((1, 1, 0.0))
    }

    async fn musical_to_time(
        &self,
        _project: ProjectContext,
        measure: i32,
        beat: i32,
        fraction: f64,
    ) -> f64 {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let measure_0based = (measure - 1).max(0);
            let beat_0based = (beat - 1).max(0) as f64 + fraction;

            if let Ok(beats) = reaper_medium::PositionInBeats::new(beat_0based) {
                let result = medium.time_map_2_beats_to_time(
                    ReaperProjectContext::CurrentProject,
                    MeasureMode::FromMeasureAtIndex(measure_0based),
                    beats,
                );
                result.get()
            } else {
                0.0
            }
        })
        .await
        .unwrap_or(0.0)
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    async fn add_tempo_point(&self, _project: ProjectContext, seconds: f64, bpm: f64) -> u32 {
        debug!(
            "ReaperTempoMap: add_tempo_point at {} seconds, {} BPM",
            seconds, bpm
        );
        main_thread::query(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            let result = sw::set_tempo_marker(
                low,
                ReaperProjectContext::CurrentProject,
                -1, // add new
                seconds,
                -1,   // measurepos (auto)
                -1.0, // beatpos (auto)
                bpm,
                0,     // timesig_num (don't change)
                0,     // timesig_denom (don't change)
                false, // lineartempo
            );

            if result {
                let count = reaper
                    .medium_reaper()
                    .count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);
                count.saturating_sub(1)
            } else {
                0
            }
        })
        .await
        .unwrap_or(0)
    }

    async fn remove_tempo_point(&self, _project: ProjectContext, index: u32) {
        debug!("ReaperTempoMap: remove_tempo_point at index {}", index);
        main_thread::run(move || {
            let low = Reaper::get().medium_reaper().low();
            sw::delete_tempo_marker(low, ReaperProjectContext::CurrentProject, index as i32);
        });
    }

    async fn set_tempo_at_point(&self, _project: ProjectContext, index: u32, bpm: f64) {
        debug!(
            "ReaperTempoMap: set_tempo_at_point index {} to {} BPM",
            index, bpm
        );
        main_thread::run(move || {
            let low = Reaper::get().medium_reaper().low();

            if let Some(m) =
                sw::get_tempo_marker(low, ReaperProjectContext::CurrentProject, index as i32)
            {
                sw::set_tempo_marker(
                    low,
                    ReaperProjectContext::CurrentProject,
                    index as i32,
                    m.timepos,
                    m.measurepos,
                    m.beatpos,
                    bpm,
                    m.timesig_num,
                    m.timesig_denom,
                    m.lineartempo,
                );
            }
        });
    }

    async fn set_time_signature_at_point(
        &self,
        _project: ProjectContext,
        index: u32,
        numerator: i32,
        denominator: i32,
    ) {
        debug!(
            "ReaperTempoMap: set_time_signature_at_point index {} to {}/{}",
            index, numerator, denominator
        );
        main_thread::run(move || {
            let low = Reaper::get().medium_reaper().low();

            if let Some(m) =
                sw::get_tempo_marker(low, ReaperProjectContext::CurrentProject, index as i32)
            {
                sw::set_tempo_marker(
                    low,
                    ReaperProjectContext::CurrentProject,
                    index as i32,
                    m.timepos,
                    m.measurepos,
                    m.beatpos,
                    m.bpm,
                    numerator,
                    denominator,
                    m.lineartempo,
                );
            }
        });
    }

    async fn move_tempo_point(&self, _project: ProjectContext, index: u32, seconds: f64) {
        debug!(
            "ReaperTempoMap: move_tempo_point index {} to {} seconds",
            index, seconds
        );
        main_thread::run(move || {
            let low = Reaper::get().medium_reaper().low();

            if let Some(m) =
                sw::get_tempo_marker(low, ReaperProjectContext::CurrentProject, index as i32)
            {
                sw::set_tempo_marker(
                    low,
                    ReaperProjectContext::CurrentProject,
                    index as i32,
                    seconds, // new position
                    -1,      // auto measure
                    -1.0,    // auto beat
                    m.bpm,
                    m.timesig_num,
                    m.timesig_denom,
                    m.lineartempo,
                );
            }
        });
    }

    // =========================================================================
    // Project Defaults
    // =========================================================================

    async fn get_default_tempo(&self, _project: ProjectContext) -> f64 {
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper.current_project().tempo().bpm().get()
        })
        .await
        .unwrap_or(120.0)
    }

    async fn set_default_tempo(&self, _project: ProjectContext, bpm: f64) {
        debug!("ReaperTempoMap: set_default_tempo to {} BPM", bpm);
        main_thread::run(move || {
            let reaper = Reaper::get();
            if let Ok(bpm_value) = reaper_medium::Bpm::new(bpm) {
                let tempo = reaper_high::Tempo::from_bpm(bpm_value);
                let _ = reaper
                    .current_project()
                    .set_tempo(tempo, reaper_medium::UndoBehavior::OmitUndoPoint);
            }
        });
    }

    async fn get_default_time_signature(&self, _project: ProjectContext) -> (i32, i32) {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let measure_info =
                medium.time_map_get_measure_info(ReaperProjectContext::CurrentProject, 0);
            (
                measure_info.time_signature.numerator.get() as i32,
                measure_info.time_signature.denominator.get() as i32,
            )
        })
        .await
        .unwrap_or((4, 4))
    }

    async fn set_default_time_signature(
        &self,
        _project: ProjectContext,
        numerator: i32,
        denominator: i32,
    ) {
        debug!(
            "ReaperTempoMap: set_default_time_signature to {}/{}",
            numerator, denominator
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // Get tempo at position 0
            let bpm = reaper.current_project().tempo().bpm().get();

            // Check if there's already a marker at position 0
            let count = reaper
                .medium_reaper()
                .count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);

            let mut found_at_zero = false;
            for i in 0..count {
                if let Some(m) =
                    sw::get_tempo_marker(low, ReaperProjectContext::CurrentProject, i as i32)
                {
                    if m.timepos < 0.001 {
                        // Update existing marker at position 0
                        sw::set_tempo_marker(
                            low,
                            ReaperProjectContext::CurrentProject,
                            i as i32,
                            0.0,
                            0,
                            0.0,
                            m.bpm,
                            numerator,
                            denominator,
                            m.lineartempo,
                        );
                        found_at_zero = true;
                        break;
                    }
                }
            }

            if !found_at_zero {
                // Add new marker at position 0
                sw::set_tempo_marker(
                    low,
                    ReaperProjectContext::CurrentProject,
                    -1, // add new
                    0.0,
                    0,
                    0.0,
                    bpm,
                    numerator,
                    denominator,
                    false,
                );
            }
        });
    }

    // =========================================================================
    // Subscriptions
    // =========================================================================

    async fn subscribe_tempo_map(&self, _project: ProjectContext, tx: Tx<TempoMapEvent>) {
        info!("ReaperTempoMap: subscribe_tempo_map - subscribing to broadcast channel");

        let Some(mut rx) = tempo_map_receiver() else {
            info!(
                "ReaperTempoMap: broadcast channel not initialized, subscriber will not receive updates"
            );
            return;
        };

        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = tx.send(event).await {
                            debug!("ReaperTempoMap: subscribe_tempo_map stream closed: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!(
                            "ReaperTempoMap: subscribe_tempo_map lagged by {} messages",
                            count
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("ReaperTempoMap: broadcast channel closed");
                        break;
                    }
                }
            }

            info!("ReaperTempoMap: subscribe_tempo_map stream ended");
        });
    }
}
