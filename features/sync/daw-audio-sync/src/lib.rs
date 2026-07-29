// Lint debt: workspace flipped dead_code/unused to warn (task cleanup);
// this crate predates that — burn down separately.
#![allow(dead_code, unused)]

//! Audio-thread sync primitives for sample-accurate multi-machine playback.
//!
//! # Layers
//!
//! - **Snapshot** ([`AudioSnapshot`], [`SnapshotCell`]): the per-buffer
//!   observation written by the audio thread and read by everyone else
//!   (main thread, sync engine, diagnostics RPC).
//! - **Hook** ([`AudioSyncHook`]): implements reaper-medium's
//!   `OnAudioBuffer`. Registered once at extension load; runs on REAPER's
//!   real-time audio thread.
//!
//! Future layers (peer clock sync, sample-position protocol, drift
//! correction) build on this foundation.
//!
//! # Realtime discipline
//!
//! Everything on the audio thread must be:
//! - **alloc-free** — no `Vec::push`, `Box::new`, `String::from`, etc.
//! - **lock-free** — no `Mutex`, no `RwLock`, no blocking syscalls.
//! - **bounded latency** — short, predictable work per callback.
//!
//! The snapshot store uses a seqlock pattern (two `AtomicU64`s for the
//! sequence counter + per-field `AtomicU64`s) so the audio thread writes
//! without locking and readers can detect torn reads via the sequence.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub mod clock_sync;
pub mod drift;
pub mod registry;

#[inline]
pub(crate) fn split_project_id(id: ProjectId) -> (u64, u64) {
    let mut hi = [0u8; 8];
    let mut lo = [0u8; 8];
    hi.copy_from_slice(&id[0..8]);
    lo.copy_from_slice(&id[8..16]);
    (u64::from_le_bytes(hi), u64::from_le_bytes(lo))
}

#[inline]
pub(crate) fn combine_project_id(hi: u64, lo: u64) -> ProjectId {
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&hi.to_le_bytes());
    out[8..16].copy_from_slice(&lo.to_le_bytes());
    out
}

use reaper_medium::{
    OnAudioBuffer, OnAudioBufferArgs, ProjectContext, RealTimeAudioThreadScope,
    Reaper as MediumReaper,
};

/// 16-byte project identifier. Each open REAPER project gets a stable
/// id assigned at first observation; the id is process-local (resets
/// on extension reload) — for FTS-session matching, callers should
/// pair this with a longer-lived identifier (project file path or
/// REAPER project GUID) at the management layer.
///
/// `[0u8; 16]` is the sentinel "no project" / "current project" value
/// for backward compat with single-project consumers.
pub type ProjectId = [u8; 16];

/// One audio-buffer observation for a single project. Written by the
/// audio thread, read by anyone. All fields are values at the start
/// of the buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioSnapshot {
    /// Monotonic counter — increments once per audio buffer. Readers
    /// can use deltas to detect "did we miss a tick".
    pub sequence: u64,
    /// Project this snapshot describes. Distinct from REAPER's
    /// internal project GUID; assigned by the bridge's project
    /// registry. Single-project consumers can ignore this.
    pub project_id: ProjectId,
    /// REAPER's audio-clock host time at the start of this buffer
    /// (microseconds). Same time domain as `mLink.clock().micros()` on
    /// reablink — a high-resolution monotonic clock anchored to the
    /// audio engine, not wall clock.
    pub host_micros: u64,
    /// Playhead position at the start of the buffer, in seconds.
    /// `f64` bit-cast through `AtomicU64` for lock-free storage.
    pub playhead_seconds: f64,
    /// Sample rate reported by REAPER (Hz). Stored as `f64` for
    /// arithmetic ergonomics; typically 44.1k or 48k.
    pub sample_rate: f64,
    /// Number of frames in this buffer. Typical 64..2048.
    pub buffer_len: u32,
    /// Whether the transport is playing as observed by the audio
    /// thread (cached so consumers don't need a separate API call).
    pub is_playing: bool,
}

/// Lock-free single-writer / multiple-reader cell holding the latest
/// [`AudioSnapshot`]. Uses a seqlock pattern: the writer bumps the
/// sequence on entry (making it odd → "write in progress"), stores the
/// payload, then bumps again (making it even → "consistent"). Readers
/// load the sequence, then the payload, then re-load the sequence; if
/// it changed or was odd, they retry.
pub struct SnapshotCell {
    seq: AtomicU64,
    snapshot_seq: AtomicU64,
    project_id_hi: AtomicU64,
    project_id_lo: AtomicU64,
    host_micros: AtomicU64,
    playhead_bits: AtomicU64,
    sample_rate_bits: AtomicU64,
    buffer_len: AtomicU32,
    is_playing: AtomicU32,
}

impl SnapshotCell {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            snapshot_seq: AtomicU64::new(0),
            project_id_hi: AtomicU64::new(0),
            project_id_lo: AtomicU64::new(0),
            host_micros: AtomicU64::new(0),
            playhead_bits: AtomicU64::new(0),
            sample_rate_bits: AtomicU64::new(0),
            buffer_len: AtomicU32::new(0),
            is_playing: AtomicU32::new(0),
        }
    }

    /// Write a new snapshot. Audio thread only. Wait-free.
    ///
    /// Internal seqlock convention: the cell's own `seq` counter is
    /// distinct from `snap.sequence`. The audio thread increments
    /// `seq` by 1 on entry (odd → write in progress) and by 1 again
    /// on exit (even → consistent). `snap.sequence` rides through as
    /// payload so readers can observe missed buffers.
    #[inline]
    pub fn store(&self, snap: &AudioSnapshot) {
        let prev = self.seq.load(Ordering::Relaxed);
        // Make odd: write-in-progress. If prev is even, +1 makes it
        // odd; if prev is odd (uninitialised: prev=0 is even; after
        // first store prev=2 is even; only re-entrant calls hit odd,
        // which the audio thread never does — single producer).
        let in_progress = prev.wrapping_add(1);
        self.seq.store(in_progress, Ordering::Release);
        self.snapshot_seq.store(snap.sequence, Ordering::Relaxed);
        let (hi, lo) = split_project_id(snap.project_id);
        self.project_id_hi.store(hi, Ordering::Relaxed);
        self.project_id_lo.store(lo, Ordering::Relaxed);
        self.host_micros.store(snap.host_micros, Ordering::Relaxed);
        self.playhead_bits
            .store(snap.playhead_seconds.to_bits(), Ordering::Relaxed);
        self.sample_rate_bits
            .store(snap.sample_rate.to_bits(), Ordering::Relaxed);
        self.buffer_len.store(snap.buffer_len, Ordering::Relaxed);
        self.is_playing
            .store(snap.is_playing as u32, Ordering::Relaxed);
        // Even: consistent.
        self.seq
            .store(in_progress.wrapping_add(1), Ordering::Release);
    }

    /// Read the latest snapshot. Returns `None` if nothing has been
    /// stored yet. Retries on contention; gives up after a few spins
    /// (audio thread is fast — contention windows are sub-µs).
    pub fn load(&self) -> Option<AudioSnapshot> {
        for _ in 0..4 {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 == 0 {
                return None;
            }
            if s1 & 1 != 0 {
                // Write in progress — retry.
                core::hint::spin_loop();
                continue;
            }
            let snapshot_seq = self.snapshot_seq.load(Ordering::Relaxed);
            let project_id = combine_project_id(
                self.project_id_hi.load(Ordering::Relaxed),
                self.project_id_lo.load(Ordering::Relaxed),
            );
            let host_micros = self.host_micros.load(Ordering::Relaxed);
            let playhead = f64::from_bits(self.playhead_bits.load(Ordering::Relaxed));
            let sr = f64::from_bits(self.sample_rate_bits.load(Ordering::Relaxed));
            let buffer_len = self.buffer_len.load(Ordering::Relaxed);
            let is_playing = self.is_playing.load(Ordering::Relaxed) != 0;
            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 {
                return Some(AudioSnapshot {
                    sequence: snapshot_seq,
                    project_id,
                    host_micros,
                    playhead_seconds: playhead,
                    sample_rate: sr,
                    buffer_len,
                    is_playing,
                });
            }
        }
        None
    }
}

impl Default for SnapshotCell {
    fn default() -> Self {
        Self::new()
    }
}

/// REAPER audio hook. Registered once at extension load via
/// `ReaperSession::audio_reg_hardware_hook_add`. Writes a fresh
/// [`AudioSnapshot`] to the shared [`SnapshotCell`] on every callback's
/// pre-buffer phase.
///
/// Holds a `MediumReaper<RealTimeAudioThreadScope>` so it can call
/// `get_play_position_2_ex` (audio-thread-safe variant) and
/// `time_precise` for the host clock.
pub struct AudioSyncHook {
    cell: Arc<SnapshotCell>,
    reaper: MediumReaper<RealTimeAudioThreadScope>,
    counter: u64,
}

impl AudioSyncHook {
    pub fn new(cell: Arc<SnapshotCell>, reaper: MediumReaper<RealTimeAudioThreadScope>) -> Self {
        Self {
            cell,
            reaper,
            counter: 0,
        }
    }
}

impl OnAudioBuffer for AudioSyncHook {
    fn call(&mut self, args: OnAudioBufferArgs) {
        // We sample on the pre-buffer phase only — keeps the snapshot
        // rate at one per audio buffer instead of two.
        if args.is_post {
            return;
        }
        self.counter = self.counter.wrapping_add(1);

        // time_precise: REAPER's monotonic audio-engine clock in
        // seconds. Convert to microseconds for compactness in the
        // wire protocol later. Only exposed at the low binding level
        // (medium hasn't wrapped it), so we reach through.
        let host_secs = self.reaper.low().time_precise();
        let host_micros = (host_secs * 1_000_000.0) as u64;

        // get_play_position_2_ex: position of next audio block —
        // matches the audio thread's notion of "now". get_play_position
        // is for the displayed cursor which lags by latency.
        let pos_value = self
            .reaper
            .get_play_position_2_ex(ProjectContext::CurrentProject)
            .get();

        let is_playing = self
            .reaper
            .get_play_state_ex(ProjectContext::CurrentProject)
            .is_playing;

        self.cell.store(&AudioSnapshot {
            sequence: self.counter,
            project_id: [0u8; 16],
            host_micros,
            playhead_seconds: pos_value,
            sample_rate: args.srate.get(),
            buffer_len: args.len,
            is_playing,
        });
    }
}

/// Convenience: build the cell + hook pair and return the cell handle
/// for the reader side. Caller registers the returned hook via
/// `ReaperSession::audio_reg_hardware_hook_add`.
pub fn build_hook(
    reaper: MediumReaper<RealTimeAudioThreadScope>,
) -> (Arc<SnapshotCell>, AudioSyncHook) {
    let cell = Arc::new(SnapshotCell::new());
    let hook = AudioSyncHook::new(cell.clone(), reaper);
    (cell, hook)
}

// ── Process-global cell ────────────────────────────────────────────
//
// The audio hook is registered in the REAPER extension (daw-bridge),
// but the Diagnostics RPC impl that exposes snapshots lives in
// daw-reaper. To bridge them without circular deps, we publish the
// cell as a global here. daw-bridge sets it on extension load;
// daw-reaper reads it from the Diagnostics impl.

static GLOBAL_CELL: std::sync::OnceLock<Arc<SnapshotCell>> = std::sync::OnceLock::new();

/// Publish the cell so other crates can read snapshots. Call once at
/// extension load. Subsequent calls are silently ignored.
pub fn set_global_cell(cell: Arc<SnapshotCell>) {
    let _ = GLOBAL_CELL.set(cell);
}

/// Read the latest snapshot from the process-global cell. Returns
/// `None` if the cell hasn't been published yet (extension not
/// loaded) or if the audio hook hasn't fired yet (no audio engine).
pub fn global_snapshot() -> Option<AudioSnapshot> {
    GLOBAL_CELL.get()?.load()
}

// ── Process-global ClockSync ───────────────────────────────────────
//
// Same pattern as `GLOBAL_CELL`: daw-bridge publishes the live
// session on startup; consumers (daw-reaper's Diagnostics impl, etc.)
// read peer-table snapshots without re-binding sockets.

static GLOBAL_CLOCK_SYNC: std::sync::OnceLock<Arc<clock_sync::ClockSync>> =
    std::sync::OnceLock::new();

pub fn set_global_clock_sync(session: Arc<clock_sync::ClockSync>) {
    let _ = GLOBAL_CLOCK_SYNC.set(session);
}

pub fn global_clock_sync() -> Option<&'static Arc<clock_sync::ClockSync>> {
    GLOBAL_CLOCK_SYNC.get()
}

// ── Process-global DriftCorrector ──────────────────────────────────

static GLOBAL_DRIFT_CORRECTOR: std::sync::OnceLock<Arc<drift::DriftCorrector>> =
    std::sync::OnceLock::new();

pub fn set_global_drift_corrector(corrector: Arc<drift::DriftCorrector>) {
    let _ = GLOBAL_DRIFT_CORRECTOR.set(corrector);
}

pub fn global_drift_corrector() -> Option<&'static Arc<drift::DriftCorrector>> {
    GLOBAL_DRIFT_CORRECTOR.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn snapshot_round_trip() {
        let cell = SnapshotCell::new();
        let snap = AudioSnapshot {
            sequence: 42,
            project_id: [7u8; 16],
            host_micros: 1_000_000,
            playhead_seconds: 3.5,
            sample_rate: 48000.0,
            buffer_len: 256,
            is_playing: true,
        };
        cell.store(&snap);
        let read = cell.load().expect("stored");
        assert_eq!(read, snap);
    }

    #[test]
    fn empty_cell_returns_none() {
        let cell = SnapshotCell::new();
        assert!(cell.load().is_none());
    }

    #[test]
    fn seqlock_under_contention() {
        let cell = Arc::new(SnapshotCell::new());
        let writer_cell = cell.clone();
        let writer = thread::spawn(move || {
            for i in 0..10_000 {
                writer_cell.store(&AudioSnapshot {
                    sequence: i,
                    project_id: [0u8; 16],
                    host_micros: i * 1000,
                    playhead_seconds: i as f64 * 0.01,
                    sample_rate: 48000.0,
                    buffer_len: 256,
                    is_playing: i % 2 == 0,
                });
            }
        });
        let mut last_seq = 0u64;
        for _ in 0..100_000 {
            if let Some(s) = cell.load() {
                // Cross-field consistency: playhead and host_micros
                // were written together, so they must match.
                assert!((s.playhead_seconds - s.sequence as f64 * 0.01).abs() < 1e-9);
                assert_eq!(s.host_micros, s.sequence * 1000);
                assert!(s.sequence >= last_seq);
                last_seq = s.sequence;
            }
        }
        writer.join().unwrap();
    }
}
