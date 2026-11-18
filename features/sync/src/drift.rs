//! Drift correction for PeerMesh transport sync.
//!
//! Uses proportional control to adjust playback rate based on the position
//! difference between master and follower. At 5Hz heartbeat rate, the
//! semitone-nudge approach from the Link engine (designed for 30Hz) causes
//! runaway oscillation. Instead, we compute a playrate proportional to the
//! drift, which naturally converges without overshooting.
//!
//! The [`DriftCorrector`] is host-agnostic: it takes a position difference and
//! returns a [`DriftAction`] describing what the caller should do.

use tracing::debug;

// ── Rolling Average ─────────────────────────────────────────────────────────

/// Fixed-size circular buffer with interquartile-mean smoothing.
///
/// Port of ReaBlink's `RollingAverage.hpp`. Maintains a bounded window of
/// samples and returns the trimmed mean (drops the lowest and highest
/// quarters before averaging), which rejects outliers from timer jitter
/// and scheduling noise.
pub struct RollingAverage {
    values: Vec<f64>,
    max_size: usize,
    /// Write cursor — next position to overwrite in the circular buffer.
    cursor: usize,
    /// Number of values added so far (saturates at `max_size`).
    len: usize,
}

impl RollingAverage {
    /// Create a new rolling average with the given window size.
    pub fn new(size: usize) -> Self {
        assert!(size > 0, "RollingAverage size must be > 0");
        Self {
            values: vec![0.0; size],
            max_size: size,
            cursor: 0,
            len: 0,
        }
    }

    /// Push a new sample into the buffer, evicting the oldest if full.
    pub fn add(&mut self, value: f64) {
        self.values[self.cursor] = value;
        self.cursor = (self.cursor + 1) % self.max_size;
        if self.len < self.max_size {
            self.len += 1;
        }
    }

    /// Interquartile mean of the buffered samples.
    ///
    /// Sorts a copy of the live window, trims the bottom and top quarters,
    /// and averages the remaining middle half. Returns `0.0` when empty.
    pub fn average(&self) -> f64 {
        if self.len == 0 {
            return 0.0;
        }

        let mut sorted: Vec<f64> = self.values[..self.len].to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let quarter = sorted.len() / 4;
        let trimmed = &sorted[quarter..sorted.len() - quarter];
        if trimmed.is_empty() {
            // Fewer than 4 samples — just average everything.
            return sorted.iter().sum::<f64>() / sorted.len() as f64;
        }
        trimmed.iter().sum::<f64>() / trimmed.len() as f64
    }

    /// Number of samples currently in the buffer.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reset the buffer, clearing all samples.
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.len = 0;
    }
}

// ── Drift Correction ────────────────────────────────────────────────────────

/// Maximum playrate correction away from 1.0.
/// Keeps playrate in [1 - MAX, 1 + MAX] = [0.85, 1.15].
/// 15% is inaudible for tempo-nudge; 50% was perceptible during live playback.
const MAX_RATE_CORRECTION: f64 = 0.15;

/// Proportional gain — how aggressively to correct.
///
/// With gain=1.5 and 30Hz heartbeat: a 50ms drift produces 7.5% playrate
/// correction, converging in ~0.66s. Gentler than 2.0 to avoid audible pumping.
const PROPORTIONAL_GAIN: f64 = 1.5;

/// Action to take for drift correction.
#[derive(Debug, Clone, PartialEq)]
pub enum DriftAction {
    /// No correction needed — drift within tolerance and playrate is normal.
    None,
    /// Set playrate to the given value to correct drift.
    SetRate { new_playrate: f64 },
    /// Reset playrate to 1.0 (drift within tolerance, playrate was nudged).
    Reset,
    /// Hard seek to master position (drift exceeds hard-seek threshold).
    HardSeek,
}

/// Drift corrector using proportional control.
///
/// Instead of fixed semitone nudges (which compound and oscillate at 5Hz),
/// computes a playrate proportional to the drift. Larger drift = faster
/// correction, and the correction naturally decreases as positions converge.
///
/// ```text
/// playrate = 1.0 + clamp(drift * gain, -max, +max)
/// ```
pub struct DriftCorrector {
    /// Tolerance in seconds. Drift below this is considered "in sync".
    tolerance: f64,
    /// Hard seek threshold in seconds.
    hard_seek_threshold: f64,
    /// Smoothing filter for raw drift measurements (window=4, ~133ms at 30Hz).
    /// Smaller than Link's window=8 to avoid phase lag in the control loop;
    /// 4 samples is enough to reject single-tick jitter without destabilizing
    /// the proportional controller.
    drift_avg: RollingAverage,
}

impl DriftCorrector {
    /// Create a new drift corrector with default thresholds.
    ///
    /// - Tolerance: 10ms
    /// - Hard seek: 0.5s
    pub fn new() -> Self {
        Self {
            tolerance: 0.01,
            // Hard-seek when drift exceeds 1.0s. Proportional control at
            // gain=1.5 converges drifts up to ~500ms in a few seconds.
            // Above 1.0s, something is fundamentally wrong (seek, tempo
            // change, etc.) and a hard seek is more efficient.
            hard_seek_threshold: 1.0,
            drift_avg: RollingAverage::new(4),
        }
    }

    /// Create a drift corrector with custom thresholds.
    pub fn with_thresholds(tolerance: f64, hard_seek_threshold: f64) -> Self {
        Self {
            tolerance,
            hard_seek_threshold,
            drift_avg: RollingAverage::new(4),
        }
    }

    /// Feed a position difference and get the correction action.
    ///
    /// # Arguments
    /// - `drift_seconds`: `remote_position - local_position`. Positive means
    ///   local is behind the master (needs to speed up).
    /// - `current_playrate`: current playback rate (1.0 = normal speed).
    ///
    /// # Returns
    /// A [`DriftAction`] describing what the caller should do.
    pub fn correct(&mut self, drift_seconds: f64, current_playrate: f64) -> DriftAction {
        let abs_drift = drift_seconds.abs();

        // Way too far off — hard seek (check raw drift for immediate response)
        if abs_drift > self.hard_seek_threshold {
            debug!("Drift {drift_seconds:.3}s exceeds hard-seek threshold");
            self.drift_avg.reset(); // stale after seek
            return DriftAction::HardSeek;
        }

        // Feed raw measurement into the smoothing filter
        self.drift_avg.add(drift_seconds);
        let smoothed = self.drift_avg.average();
        let abs_smoothed = smoothed.abs();

        if abs_smoothed > self.tolerance {
            // Proportional correction using smoothed drift
            let correction =
                (smoothed * PROPORTIONAL_GAIN).clamp(-MAX_RATE_CORRECTION, MAX_RATE_CORRECTION);
            let new_rate = 1.0 + correction;
            debug!(
                "Drift correction: raw={drift_seconds:.4}s smoothed={smoothed:.4}s, playrate {current_playrate:.4} → {new_rate:.4}"
            );
            DriftAction::SetRate {
                new_playrate: new_rate,
            }
        } else if (current_playrate - 1.0).abs() > 0.001 {
            // Within tolerance but playrate was previously adjusted — reset
            debug!("Drift within tolerance (smoothed={smoothed:.4}s), resetting playrate");
            DriftAction::Reset
        } else {
            DriftAction::None
        }
    }

    /// Get the hard-seek threshold in seconds.
    pub fn hard_seek_threshold(&self) -> f64 {
        self.hard_seek_threshold
    }

    /// Reset the corrector state (e.g., on transport stop/start).
    pub fn reset(&mut self) {
        self.drift_avg.reset();
    }
}

impl Default for DriftCorrector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────
