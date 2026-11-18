//! Time position utilities for converting between time and musical positions
//!
//! This module provides functions to convert time positions (in seconds) to
//! musical positions in the format Measure:Beat:SubBeat (e.g., "3.2.25")
//! using tempo and time signature data from REAPER projects.

use crate::types::time_tempo::{TempoTimeEnvelope, TempoTimePoint};

/// Convert a time position to musical position in Measure:Beat:SubBeat format
///
/// # Arguments
/// * `time_position` - Position in seconds
/// * `tempo_points` - Vector of tempo/time signature change points
/// * `default_tempo` - Default tempo in BPM
/// * `default_time_signature` - Default time signature as (numerator, denominator)
///
/// # Returns
/// String in format "Measure.Beat.SubBeat" (e.g., "3.2.25")
pub fn time_to_beat_position(
    time_position: f64,
    tempo_points: &[TempoTimePoint],
    default_tempo: f64,
    default_time_signature: (i32, i32),
) -> String {
    let (measure, beat, subbeat) = time_to_beat_position_structured(
        time_position,
        tempo_points,
        default_tempo,
        default_time_signature,
    );

    format!("{}.{}.{:02}", measure, beat, subbeat)
}

/// Convert a time position to musical position as structured data
///
/// # Arguments
/// * `time_position` - Position in seconds
/// * `tempo_points` - Vector of tempo/time signature change points
/// * `default_tempo` - Default tempo in BPM
/// * `default_time_signature` - Default time signature as (numerator, denominator)
///
/// # Returns
/// Tuple of (measure, beat, subbeat) where subbeat is in hundredths
pub fn time_to_beat_position_structured(
    time_position: f64,
    tempo_points: &[TempoTimePoint],
    default_tempo: f64,
    default_time_signature: (i32, i32),
) -> (i32, i32, i32) {
    if tempo_points.is_empty() {
        // No tempo changes, simple calculation with effective tempo
        let tempo_ratio = default_time_signature.1 as f64 / 4.0;
        let effective_tempo = default_tempo * tempo_ratio;
        let total_beats = time_position * effective_tempo / 60.0;
        let beats_per_measure = default_time_signature.0 as f64;

        let measure = (total_beats / beats_per_measure).floor() as i32 + 1;
        let beat_in_measure = ((total_beats - 1.0) % beats_per_measure + 1.0) as i32;
        let beat_fraction =
            total_beats - (measure - 1) as f64 * beats_per_measure - (beat_in_measure - 1) as f64;
        let subbeat = (beat_fraction * 100.0).round() as i32;

        return (measure, beat_in_measure, subbeat);
    }

    // Need to account for tempo and time signature changes throughout the song
    let mut last_time = 0.0;
    let mut current_tempo = default_tempo;
    let mut current_time_sig = default_time_signature;
    let mut current_measure = 1.0; // Track measures as we go

    // Add bounds checking to prevent overflow
    let max_measures = 1000.0;

    for point in tempo_points {
        if point.position <= time_position {
            // Add beats for the time segment before this point
            let segment_duration = point.position - last_time;

            // Calculate effective tempo based on time signature
            let tempo_ratio = current_time_sig.1 as f64 / 4.0;
            let effective_tempo = current_tempo * tempo_ratio;

            let segment_beats = segment_duration * effective_tempo / 60.0;

            // Calculate how many measures this segment represents
            let beats_per_measure = current_time_sig.0 as f64;
            let segment_measures = segment_beats / beats_per_measure;
            current_measure += segment_measures;

            // Prevent overflow
            if current_measure > max_measures {
                current_measure = max_measures;
            }

            // Update for next segment
            last_time = point.position;
            current_tempo = point.tempo;
            if let Some(time_sig) = point.time_signature() {
                current_time_sig = time_sig;
            }
        } else {
            // This point is after our target time, add final segment
            let segment_duration = time_position - last_time;

            // Calculate effective tempo based on current time signature
            let tempo_ratio = current_time_sig.1 as f64 / 4.0;
            let effective_tempo = current_tempo * tempo_ratio;

            let segment_beats = segment_duration * effective_tempo / 60.0;

            // Calculate final segment measures
            let beats_per_measure = current_time_sig.0 as f64;
            let segment_measures = segment_beats / beats_per_measure;
            current_measure += segment_measures;

            // Prevent overflow
            if current_measure > max_measures {
                current_measure = max_measures;
            }

            // Calculate final position with bounds checking
            let measure = (current_measure.floor() as i32 + 1).clamp(1, 1000);
            let beat_in_measure =
                ((current_measure - current_measure.floor()) * beats_per_measure + 1.0) as i32;
            let beat_in_measure = beat_in_measure.clamp(1, beats_per_measure as i32);
            let beat_fraction = (current_measure - current_measure.floor()) * beats_per_measure;
            let subbeat = (beat_fraction * 100.0).round() as i32;
            let subbeat = subbeat.clamp(0, 99);

            return (measure, beat_in_measure, subbeat);
        }
    }

    // Add final segment if we haven't reached the target time yet
    if last_time < time_position {
        let segment_duration = time_position - last_time;

        // Calculate effective tempo based on current time signature
        let tempo_ratio = current_time_sig.1 as f64 / 4.0;
        let effective_tempo = current_tempo * tempo_ratio;

        let segment_beats = segment_duration * effective_tempo / 60.0;

        let beats_per_measure = current_time_sig.0 as f64;
        let segment_measures = segment_beats / beats_per_measure;
        current_measure += segment_measures;

        // Prevent overflow
        if current_measure > max_measures {
            current_measure = max_measures;
        }
    }

    // Calculate final position with bounds checking
    let measure = (current_measure.floor() as i32 + 1).clamp(1, 1000);
    let beat_in_measure =
        ((current_measure - current_measure.floor()) * current_time_sig.0 as f64 + 1.0) as i32;
    let beat_in_measure = beat_in_measure.clamp(1, current_time_sig.0);
    let beat_fraction = (current_measure - current_measure.floor()) * current_time_sig.0 as f64;
    let subbeat = (beat_fraction * 100.0).round() as i32;
    let subbeat = subbeat.clamp(0, 99);

    (measure, beat_in_measure, subbeat)
}

/// Convert a time position using a TempoTimeEnvelope
///
/// # Arguments
/// * `time_position` - Position in seconds
/// * `tempo_envelope` - TempoTimeEnvelope containing all tempo/time signature data
///
/// # Returns
/// String in format "Measure.Beat.SubBeat" (e.g., "3.2.25")
pub fn time_to_beat_position_with_envelope(
    time_position: f64,
    tempo_envelope: &TempoTimeEnvelope,
) -> String {
    // Use the existing musical_position_at_time method from TempoTimeEnvelope
    // which properly handles time signature changes
    let (measure, beat, fraction) = tempo_envelope.musical_position_at_time(time_position);

    // Convert fraction to subbeat (hundredths)
    let subbeat = (fraction * 100.0).round() as i32;

    format!("{}.{}.{:02}", measure, beat, subbeat)
}

/// Convert a time position using a TempoTimeEnvelope (structured version)
///
/// # Arguments
/// * `time_position` - Position in seconds
/// * `tempo_envelope` - TempoTimeEnvelope containing all tempo/time signature data
///
/// # Returns
/// Tuple of (measure, beat, subbeat) where subbeat is in hundredths
pub fn time_to_beat_position_structured_with_envelope(
    time_position: f64,
    tempo_envelope: &TempoTimeEnvelope,
) -> (i32, i32, i32) {
    // Use the existing musical_position_at_time method from TempoTimeEnvelope
    // which properly handles time signature changes
    let (measure, beat, fraction) = tempo_envelope.musical_position_at_time(time_position);

    // Convert fraction to subbeat (hundredths)
    let subbeat = (fraction * 100.0).round() as i32;

    (measure, beat, subbeat)
}
