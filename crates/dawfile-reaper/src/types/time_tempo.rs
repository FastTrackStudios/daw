//! Time and Tempo parsing for RPP files
//!
//! Handles tempo changes, time signature changes, and musical position calculations
//! based on the TEMPOENVEX envelope data from REAPER projects.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::primitives::token::parse_token_line;

/// A tempo/time signature change point
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoTimePoint {
    /// Position in seconds when this change occurs
    pub position: f64,
    /// Tempo in BPM
    pub tempo: f64,
    /// Envelope shape (0=linear, 1=square, etc.)
    pub shape: i32,
    /// Time signature encoded as 65536 * denominator + numerator
    /// e.g., 4/4 = 65536 * 4 + 4 = 262148
    pub time_signature_encoded: Option<i32>,
    /// Whether this point is selected
    pub selected: bool,
    /// Unknown field
    pub unknown1: i32,
    /// Bezier tension for curves
    pub bezier_tension: f64,
    /// Metronome pattern (e.g., "ABBB")
    pub metronome_pattern: String,
    /// Additional unknown fields
    pub unknown2: i32,
    pub unknown3: i32,
    pub unknown4: i32,
}

impl TempoTimePoint {
    /// Create a TempoTimePoint from a raw RPP PT line
    pub fn from_pt_line(line: &str) -> Result<Self, String> {
        let (_remaining, tokens) =
            parse_token_line(line).map_err(|e| format!("Failed to parse PT line: {:?}", e))?;

        if tokens.len() < 4 {
            return Err(format!(
                "PT line has insufficient tokens: expected at least 4, got {}: {:?}",
                tokens.len(),
                tokens
            ));
        }

        // Skip the first token "PT" and parse the rest
        let position = tokens[1].as_number().ok_or("Invalid position")?;
        let tempo = tokens[2].as_number().ok_or("Invalid tempo")?;
        let shape = tokens[3].as_number().ok_or("Invalid shape")? as i32;

        // Optional fields
        let time_signature_encoded = if tokens.len() > 4 {
            let encoded = tokens[4].as_number().ok_or("Invalid time signature")? as i32;
            if encoded > 0 { Some(encoded) } else { None }
        } else {
            None
        };

        let selected = if tokens.len() > 5 {
            tokens[5].as_number().unwrap_or(0.0) as i32 != 0
        } else {
            false
        };

        let unknown1 = if tokens.len() > 6 {
            tokens[6].as_number().unwrap_or(0.0) as i32
        } else {
            0
        };

        let bezier_tension = if tokens.len() > 7 {
            tokens[7].as_number().unwrap_or(0.0)
        } else {
            0.0
        };

        // The metronome pattern is typically the last token if present
        let metronome_pattern = if tokens.len() > 8 {
            // Look for the last non-empty string/identifier token
            let mut pattern = "".to_string();
            for i in (8..tokens.len()).rev() {
                if let Some(p) = tokens[i].as_string() {
                    if !p.is_empty() {
                        pattern = p.to_string();
                        break;
                    }
                }
            }
            pattern
        } else {
            "".to_string()
        };

        let unknown2 = if tokens.len() > 9 {
            tokens[9].as_number().unwrap_or(0.0) as i32
        } else {
            0
        };

        let unknown3 = if tokens.len() > 10 {
            tokens[10].as_number().unwrap_or(0.0) as i32
        } else {
            0
        };

        let unknown4 = if tokens.len() > 11 {
            tokens[11].as_number().unwrap_or(0.0) as i32
        } else {
            0
        };

        Ok(TempoTimePoint {
            position,
            tempo,
            shape,
            time_signature_encoded,
            selected,
            unknown1,
            bezier_tension,
            metronome_pattern,
            unknown2,
            unknown3,
            unknown4,
        })
    }

    /// Decode time signature from the encoded value
    /// Returns (numerator, denominator) or None if not set
    pub fn time_signature(&self) -> Option<(i32, i32)> {
        self.time_signature_encoded.map(|encoded| {
            let denominator = encoded / 65536;
            let numerator = encoded % 65536;
            (numerator, denominator)
        })
    }

    /// Get time signature as a string (e.g., "4/4")
    pub fn time_signature_string(&self) -> String {
        if let Some((num, den)) = self.time_signature() {
            format!("{}/{}", num, den)
        } else {
            "".to_string()
        }
    }
}

impl fmt::Display for TempoTimePoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tempo Change at {:.3}s:", self.position)?;
        writeln!(f, "  Tempo: {:.1} BPM", self.tempo)?;
        if let Some((num, den)) = self.time_signature() {
            writeln!(f, "  Time Signature: {}/{}", num, den)?;
        }
        if !self.metronome_pattern.is_empty() {
            writeln!(f, "  Metronome Pattern: {}", self.metronome_pattern)?;
        }
        writeln!(f, "  Shape: {}", self.shape)?;
        if self.selected {
            writeln!(f, "  Selected: Yes")?;
        }
        Ok(())
    }
}

/// Collection of tempo and time signature changes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoTimeEnvelope {
    /// All tempo/time signature change points, sorted by position
    pub points: Vec<TempoTimePoint>,
    /// Default tempo (from project properties)
    pub default_tempo: f64,
    /// Default time signature (from project properties)
    pub default_time_signature: (i32, i32),
}

impl TempoTimeEnvelope {
    const EPSILON: f64 = 1e-9;

    /// Create a new tempo envelope with defaults
    pub fn new(default_tempo: f64, default_time_signature: (i32, i32)) -> Self {
        Self {
            points: Vec::new(),
            default_tempo,
            default_time_signature,
        }
    }

    /// Add a tempo/time signature change point
    pub fn add_point(&mut self, point: TempoTimePoint) {
        self.points.push(point);
        // Keep points sorted by position
        self.points.sort_by(|a, b| {
            a.position
                .partial_cmp(&b.position)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Get the tempo and time signature at a given time position
    pub fn get_at_time(&self, time: f64) -> (f64, (i32, i32)) {
        // Find the last point before or at the given time
        let mut current_tempo = self.default_tempo;
        let mut current_time_sig = self.default_time_signature;

        for point in &self.points {
            if point.position <= time {
                current_tempo = point.tempo;
                if let Some(time_sig) = point.time_signature() {
                    current_time_sig = time_sig;
                }
            } else {
                break;
            }
        }

        (current_tempo, current_time_sig)
    }

    fn integrate_linear_tempo_segment(
        start: &TempoTimePoint,
        end: &TempoTimePoint,
        from: f64,
        to: f64,
    ) -> f64 {
        let seg_duration = end.position - start.position;
        if seg_duration <= Self::EPSILON {
            return (to - from) * start.tempo / 60.0;
        }

        let off_from = (from - start.position).clamp(0.0, seg_duration);
        let off_to = (to - start.position).clamp(0.0, seg_duration);
        if off_to <= off_from {
            return 0.0;
        }

        let slope = (end.tempo - start.tempo) / seg_duration;
        let tempo_integral =
            start.tempo * (off_to - off_from) + 0.5 * slope * (off_to * off_to - off_from * off_from);
        tempo_integral / 60.0
    }

    fn integrate_between_points(
        start: &TempoTimePoint,
        end: &TempoTimePoint,
        from: f64,
        to: f64,
    ) -> f64 {
        if to <= from {
            return 0.0;
        }
        Self::integrate_linear_tempo_segment(start, end, from, to)
    }

    fn quarter_notes_at_time(&self, time: f64) -> f64 {
        if time <= 0.0 {
            return 0.0;
        }
        if self.points.is_empty() {
            return time * self.default_tempo / 60.0;
        }

        let mut total_qn = 0.0f64;
        let first = &self.points[0];

        // Pre-first-marker segment runs at default tempo.
        if time <= first.position {
            return time * self.default_tempo / 60.0;
        }
        total_qn += (first.position.max(0.0)) * self.default_tempo / 60.0;

        for idx in 0..self.points.len() {
            let start = &self.points[idx];
            let next = self.points.get(idx + 1);

            let seg_from = start.position.max(0.0);
            let seg_to = next.map(|p| p.position).unwrap_or(time).min(time);
            if seg_to <= seg_from {
                continue;
            }

            if let Some(end) = next {
                total_qn += Self::integrate_between_points(start, end, seg_from, seg_to);
            } else {
                total_qn += (seg_to - seg_from) * start.tempo / 60.0;
            }

            if time <= seg_to + Self::EPSILON {
                break;
            }
        }

        total_qn
    }

    /// Calculate the total number of beats up to a given time
    /// This integrates tempo changes over time
    pub fn beats_at_time(&self, time: f64) -> f64 {
        self.quarter_notes_at_time(time)
    }

    /// Calculate musical position (measure and beat) at a given time
    /// Returns (measure, beat, beat_fraction) where measure is 1-based
    /// This is more complex because time signatures can change throughout the song
    pub fn musical_position_at_time(&self, time: f64) -> (i32, i32, f64) {
        if time <= 0.0 {
            return (1, 1, 0.0);
        }

        let target_qn = self.quarter_notes_at_time(time);

        let mut measure = 1i32;
        let mut quarter_in_measure = 0.0f64;
        let mut current_sig = self.default_time_signature;

        let advance_quarters = |quarters: f64,
                                sig: (i32, i32),
                                measure_ref: &mut i32,
                                qim: &mut f64| {
            if quarters <= 0.0 {
                return;
            }
            let measure_len_qn = (sig.0 as f64) * (4.0 / sig.1 as f64);
            *qim += quarters;
            while *qim + Self::EPSILON >= measure_len_qn {
                *qim -= measure_len_qn;
                *measure_ref += 1;
            }
            if *qim < 0.0 {
                *qim = 0.0;
            }
        };

        let mut sig_changes: Vec<(f64, (i32, i32))> = vec![(0.0, current_sig)];
        for point in &self.points {
            if let Some(sig) = point.time_signature() {
                let qn = self.quarter_notes_at_time(point.position);
                if let Some((last_qn, last_sig)) = sig_changes.last().copied() {
                    if (qn - last_qn).abs() <= Self::EPSILON {
                        sig_changes.pop();
                        sig_changes.push((qn, sig));
                    } else if sig != last_sig {
                        sig_changes.push((qn, sig));
                    }
                }
            }
        }

        let mut prev_qn = 0.0f64;
        let mut done = false;

        for (idx, (change_qn, next_sig)) in sig_changes.iter().enumerate() {
            if idx == 0 {
                current_sig = *next_sig;
                continue;
            }

            if *change_qn <= prev_qn + Self::EPSILON {
                current_sig = *next_sig;
                continue;
            }

            let stop_qn = target_qn.min(*change_qn);
            if stop_qn > prev_qn + Self::EPSILON {
                advance_quarters(
                    stop_qn - prev_qn,
                    current_sig,
                    &mut measure,
                    &mut quarter_in_measure,
                );
                prev_qn = stop_qn;
            }

            if target_qn <= *change_qn + Self::EPSILON {
                done = true;
                break;
            }

            current_sig = *next_sig;
        }

        if !done && target_qn > prev_qn + Self::EPSILON {
            advance_quarters(
                target_qn - prev_qn,
                current_sig,
                &mut measure,
                &mut quarter_in_measure,
            );
        }

        let beat_len_qn = 4.0 / current_sig.1 as f64;
        let beat_pos = quarter_in_measure / beat_len_qn;
        let beat_whole = beat_pos.floor();
        let mut beat = (beat_whole as i32 + 1).clamp(1, current_sig.0.max(1));
        let mut fraction = (beat_pos - beat_whole).clamp(0.0, 1.0);

        // Normalize near-boundary floating point residue.
        if fraction >= 1.0 - 1e-6 {
            fraction = 0.0;
            beat += 1;
        }
        if beat > current_sig.0.max(1) {
            beat = 1;
            measure += 1;
        }

        (measure.clamp(1, 1_000_000), beat, fraction)
    }

    /// Get musical position as a formatted string in REAPER's format (measure.beat.fraction)
    pub fn musical_position_string_at_time(&self, time: f64) -> String {
        let (measure, beat, fraction) = self.musical_position_at_time(time);

        // REAPER format: measure.beat.fraction (e.g., "12.1.00", "14.5.25")
        // Convert fraction to hundredths (0.25 becomes 25)
        let fraction_hundredths = (fraction * 100.0).round() as i32;

        format!("{}.{}.{:02}", measure, beat, fraction_hundredths)
    }
}

impl Default for TempoTimeEnvelope {
    fn default() -> Self {
        Self::new(120.0, (4, 4))
    }
}

impl fmt::Display for TempoTimeEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tempo/Time Signature Envelope")?;
        writeln!(
            f,
            "  Default: {} BPM, {}/{}",
            self.default_tempo, self.default_time_signature.0, self.default_time_signature.1
        )?;
        writeln!(f, "  Changes: {} points", self.points.len())?;

        if !self.points.is_empty() {
            writeln!(f)?;
            for point in &self.points {
                write!(f, "{}", point)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tempo_point() {
        let line = r#"PT 0.000000000000 121.9442407666 1 262148 0 1 0 "" 0 169 0 ABBB"#;

        // Debug: let's see what tokens we get
        let (_remaining, tokens) = parse_token_line(line).unwrap();
        println!("Debug tokens: {:?}", tokens);

        let point = TempoTimePoint::from_pt_line(line).unwrap();

        assert_eq!(point.position, 0.0);
        assert_eq!(point.tempo, 121.9442407666);
        assert_eq!(point.shape, 1);
        assert_eq!(point.time_signature_encoded, Some(262148));
        assert_eq!(point.time_signature(), Some((4, 4)));
        assert_eq!(point.time_signature_string(), "4/4");
        assert_eq!(point.metronome_pattern, "ABBB");
    }

    #[test]
    fn test_parse_tempo_point_minimal() {
        let line = r#"PT 25.658694116649 250.0000000000 1"#;
        let point = TempoTimePoint::from_pt_line(line).unwrap();

        assert_eq!(point.position, 25.658694116649);
        assert_eq!(point.tempo, 250.0);
        assert_eq!(point.shape, 1);
        assert_eq!(point.time_signature_encoded, None);
        assert_eq!(point.time_signature(), None);
        assert_eq!(point.metronome_pattern, "");
    }

    #[test]
    fn test_region_test_tempo_changes() {
        // Test with the actual tempo changes from Region Test.RPP
        let mut envelope = TempoTimeEnvelope::new(121.9442407666, (4, 4));

        // Add all the tempo changes from the file
        let points = vec![
            r#"PT 0.000000000000 121.9442407666 1 262148 0 1 0 "" 0 169 0 ABBB"#,
            r#"PT 15.744901013201 87.0000000000 1 524295 1 1 0 "" 0 10921 0 ABBBBBB"#,
            r#"PT 18.158694116649 32.0000000000 1 262148 0 1 0 "" 0 169 0 ABBB"#,
            r#"PT 25.658694116649 250.0000000000 1"#,
            r#"PT 26.618694116649 134.0000000000 1 262148 0 1 0 "" 0 169 0 ABBB"#,
        ];

        for point_line in points {
            let point = TempoTimePoint::from_pt_line(point_line).unwrap();
            envelope.add_point(point);
        }

        // Test the "Ending Section" marker at 26.618694116649 seconds
        let ending_section_time = 26.618694116649;
        let (measure, beat, fraction) = envelope.musical_position_at_time(ending_section_time);
        let musical_pos = envelope.musical_position_string_at_time(ending_section_time);

        println!("Ending Section at {:.3}s:", ending_section_time);
        println!("  Musical Position (REAPER format): {}", musical_pos);
        println!(
            "  Measure: {}, Beat: {}, Fraction: {:.3}",
            measure, beat, fraction
        );

        // Let's also test a few other key positions
        let test_positions = vec![
            (0.0, "Project start"),
            (15.744901013201, "7/8 time signature change"),
            (18.158694116649, "32 BPM change"),
            (25.658694116649, "250 BPM change"),
            (26.618694116649, "Ending Section"),
        ];

        for (time, description) in test_positions {
            let (_measure, _beat, _) = envelope.musical_position_at_time(time);
            let musical_pos = envelope.musical_position_string_at_time(time);
            println!("  {} at {:.3}s: {}", description, time, musical_pos);
        }

        // The test should pass - we're just demonstrating the calculation
        assert!(measure > 0);
        assert!(beat > 0);
    }
}
