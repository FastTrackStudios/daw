//! Signalsmith Stretch — High-quality spectral pitch shifter.
//!
//! Wraps the [signalsmith-stretch](https://crates.io/crates/signalsmith-stretch)
//! FFT-based time-stretch / pitch-shift library as a sample-by-sample shifter
//! matching the interface of the other algorithms in this crate.
//!
//! Internally buffers input into blocks, calls `Stretch::process`, and drains
//! output one sample at a time via a ring buffer.
//!
//! Latency: `input_latency() + output_latency()` of the underlying library
//! plus one block of buffering (~256 samples).
//! Character: Very clean, polyphonic-capable, spectral quality.

use signalsmith_stretch::Stretch;
use std::collections::VecDeque;

/// Block size for standard quality mode.
const BLOCK_SIZE: usize = 256;
/// Block size for live (low-latency) mode.
const BLOCK_SIZE_LIVE: usize = 64;

/// Signalsmith Stretch pitch shifter.
pub struct SignalsmithShifter {
    /// Pitch ratio: 0.5 = octave down, 2.0 = octave up.
    pub speed: f64,
    /// Mix: 0.0 = dry only, 1.0 = wet only.
    pub mix: f64,
    /// Live mode: use cheaper preset and smaller blocks for lower latency.
    pub live: bool,

    /// The underlying Signalsmith Stretch instance.
    stretch: Stretch,

    /// Accumulates input samples until we have a full block.
    input_buf: Vec<f32>,
    /// Ring buffer of produced output samples.
    output_queue: VecDeque<f32>,

    /// Scratch buffer reused for process() output.
    output_scratch: Vec<f32>,

    /// Current sample rate (for re-initialisation).
    sample_rate: f64,

    /// The last `speed` value sent to the engine, used to avoid redundant calls.
    last_speed: f64,

    /// Current block size (depends on live mode).
    block_size: usize,
    /// Track previous live state to detect changes.
    prev_live: bool,
}

impl SignalsmithShifter {
    pub fn new() -> Self {
        let sample_rate = 48000.0;
        let stretch = Stretch::preset_default(1, sample_rate as u32);

        let mut s = Self {
            speed: 0.5,
            mix: 1.0,
            live: false,
            stretch,
            input_buf: Vec::with_capacity(BLOCK_SIZE),
            output_queue: VecDeque::with_capacity(BLOCK_SIZE * 2),
            output_scratch: vec![0.0f32; BLOCK_SIZE],
            sample_rate,
            last_speed: -1.0, // force first update
            block_size: BLOCK_SIZE,
            prev_live: false,
        };
        s.apply_speed();
        s
    }

    pub fn update(&mut self, sample_rate: f64) {
        let sr_changed = (self.sample_rate - sample_rate).abs() > 0.5;
        let live_changed = self.live != self.prev_live;

        if sr_changed || live_changed {
            self.sample_rate = sample_rate;
            self.prev_live = self.live;
            self.block_size = if self.live {
                BLOCK_SIZE_LIVE
            } else {
                BLOCK_SIZE
            };

            self.stretch = if self.live {
                // Low-latency: small FFT block (256) with 1/4 hop (64).
                // Trades spectral resolution for ~5ms latency at 48kHz.
                Stretch::new(1, 256, 64)
            } else {
                Stretch::preset_default(1, sample_rate as u32)
            };

            self.input_buf.clear();
            self.output_queue.clear();
            self.last_speed = -1.0;
        }
        self.apply_speed();
    }

    pub fn reset(&mut self) {
        self.stretch.reset();
        self.input_buf.clear();
        self.output_queue.clear();
        self.last_speed = -1.0;
        self.apply_speed();
    }

    /// Process one sample. Returns the pitch-shifted (mixed) output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        // Ensure transpose factor is up to date.
        if (self.speed - self.last_speed).abs() > 1e-9 {
            self.apply_speed();
        }

        self.input_buf.push(input as f32);

        if self.input_buf.len() >= self.block_size {
            self.process_block();
        }

        // Return next output sample if available, otherwise zero (startup).
        let wet = self.output_queue.pop_front().unwrap_or(0.0) as f64;
        input * (1.0 - self.mix) + wet * self.mix
    }

    pub fn latency(&self) -> usize {
        self.stretch.input_latency() + self.stretch.output_latency() + self.block_size
    }

    // ── private ──────────────────────────────────────────────────────

    fn apply_speed(&mut self) {
        self.stretch
            .set_transpose_factor(self.speed as f32, Some(0.5));
        self.last_speed = self.speed;
    }

    fn process_block(&mut self) {
        let n = self.input_buf.len();
        // Ensure scratch is large enough.
        self.output_scratch.resize(n, 0.0);
        self.output_scratch.fill(0.0);

        // For pure pitch shifting (no time stretch) input and output are the
        // same number of frames.
        self.stretch
            .process(&self.input_buf[..n], &mut self.output_scratch[..n]);

        // Drain into the output ring buffer.
        for &s in &self.output_scratch[..n] {
            self.output_queue.push_back(s);
        }

        self.input_buf.clear();
    }
}

impl Default for SignalsmithShifter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_shifter() -> SignalsmithShifter {
        let mut s = SignalsmithShifter::new();
        s.speed = 0.5;
        s.mix = 1.0;
        s.update(SR);
        s
    }

    #[test]
    fn silence_in_silence_out() {
        let mut s = make_shifter();
        for _ in 0..4800 {
            let out = s.tick(0.0);
            assert!(out.abs() < 1e-6, "Should be silent: {out}");
        }
    }

    #[test]
    fn produces_output_on_sine() {
        let mut s = make_shifter();
        let freq = 220.0;

        let mut energy = 0.0;
        let n = 48000;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            if i > 4096 {
                energy += out * out;
            }
        }
        assert!(energy > 0.1, "Should produce output: energy={energy}");
    }

    #[test]
    fn no_nan() {
        let mut s = make_shifter();
        for i in 0..48000 {
            let input = (2.0 * PI * 82.0 * i as f64 / SR).sin() * 0.9;
            let out = s.tick(input);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn different_speeds_differ() {
        let freq = 220.0;
        let n = 9600;

        let collect = |speed: f64| -> Vec<f64> {
            let mut s = make_shifter();
            s.speed = speed;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let x = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(s.tick(x));
            }
            out
        };

        let down = collect(0.5);
        let up = collect(2.0);

        let diff: f64 = down
            .iter()
            .zip(up.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / n as f64;

        assert!(
            diff > 0.001,
            "Different speeds should produce different output: {diff}"
        );
    }

    #[test]
    fn dry_wet_mix() {
        let mut s = make_shifter();
        s.mix = 0.0;

        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = s.tick(input);
            assert!((out - input).abs() < 1e-10, "Mix=0 should pass dry");
        }
    }
}
