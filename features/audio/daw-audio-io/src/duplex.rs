//! Cross-platform **duplex audio backend** — one realtime callback that gets
//! capture *and* playback buffers for the same cycle (no ring bridge between
//! separate input/output streams). This is what low-latency DAW engines use.
//!
//! The contract is platform-agnostic: a caller hands [`start`](DuplexBackend::start)
//! a [`DuplexConfig`] (how many capture/playback channels, desired latency) and a
//! `process` closure, and the backend invokes that closure from the realtime
//! thread every block. Each platform supplies its own implementation behind the
//! [`Backend`] alias:
//!
//! | OS | impl | mechanism |
//! |----|------|-----------|
//! | Linux | [`crate::duplex_pw::PipewireBackend`] | `pw_filter` (capture+playback ports, one process cb) |
//! | macOS | _todo_ | CoreAudio AUHAL duplex unit |
//! | Windows | _todo_ | WASAPI duplex / ASIO |
//!
//! The rig's signal processing (tap input channel → FX chain → stereo out) is the
//! `process` closure — identical on every platform; only the backend differs.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// What the backend opens: channel counts + the desired block size.
#[derive(Clone, Debug)]
pub struct DuplexConfig {
    /// Graph node name (how it appears in patchbays).
    pub name: String,
    /// Number of capture (input) ports to create.
    pub inputs: usize,
    /// Number of playback (output) ports to create.
    pub outputs: usize,
    /// Desired block size + rate as `(frames, rate)` → a per-node latency request
    /// (e.g. `(64, 48000)`). `None` lets the graph decide.
    pub latency: Option<(u32, u32)>,
}

/// One realtime block handed to the `process` closure. `inputs[c]` and
/// `outputs[c]` are each `frames` long; input and output are the **same** cycle
/// (sample-synchronous — no ring, so no phase underruns).
pub struct ProcessBlock<'a> {
    pub inputs: &'a [&'a [f32]],
    pub outputs: &'a mut [&'a mut [f32]],
    pub frames: usize,
}

/// The `process` closure type: called from the realtime thread every block.
pub type ProcessFn = Box<dyn FnMut(&mut ProcessBlock) + Send>;

/// Live engine metrics, written from the realtime callback, read by the UI.
/// These are the numbers the rig meters need — render time (DSP load) and
/// xruns — measured directly because the duplex callback is *our* code.
#[derive(Default)]
pub struct EngineStats {
    /// Total process callbacks fired.
    pub calls: AtomicU64,
    /// Frames in the last block (the running quantum).
    pub block_frames: AtomicU32,
    /// Render time of the last block, nanoseconds.
    pub render_ns: AtomicU64,
    /// Peak render time since the last [`reset_peak`](Self::reset_peak), ns.
    pub peak_render_ns: AtomicU64,
    /// Blocks where the graph reported an xrun.
    pub xruns: AtomicU64,
}

impl EngineStats {
    /// Record a block's render time (updates last + peak).
    pub fn record_render(&self, ns: u64) {
        self.render_ns.store(ns, Ordering::Relaxed);
        self.peak_render_ns.fetch_max(ns, Ordering::Relaxed);
    }
    /// DSP load 0..1 = render time / block budget at `rate`.
    pub fn load(&self, rate: u32) -> f64 {
        let frames = self.block_frames.load(Ordering::Relaxed).max(1) as f64;
        let budget_ns = frames / rate.max(1) as f64 * 1e9;
        (self.render_ns.load(Ordering::Relaxed) as f64 / budget_ns).clamp(0.0, 1.0)
    }
    /// Last / peak render time in milliseconds.
    pub fn render_ms(&self) -> (f64, f64) {
        (
            self.render_ns.load(Ordering::Relaxed) as f64 / 1e6,
            self.peak_render_ns.load(Ordering::Relaxed) as f64 / 1e6,
        )
    }
    pub fn reset_peak(&self) {
        self.peak_render_ns.store(0, Ordering::Relaxed);
    }
}

/// A platform duplex audio backend. Dropping it stops audio.
pub trait DuplexBackend: Send + Sized {
    /// Open the device(s) and begin calling `process` from the realtime thread.
    fn start(cfg: DuplexConfig, process: ProcessFn) -> Result<Self, String>;
    /// The negotiated sample rate.
    fn sample_rate(&self) -> u32;
    /// Shared live metrics (render time, xruns, block size).
    fn stats(&self) -> Arc<EngineStats>;
    /// The graph node name, so the caller can wire device ports to it.
    fn node_name(&self) -> &str;
}

// The platform backend. Linux = native PipeWire `pw_filter`; other platforms
// land here as they're implemented.
#[cfg(all(feature = "pipewire", target_os = "linux"))]
pub use crate::duplex_pw::PipewireBackend as Backend;
