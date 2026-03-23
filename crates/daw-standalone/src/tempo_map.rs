//! Standalone tempo map implementation
//!
//! In-memory tempo/time signature management with mock data.

use crate::platform::RwLock;
use daw_proto::{
    Position, ProjectContext, TimePosition, TimeSignature,
    tempo_map::{TempoMapEvent, TempoMapService, TempoPoint},
};
use std::sync::Arc;
use tracing::debug;
use vox::Tx;

/// Internal tempo map state
#[derive(Clone)]
struct TempoMapState {
    tempo_points: Vec<TempoPoint>,
    default_tempo: f64,
    default_time_sig: (i32, i32),
}

impl Default for TempoMapState {
    fn default() -> Self {
        Self {
            tempo_points: create_mock_tempo_points(),
            default_tempo: 120.0,
            default_time_sig: (4, 4),
        }
    }
}

/// Create mock tempo points with some tempo/time signature changes
fn create_mock_tempo_points() -> Vec<TempoPoint> {
    vec![
        // Song 1 start - 120 BPM, 4/4
        TempoPoint::with_time_signature(
            Position::from_time(TimePosition::from_seconds(0.0)),
            120.0,
            TimeSignature::new(4, 4),
        ),
        // Bridge section - tempo drops to 110 BPM, 6/8
        TempoPoint::with_time_signature(
            Position::from_time(TimePosition::from_seconds(135.0)),
            110.0,
            TimeSignature::new(6, 8),
        ),
        // Final chorus - tempo increases to 125 BPM, back to 4/4
        TempoPoint::with_time_signature(
            Position::from_time(TimePosition::from_seconds(165.0)),
            125.0,
            TimeSignature::new(4, 4),
        ),
        // Song 2 start - 95 BPM, 3/4
        TempoPoint::with_time_signature(
            Position::from_time(TimePosition::from_seconds(250.0)),
            95.0,
            TimeSignature::new(3, 4),
        ),
        // Song 3 start - 80 BPM, 3/4
        TempoPoint::with_time_signature(
            Position::from_time(TimePosition::from_seconds(440.0)),
            80.0,
            TimeSignature::new(3, 4),
        ),
        // Song 3 solo - tempo picks up to 90 BPM
        TempoPoint::from_seconds(580.0, 90.0),
    ]
}

/// Standalone tempo map implementation with mock data
#[derive(Clone)]
pub struct StandaloneTempoMap {
    state: Arc<RwLock<TempoMapState>>,
}

impl Default for StandaloneTempoMap {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneTempoMap {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(
                "standalone-tempo-map-state",
                TempoMapState::default(),
            )),
        }
    }

    /// Get the tempo point at or before the given position
    fn get_active_tempo_point(points: &[TempoPoint], seconds: f64) -> Option<&TempoPoint> {
        points
            .iter()
            .filter(|p| p.position_seconds() <= seconds)
            .max_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap()
            })
    }

    /// Get the active time signature at a given position
    fn get_active_time_signature(
        points: &[TempoPoint],
        seconds: f64,
        default: (i32, i32),
    ) -> (i32, i32) {
        points
            .iter()
            .filter(|p| p.position_seconds() <= seconds && p.time_signature.is_some())
            .max_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap()
            })
            .and_then(|p| p.time_signature.as_ref())
            .map(|ts| (ts.numerator as i32, ts.denominator as i32))
            .unwrap_or(default)
    }

    /// Convert time to musical position using the tempo map
    fn time_to_musical_internal(
        points: &[TempoPoint],
        seconds: f64,
        default_tempo: f64,
        default_time_sig: (i32, i32),
    ) -> (i32, i32, f64) {
        // Find all tempo points up to this time
        let mut total_beats = 0.0;
        let mut prev_time = 0.0;
        let mut prev_tempo = default_tempo;
        let mut time_sig = default_time_sig;

        for point in points.iter().filter(|p| p.position_seconds() <= seconds) {
            let point_time = point.position_seconds();

            // Add beats from previous point to this point
            let seconds_per_beat = 60.0 / prev_tempo;
            total_beats += (point_time - prev_time) / seconds_per_beat;

            prev_time = point_time;
            prev_tempo = point.bpm;
            if let Some(ts) = &point.time_signature {
                time_sig = (ts.numerator as i32, ts.denominator as i32);
            }
        }

        // Add remaining beats from last point to target time
        let seconds_per_beat = 60.0 / prev_tempo;
        total_beats += (seconds - prev_time) / seconds_per_beat;

        // Convert total beats to measure/beat/fraction
        let beats_per_measure = time_sig.0 as f64;
        let measure = (total_beats / beats_per_measure).floor() as i32;
        let beat_in_measure = total_beats - (measure as f64 * beats_per_measure);
        let beat = beat_in_measure.floor() as i32;
        let fraction = beat_in_measure - beat as f64;

        (measure, beat, fraction)
    }

    /// Convert musical position to time using the tempo map
    fn musical_to_time_internal(
        points: &[TempoPoint],
        measure: i32,
        beat: i32,
        fraction: f64,
        default_tempo: f64,
        default_time_sig: (i32, i32),
    ) -> f64 {
        // Calculate target beats
        let time_sig = default_time_sig;
        let target_beats = measure as f64 * time_sig.0 as f64 + beat as f64 + fraction;

        // Walk through tempo points to find the time
        let mut total_beats = 0.0;
        let mut prev_time = 0.0;
        let mut prev_tempo = default_tempo;

        for point in points {
            let point_time = point.position_seconds();
            let seconds_per_beat = 60.0 / prev_tempo;
            let beats_at_point = total_beats + (point_time - prev_time) / seconds_per_beat;

            if beats_at_point >= target_beats {
                // Target is before this point
                let remaining_beats = target_beats - total_beats;
                return prev_time + remaining_beats * seconds_per_beat;
            }

            total_beats = beats_at_point;
            prev_time = point_time;
            prev_tempo = point.bpm;
        }

        // Target is after all points
        let remaining_beats = target_beats - total_beats;
        let seconds_per_beat = 60.0 / prev_tempo;
        prev_time + remaining_beats * seconds_per_beat
    }
}

impl TempoMapService for StandaloneTempoMap {
    async fn get_tempo_points(&self, _project: ProjectContext) -> Vec<TempoPoint> {
        self.state.read().await.tempo_points.clone()
    }

    async fn get_tempo_point(&self, _project: ProjectContext, index: u32) -> Option<TempoPoint> {
        let state = self.state.read().await;
        state.tempo_points.get(index as usize).cloned()
    }

    async fn tempo_point_count(&self, _project: ProjectContext) -> usize {
        self.state.read().await.tempo_points.len()
    }

    async fn get_tempo_at(&self, _project: ProjectContext, seconds: f64) -> f64 {
        let state = self.state.read().await;
        Self::get_active_tempo_point(&state.tempo_points, seconds)
            .map(|p| p.bpm)
            .unwrap_or(state.default_tempo)
    }

    async fn get_time_signature_at(&self, _project: ProjectContext, seconds: f64) -> (i32, i32) {
        let state = self.state.read().await;
        Self::get_active_time_signature(&state.tempo_points, seconds, state.default_time_sig)
    }

    async fn time_to_qn(&self, _project: ProjectContext, seconds: f64) -> f64 {
        let state = self.state.read().await;
        let points = &state.tempo_points;
        let mut total_qn = 0.0;
        let mut prev_time = 0.0;
        let mut prev_tempo = state.default_tempo;

        for point in points {
            let point_time = point.position_seconds();
            if point_time >= seconds {
                break;
            }
            let qn_in_segment = (point_time - prev_time) * prev_tempo / 60.0;
            total_qn += qn_in_segment;
            prev_time = point_time;
            prev_tempo = point.bpm;
        }

        total_qn + (seconds - prev_time) * prev_tempo / 60.0
    }

    async fn qn_to_time(&self, _project: ProjectContext, qn: f64) -> f64 {
        let state = self.state.read().await;
        let points = &state.tempo_points;
        let mut total_qn = 0.0;
        let mut prev_time = 0.0;
        let mut prev_tempo = state.default_tempo;

        for point in points {
            let point_time = point.position_seconds();
            let qn_at_point = total_qn + (point_time - prev_time) * prev_tempo / 60.0;
            if qn_at_point >= qn {
                break;
            }
            total_qn = qn_at_point;
            prev_time = point_time;
            prev_tempo = point.bpm;
        }

        prev_time + (qn - total_qn) * 60.0 / prev_tempo
    }

    async fn time_to_musical(&self, _project: ProjectContext, seconds: f64) -> (i32, i32, f64) {
        let state = self.state.read().await;
        Self::time_to_musical_internal(
            &state.tempo_points,
            seconds,
            state.default_tempo,
            state.default_time_sig,
        )
    }

    async fn musical_to_time(
        &self,
        _project: ProjectContext,
        measure: i32,
        beat: i32,
        fraction: f64,
    ) -> f64 {
        let state = self.state.read().await;
        Self::musical_to_time_internal(
            &state.tempo_points,
            measure,
            beat,
            fraction,
            state.default_tempo,
            state.default_time_sig,
        )
    }

    async fn add_tempo_point(&self, _project: ProjectContext, seconds: f64, bpm: f64) -> u32 {
        let mut state = self.state.write().await;
        let point = TempoPoint::from_seconds(seconds, bpm);
        state.tempo_points.push(point);
        state.tempo_points.sort_by(|a, b| {
            a.position_seconds()
                .partial_cmp(&b.position_seconds())
                .unwrap()
        });

        // Return index of new point
        let index = state
            .tempo_points
            .iter()
            .position(|p| (p.position_seconds() - seconds).abs() < 0.001)
            .unwrap_or(0) as u32;

        debug!(
            "Added tempo point at {} with {} BPM (index {})",
            seconds, bpm, index
        );
        index
    }

    async fn remove_tempo_point(&self, _project: ProjectContext, index: u32) {
        let mut state = self.state.write().await;
        if (index as usize) < state.tempo_points.len() {
            state.tempo_points.remove(index as usize);
            debug!("Removed tempo point at index {}", index);
        }
    }

    async fn set_tempo_at_point(&self, _project: ProjectContext, index: u32, bpm: f64) {
        let mut state = self.state.write().await;
        if let Some(point) = state.tempo_points.get_mut(index as usize) {
            point.bpm = bpm;
            debug!("Set tempo at index {} to {} BPM", index, bpm);
        }
    }

    async fn set_time_signature_at_point(
        &self,
        _project: ProjectContext,
        index: u32,
        numerator: i32,
        denominator: i32,
    ) {
        let mut state = self.state.write().await;
        if let Some(point) = state.tempo_points.get_mut(index as usize) {
            point.time_signature = Some(TimeSignature::new(numerator as u32, denominator as u32));
            debug!(
                "Set time signature at index {} to {}/{}",
                index, numerator, denominator
            );
        }
    }

    async fn move_tempo_point(&self, _project: ProjectContext, index: u32, seconds: f64) {
        let mut state = self.state.write().await;
        if let Some(point) = state.tempo_points.get_mut(index as usize) {
            point.position = Position::from_time(TimePosition::from_seconds(seconds));
            debug!("Moved tempo point {} to {}", index, seconds);
        }
        state.tempo_points.sort_by(|a, b| {
            a.position_seconds()
                .partial_cmp(&b.position_seconds())
                .unwrap()
        });
    }

    async fn get_default_tempo(&self, _project: ProjectContext) -> f64 {
        self.state.read().await.default_tempo
    }

    async fn set_default_tempo(&self, _project: ProjectContext, bpm: f64) {
        self.state.write().await.default_tempo = bpm;
        debug!("Set default tempo to {} BPM", bpm);
    }

    async fn get_default_time_signature(&self, _project: ProjectContext) -> (i32, i32) {
        self.state.read().await.default_time_sig
    }

    async fn set_default_time_signature(
        &self,
        _project: ProjectContext,
        numerator: i32,
        denominator: i32,
    ) {
        self.state.write().await.default_time_sig = (numerator, denominator);
        debug!(
            "Set default time signature to {}/{}",
            numerator, denominator
        );
    }

    async fn subscribe_tempo_map(&self, _project: ProjectContext, _tx: Tx<TempoMapEvent>) {}
}
