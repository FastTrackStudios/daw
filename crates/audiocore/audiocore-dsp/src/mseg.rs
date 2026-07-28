//! MSEG — multi-segment envelope patterns for rhythmic modulation.
//!
//! Clean-room implementation of the drawable-pattern idea popularized
//! by ShaperBox/GrossBeat-style tools: a list of points over one cycle
//! (x ∈ [0, 1)), each with a value, a curve tension into the next
//! point, and an optional **clear-tails** flag (the reverb-specific
//! trick: crossing the point hard-kills the tail). Patterns are
//! self-repeating — the last segment wraps to the first point, so the
//! start and end values need not match.
//!
//! Design constraint carried over from studying pattern-modulated
//! reverbs: MSEGs drive **gain-domain** signals (send/wet levels).
//! Continuously modulating IR or delay parameters clicks; levels ramp.

/// One pattern point.
#[derive(Debug, Clone, Copy)]
pub struct MsegPoint {
    /// Position in the cycle, [0, 1).
    pub x: f64,
    /// Value at the point, usually [0, 1].
    pub y: f64,
    /// Curve tension into the NEXT point: 0 = linear, positive bows
    /// toward the destination (fast-then-slow), negative away
    /// (slow-then-fast). Range about ±1.
    pub tension: f64,
    /// Step instead of curve: hold `y` until the next point.
    pub hold: bool,
    /// Crossing this point hard-clears the target's tail.
    pub clear_tails: bool,
}

impl MsegPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            tension: 0.0,
            hold: false,
            clear_tails: false,
        }
    }
}

/// A pattern: points sorted by `x` over one cycle.
#[derive(Debug, Clone, Default)]
pub struct Mseg {
    points: Vec<MsegPoint>,
}

impl Mseg {
    pub fn new(mut points: Vec<MsegPoint>) -> Self {
        points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(core::cmp::Ordering::Equal));
        for p in &mut points {
            p.x = p.x.rem_euclid(1.0);
        }
        Self { points }
    }

    /// Convenience: a square gate with `duty` on-fraction.
    pub fn gate(duty: f64) -> Self {
        let d = duty.clamp(0.01, 0.99);
        Self::new(vec![
            MsegPoint {
                x: 0.0,
                y: 1.0,
                tension: 0.0,
                hold: true,
                clear_tails: false,
            },
            MsegPoint {
                x: d,
                y: 0.0,
                tension: 0.0,
                hold: true,
                clear_tails: false,
            },
        ])
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn points(&self) -> &[MsegPoint] {
        &self.points
    }

    /// Evaluate at `phase` ∈ [0, 1).
    pub fn value(&self, phase: f64) -> f64 {
        let n = self.points.len();
        if n == 0 {
            return 1.0;
        }
        if n == 1 {
            return self.points[0].y;
        }
        let ph = phase.rem_euclid(1.0);
        // Find the segment [i, i+1) containing ph (wrapping).
        let mut i = n - 1; // default: before the first point → wrap segment
        for (k, p) in self.points.iter().enumerate() {
            if p.x <= ph {
                i = k;
            } else {
                break;
            }
        }
        let a = self.points[i];
        let b = self.points[(i + 1) % n];
        if a.hold {
            return a.y;
        }
        // Segment span with wraparound.
        let span = if (i + 1) % n == 0 || b.x <= a.x {
            (1.0 - a.x) + b.x
        } else {
            b.x - a.x
        };
        if span <= 1e-9 {
            return b.y;
        }
        let mut t = (ph - a.x).rem_euclid(1.0) / span;
        t = t.clamp(0.0, 1.0);
        // Tension curve: t^k with k = 2^(±3·tension).
        let k = (3.0 * a.tension.clamp(-1.0, 1.0)).exp2();
        let shaped = t.powf(k);
        a.y + (b.y - a.y) * shaped
    }

    /// Whether a `clear_tails` point lies in the phase interval
    /// (prev, now] (wrap-aware).
    pub fn clear_crossed(&self, prev_phase: f64, now_phase: f64) -> bool {
        let p0 = prev_phase.rem_euclid(1.0);
        let p1 = now_phase.rem_euclid(1.0);
        self.points.iter().any(|p| {
            if !p.clear_tails {
                return false;
            }
            if p0 <= p1 {
                p.x > p0 && p.x <= p1
            } else {
                // Wrapped around the cycle end.
                p.x > p0 || p.x <= p1
            }
        })
    }
}

/// Pattern playback: free-running (Hz) or tempo-synced (cycles per
/// beat count).
#[derive(Debug, Clone, Copy)]
pub enum MsegRate {
    /// Cycle frequency in Hz.
    FreeHz(f64),
    /// One cycle per `beats` beats (needs a tempo).
    SyncBeats(f64),
}

/// A playing MSEG lane: pattern + clock + depth + output smoothing.
#[derive(Debug, Clone)]
pub struct MsegLane {
    pub pattern: Mseg,
    pub rate: MsegRate,
    /// 0 = lane inert (output 1.0), 1 = full pattern depth.
    pub depth: f64,
    phase: f64,
    smoothed: f64,
    smooth_coeff: f64,
}

impl MsegLane {
    pub fn new(pattern: Mseg, rate: MsegRate) -> Self {
        Self {
            pattern,
            rate,
            depth: 1.0,
            phase: 0.0,
            smoothed: 1.0,
            smooth_coeff: 0.01,
        }
    }

    pub fn configure(&mut self, sample_rate: f64) {
        // ~2 ms output smoothing: gain moves ramp, never click.
        self.smooth_coeff = 1.0 - (-1.0 / (0.002 * sample_rate)).exp();
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.smoothed = 1.0;
    }

    /// Advance one sample; returns `(gain, clear_tails_crossed)`.
    #[inline]
    pub fn tick(&mut self, sample_rate: f64, tempo_bpm: Option<f64>) -> (f64, bool) {
        if self.depth <= 1e-9 || self.pattern.is_empty() {
            return (1.0, false);
        }
        let inc = match self.rate {
            MsegRate::FreeHz(hz) => hz.max(0.0) / sample_rate,
            MsegRate::SyncBeats(beats) => {
                let bpm = tempo_bpm.unwrap_or(120.0).max(1.0);
                bpm / 60.0 / beats.max(1.0e-3) / sample_rate
            }
        };
        let prev = self.phase;
        self.phase = (self.phase + inc).rem_euclid(1.0);
        let cleared = self.pattern.clear_crossed(prev, self.phase);

        let raw = self.pattern.value(self.phase).clamp(0.0, 1.0);
        let target = 1.0 + (raw - 1.0) * self.depth.clamp(0.0, 1.0);
        self.smoothed += (target - self.smoothed) * self.smooth_coeff;
        (self.smoothed, cleared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_interpolates_and_wraps() {
        let m = Mseg::new(vec![
            MsegPoint::new(0.0, 0.0),
            MsegPoint::new(0.5, 1.0),
        ]);
        assert!((m.value(0.25) - 0.5).abs() < 1e-9);
        // Wrap segment 0.5 → 1.0 back to y=0 at x=0.
        assert!((m.value(0.75) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn hold_points_step() {
        let m = Mseg::gate(0.5);
        assert_eq!(m.value(0.25), 1.0);
        assert_eq!(m.value(0.75), 0.0);
    }

    #[test]
    fn tension_bends_the_segment() {
        let mut p = MsegPoint::new(0.0, 0.0);
        p.tension = 1.0;
        let m = Mseg::new(vec![p, MsegPoint::new(1.0 - 1e-9, 1.0)]);
        // Positive tension: slow start (t^k, k>1) → below linear midway.
        assert!(m.value(0.5) < 0.5);
    }

    #[test]
    fn clear_crossing_detects_wrap() {
        let mut p = MsegPoint::new(0.05, 1.0);
        p.clear_tails = true;
        let m = Mseg::new(vec![p, MsegPoint::new(0.5, 0.0)]);
        assert!(m.clear_crossed(0.9, 0.1));
        assert!(!m.clear_crossed(0.1, 0.4));
    }

    #[test]
    fn lane_gates_smoothly_in_sync() {
        let mut lane = MsegLane::new(Mseg::gate(0.5), MsegRate::SyncBeats(1.0));
        lane.configure(48000.0);
        // One beat at 120 BPM = 24000 samples per cycle.
        let mut min_g = 1.0f64;
        let mut max_g = 0.0f64;
        let mut max_step = 0.0f64;
        let mut prev = 1.0;
        for _ in 0..96_000 {
            let (g, _) = lane.tick(48000.0, Some(120.0));
            min_g = min_g.min(g);
            max_g = max_g.max(g);
            max_step = max_step.max((g - prev).abs());
            prev = g;
        }
        assert!(max_g > 0.95 && min_g < 0.05, "gate should span: {min_g}..{max_g}");
        assert!(max_step < 0.05, "gain moves must ramp: {max_step}");
    }
}
