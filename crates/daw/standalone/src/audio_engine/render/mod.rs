//! Backend-agnostic project renderer.
//!
//! `ProjectRenderer` takes a [`Standalone`] backend + project GUID
//! and produces a stereo master buffer for a given sample range.
//! It walks the routing graph each block (cheap because we snapshot
//! the state once per project revision) and honors:
//!
//! - Item playback: position, length, fade in/out (linear), volume,
//!   take start_offset, take play_rate (linear-interp resampling),
//!   take volume, item mute / track mute / solo
//! - Track sends → destination track bus (volume + pan)
//! - `parent_send_enabled` (REAPER `B_MAINSEND`) → sum to master
//! - Track volume + pan applied on the pre-send signal
//!
//! Out of scope for v0:
//! - Hardware outputs (currently sum to master same as parent send)
//! - Channel mappings beyond stereo (channel count is honored on the
//!   bus but down-mixed to stereo at the master)
//!
//! Module layout — each stage of the block pipeline lives in its own
//! file; `render_block` below only orchestrates:
//!
//! - [`snapshot`] — per-revision copy of everything the audio thread
//!   reads (tracks, items, envelopes, FX chains, master section)
//! - [`graph`] — topo processing order + solo routing mask, computed
//!   once per snapshot
//! - [`item_mix`] — item playback into the track bus
//! - [`midi`] — per-block MIDI / note-expression event collection
//! - [`envelope`] — per-frame envelope cursors
//! - [`schedule`] — which track of a block runs when, on one thread or
//!   many (each as soon as what feeds it is done), the same for realtime
//!   and offline renders
//! - [`node`] — one track's block: media, the sums into it, FX, fader,
//!   sends and its parent share
//!
//! WASM-compatible: pure math + heap, no threads, no cpal. The cpal
//! mixer in `mixer.rs` wraps this on native; an AudioWorklet shim can
//! wrap it on the web.

mod automate;
mod envelope;
#[cfg(test)]
mod envelope_bench;
mod graph;
mod item_mix;
#[cfg(any(feature = "decode", feature = "audio"))]
pub mod live_tap;
mod midi;
mod node;
pub mod offline;
#[cfg(not(target_arch = "wasm32"))]
mod parallel;
mod part;
mod pdc;
mod print;
mod schedule;
mod snapshot;

use std::sync::Arc;

pub use automate::automation_events;
use envelope::EnvelopeCursor;
use item_mix::mix_item_into_bus;
#[cfg(test)]
use midi::collect_midi_events;
use midi::{collect_midi_events_into, collect_note_expressions};
pub use part::Feed;
pub(crate) use part::Part;
pub(crate) use snapshot::{LiveHints, TempoMap};

/// Test seam: the realtime render's snapshot, built as it builds it.
#[cfg(all(test, feature = "clap-host"))]
pub(crate) use snapshot::RenderSnapshot as RenderSnapshotFor;

/// Test seam: the item MIDI the render hands snapshot track `ti`'s chain
/// for `[start, end)` seconds.
#[cfg(all(test, feature = "clap-host"))]
pub(crate) fn collect_track_midi(
    out: &mut Vec<crate::plugin::PluginMidiEvent>,
    snap: &RenderSnapshot,
    ti: usize,
    start: f64,
    end: f64,
    frames_per_second: f64,
) {
    let frames = ((end - start) * frames_per_second) as usize;
    collect_midi_events_into(out, &snap.tracks[ti], start, end, frames_per_second, frames);
}
use snapshot::{RenderSnapshot, TrackSnapshot};

use crate::sync::Standalone;

/// Live hardware input feeding the renderer (project mode).
///
/// The cpal *input* callback (owned by `AudioEngine`) pushes the
/// interleaved frames it receives — all `channels` of them — into an
/// `rtrb` ring; the renderer holds the consumer and drains it once per
/// block in render **stage 0**. The ring is SPSC + lock-free on the
/// audio thread; only the consumer handle lives behind a `Mutex` (read
/// once per block, alongside the existing scratch lock).
///
/// f32 input only — `AudioEngine` opens the input stream as `f32`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct LiveInput {
    /// Ring consumer of interleaved input frames (`channels` per frame).
    pub(crate) cons: rtrb::Consumer<f32>,
    /// Number of interleaved channels the input stream delivers.
    pub(crate) channels: usize,
    /// De-ring scratch for the block (`frames * channels`), reused.
    pub(crate) scratch: Vec<f32>,
    /// Count of blocks where the ring couldn't supply a full block
    /// (input not keeping up) — the shortfall is zero-filled.
    pub(crate) underruns: u64,
    /// Count of blocks where a backlog past one spare block was dropped
    /// to keep the input → output latency at the buffer size.
    pub(crate) resyncs: u64,
}

/// Mix the live hardware input into per-track buses (render stage 0).
///
/// Drains up to `frames * channels` interleaved samples from `live`'s
/// ring into its scratch (zero-filling any shortfall and counting it as
/// an underrun), then for every track tapping a valid input channel
/// writes that channel's mono sample into BOTH L and R of the track's
/// bus and marks it dirty. The existing FX / gain / send / master
/// stages then process it unchanged.
///
/// Split out as a free function so it's unit-testable without standing
/// up a full `ProjectRenderer` (DB, transport, snapshot).
///
/// `input_channels[ti]` is `Some(ch)` for a live-input track, else
/// `None`. `passes(ti)` is the solo-routing predicate (same one stage 1
/// uses); a track that fails it contributes silence.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn mix_live_input_into_buses(
    live: &mut LiveInput,
    input_channels: &[Option<u32>],
    passes: impl Fn(usize) -> bool,
    buses: &mut [StereoBuffer],
    dirty: &mut [bool],
    frames: usize,
) {
    let channels = live.channels;
    if channels == 0 || frames == 0 {
        return;
    }
    let want = frames * channels;
    if live.scratch.len() < want {
        live.scratch.resize(want, 0.0);
    }
    // Latency guard: the ring is drained one block per output callback, so
    // anything queued beyond that — input started before output, an output
    // stall, two device clocks drifting — would otherwise stay queued (and
    // audible as delay) for the life of the stream. Keep at most one spare
    // block for callback jitter and drop the oldest frames past it.
    let backlog = live.cons.slots();
    if backlog > want * 2 {
        let skip = (backlog - want * 2) / channels * channels;
        if let Ok(chunk) = live.cons.read_chunk(skip) {
            chunk.commit_all();
            live.resyncs = live.resyncs.wrapping_add(1);
        }
    }
    // Drain interleaved input frames from the ring; zero-fill shortfall.
    let avail = live.cons.slots();
    let n = want.min(avail);
    if n < want {
        live.underruns = live.underruns.wrapping_add(1);
    }
    if n > 0
        && let Ok(chunk) = live.cons.read_chunk(n)
    {
        let (a, b) = chunk.as_slices();
        live.scratch[..a.len()].copy_from_slice(a);
        live.scratch[a.len()..a.len() + b.len()].copy_from_slice(b);
        chunk.commit_all();
    }
    for s in &mut live.scratch[n..want] {
        *s = 0.0;
    }

    for (ti, ch_opt) in input_channels.iter().enumerate() {
        let Some(ch) = *ch_opt else { continue };
        let ch = ch as usize;
        if ch >= channels {
            continue;
        }
        if !passes(ti) {
            continue;
        }
        let Some(bus) = buses.get_mut(ti) else {
            continue;
        };
        for f in 0..frames.min(bus.frames) {
            let s = live.scratch[f * channels + ch];
            bus.samples[f * 2] = s;
            bus.samples[f * 2 + 1] = s;
        }
        dirty[ti] = true;
    }
}

/// A single programmatic / live MIDI event addressed to a project track.
///
/// The MIDI analog of a live-input audio frame: pushed (lock-free) by a
/// producer the UI thread reaches through `Standalone`, drained once per
/// block by the renderer and merged into the target track's per-block
/// MIDI event list before the FX chain runs. `offset = 0` means
/// block-start — programmatic pushes don't carry sub-block timing (this
/// matches how `collect_midi_events` clamps item events to the block).
///
/// Native: the live-MIDI ring is `rtrb`-backed (a `cfg(not(wasm32))` dep),
/// mirroring [`LiveInput`]. On wasm32 the same event type is queued in a
/// plain `VecDeque` on `Standalone` (the AudioWorkletGlobalScope is
/// single-threaded, so a mutex-guarded deque is uncontended by
/// construction) — see the wasm `LiveMidiQueue` below.
#[derive(Clone, Debug)]
pub(crate) struct LiveMidiEvent {
    /// Target project track guid (resolved to a snapshot index at drain).
    pub(crate) track: String,
    /// Sample offset from block start (0..block_size). Usually 0.
    pub(crate) offset: u32,
    pub(crate) message: daw_proto::MidiEvent,
}

/// Renderer-side handle to the live-MIDI ring (consumer end).
///
/// SPSC + lock-free like [`LiveInput`]; only the consumer handle lives
/// behind a `Mutex`, drained once per block alongside the scratch lock.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct LiveMidiQueue {
    /// Ring consumer of programmatic MIDI events.
    pub(crate) cons: rtrb::Consumer<LiveMidiEvent>,
}

/// wasm32 variant: same public shape as the native queue, backed by a plain
/// `VecDeque` instead of an rtrb ring. The AudioWorkletGlobalScope is
/// single-threaded, so producer (a `WebRenderer` MIDI method) and consumer
/// (the render block) never race; the deque lives on `Standalone`
/// (`live_midi_wasm`) because the web render path constructs a fresh
/// `ProjectRenderer` per block.
#[cfg(target_arch = "wasm32")]
pub(crate) struct LiveMidiQueue {
    pub(crate) events: std::collections::VecDeque<LiveMidiEvent>,
}

/// Drain every queued live-MIDI event into a per-track scratch bucket.
///
/// Resolves each event's track guid to a snapshot index via `idx_of`
/// (built from the snapshot's `guid` fields) and appends a
/// `PluginMidiEvent` to that track's bucket. Events whose track guid
/// doesn't resolve are dropped. Split out as a free function so the
/// drain + per-track merge is unit-testable without a `ProjectRenderer`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn drain_live_midi(
    queue: &mut LiveMidiQueue,
    idx_of: impl Fn(&str) -> Option<usize>,
    buckets: &mut [Vec<crate::plugin::PluginMidiEvent>],
) {
    drain_live_midi_routed(queue, idx_of, buckets, |_| true, None);
}

/// [`drain_live_midi`] for a render covering only some tracks: an event
/// for a track it does not `cover` goes on to `forward` — the render
/// that does (the ahead render, for a note played into a track rendered
/// ahead) — or is dropped when there is none.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn drain_live_midi_routed(
    queue: &mut LiveMidiQueue,
    idx_of: impl Fn(&str) -> Option<usize>,
    buckets: &mut [Vec<crate::plugin::PluginMidiEvent>],
    covers: impl Fn(usize) -> bool,
    mut forward: Option<&mut rtrb::Producer<LiveMidiEvent>>,
) {
    let avail = queue.cons.slots();
    for _ in 0..avail {
        let Ok(ev) = queue.cons.pop() else { break };
        let Some(ti) = idx_of(&ev.track) else {
            continue;
        };
        if !covers(ti) {
            if let Some(out) = forward.as_deref_mut() {
                let _ = out.push(ev);
            }
            continue;
        }
        let Some(bucket) = buckets.get_mut(ti) else {
            continue;
        };
        bucket.push(crate::plugin::PluginMidiEvent {
            offset: ev.offset,
            message: ev.message,
        });
    }
}

/// wasm32 variant of [`drain_live_midi`] — identical semantics over the
/// `VecDeque`-backed queue: resolve each event's track guid via `idx_of`,
/// append to that track's bucket, drop unresolvable events.
#[cfg(target_arch = "wasm32")]
pub(crate) fn drain_live_midi(
    queue: &mut LiveMidiQueue,
    idx_of: impl Fn(&str) -> Option<usize>,
    buckets: &mut [Vec<crate::plugin::PluginMidiEvent>],
) {
    while let Some(ev) = queue.events.pop_front() {
        let Some(ti) = idx_of(&ev.track) else {
            continue;
        };
        let Some(bucket) = buckets.get_mut(ti) else {
            continue;
        };
        bucket.push(crate::plugin::PluginMidiEvent {
            offset: ev.offset,
            message: ev.message,
        });
    }
}

/// Stereo render buffer (interleaved L/R/L/R/...).
#[derive(Debug, Clone)]
pub struct StereoBuffer {
    pub samples: Vec<f32>,
    pub frames: usize,
    pub sample_rate: u32,
}

impl StereoBuffer {
    pub fn zeroed(frames: usize, sample_rate: u32) -> Self {
        Self {
            samples: vec![0.0; frames * 2],
            frames,
            sample_rate,
        }
    }

    pub fn fill(&mut self, v: f32) {
        for s in self.samples.iter_mut() {
            *s = v;
        }
    }
}

/// Pre-allocated per-block working memory, owned by the renderer and
/// reused across blocks — steady-state playback allocates nothing
/// (REAPER's model: the graph is cheap, buffers are recycled).
/// Buses are indexed by track index (== snapshot index == project
/// track index), not hashed by guid.
#[derive(Default)]
struct RenderScratch {
    /// One stereo bus per track, index == track index.
    buses: Vec<StereoBuffer>,
    /// Bus received signal this block (items / sends / children /
    /// synth FX). Clean buses skip every per-frame stage.
    dirty: Vec<bool>,
    /// Each track's channels 3/4 (interleaved), what sends to its
    /// sidechain left there this block; sized on first use, so a project
    /// without sidechains carries none.
    sidechains: Vec<Vec<f32>>,
    /// Which tracks' sidechains received this block.
    keyed: Vec<bool>,
    /// Each track's plugins' outputs beyond the main pair (interleaved,
    /// [`AUX_PAIRS`] pairs one after another), for a track sending from
    /// them; sized on first use.
    aux: Vec<Vec<f32>>,
    /// Which tracks' aux outputs carry this block's audio.
    aux_live: Vec<bool>,
    /// FX guids whose plugin panicked inside `process_block`/`prepare`.
    /// Once here they are bypassed forever (this engine run) — a plugin
    /// that panics once can't be trusted on the audio thread again.
    /// Persists across blocks (never cleared by `reset`).
    panicked_fx: std::collections::HashSet<String>,
    /// Realtime mode: the snapshot this renderer is playing (revision,
    /// snapshot) — the audio thread's own, swapped for the builder's newer
    /// one between blocks (see [`ProjectRenderer::set_realtime`]).
    current: Option<(u64, Arc<RenderSnapshot>)>,
    /// Realtime mode: the meter bank, re-read only when its lock is free.
    meters: Option<Arc<crate::metering::Meters>>,
    /// Live MIDI for this block, per track, and one track's merged events —
    /// kept across blocks so a note played does not allocate on the audio
    /// thread.
    midi_buckets: Vec<Vec<crate::plugin::PluginMidiEvent>>,
    /// The block's schedule ([`schedule`]): which track runs when.
    schedule: schedule::Schedule,
    /// Per track: what it hands on this block ([`node::Edges`]).
    edges: Vec<node::Edges>,
    /// Per track: what its node did this block.
    stats: Vec<node::NodeStats>,
    /// Per track: its time last block (µs), for the schedule's order.
    cost: Vec<u32>,
    /// One scratch per rendering thread (the render thread's last).
    workers: Vec<node::Worker>,
    /// An offline render's account so far ([`OfflineStats`]).
    offline_stats: OfflineStats,
    /// This block's delay compensation ([`pdc::plan`]).
    plan: pdc::Plan,
    /// Per track, one delay line per edge out of it (its parent or master,
    /// then each send), as [`pdc::Plan::edges`] numbers them; and one for
    /// a soloed track heard straight at master.
    delays: Vec<Vec<pdc::Delay>>,
    monitor_delays: Vec<pdc::Delay>,
    /// An offline render's taps: each track's signal this block
    /// ([`offline::Tap`]), when it taps.
    taps: Vec<Vec<f32>>,
    /// An offline render's probes ([`offline::Offline::probes`]), each its
    /// signal this block, and the track index each reads this block.
    probes: Vec<Vec<f32>>,
    probe_tracks: Vec<Option<usize>>,
}

impl RenderScratch {
    /// Size everything for `n` tracks × `frames` and zero the buses.
    /// Steady state (same shape as last block) is memset-only.
    fn reset(&mut self, n: usize, frames: usize, sample_rate: u32) {
        if self.buses.len() != n {
            self.buses
                .resize_with(n, || StereoBuffer::zeroed(frames, sample_rate));
        }
        for b in &mut self.buses {
            b.frames = frames;
            b.sample_rate = sample_rate;
            if b.samples.len() != frames * 2 {
                b.samples.resize(frames * 2, 0.0);
            }
            b.samples.fill(0.0);
        }
        self.dirty.clear();
        self.dirty.resize(n, false);
        self.sidechains.resize_with(n, Vec::new);
        for (sidechain, keyed) in self.sidechains.iter_mut().zip(&self.keyed) {
            if *keyed {
                sidechain.fill(0.0);
            }
        }
        self.keyed.clear();
        self.keyed.resize(n, false);
        self.aux.resize_with(n, Vec::new);
        for (aux, live) in self.aux.iter_mut().zip(&self.aux_live) {
            if *live {
                aux.fill(0.0);
            }
        }
        self.aux_live.clear();
        self.aux_live.resize(n, false);
        self.edges.resize_with(n, node::Edges::default);
        self.stats.resize_with(n, node::NodeStats::default);
        if self.cost.len() != n {
            self.cost.resize(n, 0);
        }
    }
}

/// How many pairs past the main one a track's plugins can put out: 3/4
/// and 5/6.
pub(crate) const AUX_PAIRS: usize = 2;

/// A track's aux buffer at its size for `frames` (zeroed when it grows).
fn aux_sized(aux: &mut Vec<f32>, frames: usize) {
    if aux.len() != AUX_PAIRS * frames * 2 {
        aux.clear();
        aux.resize(AUX_PAIRS * frames * 2, 0.0);
    }
}

/// `plugin`'s aux outputs this block, summed into `aux`
/// ([`AUX_PAIRS`] interleaved pairs, one after another). Whether it had
/// any.
pub(crate) fn add_aux(
    plugin: &dyn crate::plugin::PluginInstance,
    aux: &mut [f32],
    frames: usize,
) -> bool {
    let mut any = false;
    for pair in 0..AUX_PAIRS {
        let Some((l, r)) = plugin.aux_out(pair) else {
            continue;
        };
        let Some(out) = aux.get_mut(pair * frames * 2..(pair + 1) * frames * 2) else {
            continue;
        };
        for ((o, l), r) in out.chunks_exact_mut(2).zip(l).zip(r) {
            o[0] += *l;
            o[1] += *r;
        }
        any = true;
    }
    any
}

/// A track's post-fader peaks this block: onto its meter — or, rendered
/// ahead, into the feed, for the live render to put there when the block
/// plays.
fn meter(meters: &crate::metering::Meters, ahead: Option<&mut Feed>, ti: usize, post: &[f32]) {
    match ahead {
        Some(feed) => feed.meter(ti, post),
        None => {
            let (left, right) = part::peak(post);
            meter_peaks(meters, ti, left, right);
        }
    }
}

/// Track `ti`'s peaks this block onto its meter.
fn meter_peaks(meters: &crate::metering::Meters, ti: usize, left: f32, right: f32) {
    if let Some(cell) = meters.cell(ti) {
        cell.write(left, right, crate::metering::HOLD_DECAY);
    }
}

/// A live render's start: what the tracks rendered ahead summed into
/// master and into the live tracks' buses and sidechains, added in.
fn take_feed(
    feed: &Feed,
    master: &mut [f32],
    buses: &mut [StereoBuffer],
    dirty: &mut [bool],
    sidechains: &mut [Vec<f32>],
    keyed: &mut [bool],
    frames: usize,
) {
    for (m, f) in master.iter_mut().zip(&feed.master) {
        *m += *f;
    }
    for lane in &feed.buses {
        if let Some(bus) = buses.get_mut(lane.track) {
            for (b, f) in bus.samples.iter_mut().zip(&lane.samples) {
                *b += *f;
            }
            dirty[lane.track] = true;
        }
    }
    for lane in &feed.sidechains {
        if let Some(sidechain) = sidechains.get_mut(lane.track) {
            if sidechain.len() != frames * 2 {
                sidechain.resize(frames * 2, 0.0);
            }
            for (s, f) in sidechain.iter_mut().zip(&lane.samples) {
                *s += *f;
            }
            keyed[lane.track] = true;
        }
    }
}

/// An ahead render's end: what its tracks summed into master and into the
/// live tracks' buses and sidechains, into the feed.
fn give_feed(
    feed: &mut Feed,
    master: &[f32],
    buses: &[StereoBuffer],
    dirty: &[bool],
    sidechains: &[Vec<f32>],
    keyed: &[bool],
    live: &[bool],
) {
    feed.master.copy_from_slice(master);
    for (ti, _) in live.iter().enumerate().filter(|(_, l)| **l) {
        if dirty.get(ti).copied().unwrap_or(false) {
            feed.give_lane(ti, &buses[ti].samples, false);
        }
        if keyed.get(ti).copied().unwrap_or(false) {
            feed.give_lane(ti, &sidechains[ti], true);
        }
    }
}

/// Render a stereo master block.
///
/// Keep one renderer alive across blocks: the track snapshot is cached
/// against the project's mutation revision, so steady-state playback
/// (no edits) re-walks nothing — REAPER's "graph is cheap, only
/// rebuild on change" model — and the per-block working buffers are
/// recycled. A throwaway renderer still works, it just re-snapshots
/// and re-allocates every call.
/// How many times the renderer retries the plugin map, or one plugin's
/// lock, before going on without it — on the order of 10–50 µs.
pub(super) const PLUGIN_LOCK_SPINS: usize = 2_000;

/// Blocks rendered WITHOUT their plugin stage because a control thread held
/// the plugin map (see the try-lock in `render_block_varispeed`). For an
/// instrument track that block is silent — a dropout no CPU measure shows.
static PLUGIN_STAGE_SKIPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many blocks have rendered without their plugin stage (process-wide).
#[must_use]
pub fn plugin_stage_skips() -> u64 {
    PLUGIN_STAGE_SKIPS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Plugins bypassed for a block because a control thread held that one
/// plugin (a parameter write, its state saved) past the spin.
static PLUGIN_BUSY_SKIPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many plugin runs were bypassed that way (process-wide).
#[must_use]
pub fn plugin_busy_skips() -> u64 {
    PLUGIN_BUSY_SKIPS.load(std::sync::atomic::Ordering::Relaxed)
}

/// A point in time for the block profile — a no-op on wasm, where
/// `std::time::Instant` panics (the browser rig renders here too).
#[derive(Clone, Copy)]
struct Stamp(#[cfg(not(target_arch = "wasm32"))] std::time::Instant);

impl Stamp {
    fn now() -> Self {
        Self(
            #[cfg(not(target_arch = "wasm32"))]
            std::time::Instant::now(),
        )
    }

    /// Microseconds since `now` (0 on wasm).
    fn us(self) -> u32 {
        #[cfg(not(target_arch = "wasm32"))]
        return self.0.elapsed().as_micros().min(u128::from(u32::MAX)) as u32;
        #[cfg(target_arch = "wasm32")]
        0
    }
}

/// Where one rendered block spent its time (microseconds) — the worst
/// block of a window, for a live engine's health log: a block that runs
/// long because a lock was held or a snapshot was built reads very
/// differently from one that simply had too many voices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockProfile {
    /// The whole block.
    pub total_us: u32,
    /// Getting the project snapshot (and the scratch) — lock waits and any
    /// snapshot build land here.
    pub snapshot_us: u32,
    /// Waiting for the plugin map.
    pub plugin_lock_us: u32,
    /// Every track's FX chain (instruments included).
    pub fx_us: u32,
    /// The track whose FX chain took longest (project track index), and how
    /// long.
    pub slowest_track: u32,
    pub slowest_track_us: u32,
    /// Frames in the block.
    pub frames: u32,
    /// Every track's items played into its bus: reading their sources,
    /// stretching, fades and take envelopes.
    pub items_us: u32,
    /// The track whose items took longest (project track index), and how
    /// long.
    pub slowest_items_track: u32,
    pub slowest_items_us: u32,
}

/// The worst [`BlockProfile`] of the current window, written by one render
/// thread and taken by a reader — atomics, no lock.
#[derive(Debug, Default)]
pub struct BlockProfileCell {
    fields: [std::sync::atomic::AtomicU32; 10],
}

impl BlockProfileCell {
    fn record(&self, p: &BlockProfile) {
        use std::sync::atomic::Ordering::Relaxed;
        if p.total_us <= self.fields[0].load(Relaxed) {
            return;
        }
        let v = [
            p.total_us,
            p.snapshot_us,
            p.plugin_lock_us,
            p.fx_us,
            p.slowest_track,
            p.slowest_track_us,
            p.frames,
            p.items_us,
            p.slowest_items_track,
            p.slowest_items_us,
        ];
        for (f, x) in self.fields.iter().zip(v) {
            f.store(x, Relaxed);
        }
    }

    /// The worst block since the last take, and start a new window.
    pub fn take(&self) -> Option<BlockProfile> {
        use std::sync::atomic::Ordering::Relaxed;
        let v: Vec<u32> = self.fields.iter().map(|f| f.swap(0, Relaxed)).collect();
        (v[0] > 0).then(|| BlockProfile {
            total_us: v[0],
            snapshot_us: v[1],
            plugin_lock_us: v[2],
            fx_us: v[3],
            slowest_track: v[4],
            slowest_track_us: v[5],
            frames: v[6],
            items_us: v[7],
            slowest_items_track: v[8],
            slowest_items_us: v[9],
        })
    }
}

/// A renderer's worker pool: its own (realtime) or an offline render's,
/// borrowed from the process's.
#[cfg(not(target_arch = "wasm32"))]
enum Lanes {
    Own(parallel::LanePool),
    Offline(parallel::OfflinePool),
}

#[cfg(not(target_arch = "wasm32"))]
impl Lanes {
    fn pool(&self) -> &parallel::LanePool {
        match self {
            Self::Own(p) => p,
            Self::Offline(p) => p.pool(),
        }
    }
}

/// An offline render's account, summed over its blocks: what its one wide
/// event (`render.offline`) says when the renderer goes.
#[derive(Default)]
struct OfflineStats {
    blocks: u64,
    frames: u64,
    /// Time spent rendering blocks, µs.
    wall_us: u64,
    threads: usize,
    /// Per track (by index): its FX chain's time, µs, and whether it did
    /// anything at all.
    fx_us: Vec<u64>,
    rendered: Vec<bool>,
}

impl OfflineStats {
    fn add_block(
        &mut self,
        frames: usize,
        threads: usize,
        wall_us: u32,
        stats: &[node::NodeStats],
        nodes: &[u32],
    ) {
        self.blocks += 1;
        self.frames += frames as u64;
        self.wall_us += u64::from(wall_us);
        self.threads = self.threads.max(threads);
        if self.fx_us.len() < stats.len() {
            self.fx_us.resize(stats.len(), 0);
            self.rendered.resize(stats.len(), false);
        }
        for &ti in nodes {
            let st = &stats[ti as usize];
            if st.fx_us.is_some() || st.items_us > 0 {
                self.rendered[ti as usize] = true;
            }
            self.fx_us[ti as usize] += u64::from(st.fx_us.unwrap_or(0));
        }
    }
}

pub struct ProjectRenderer {
    daw: Standalone,
    project_guid: String,
    sample_rate: u32,
    /// `(project revision, snapshot)` — refreshed when the revision moves.
    cache: Arc<std::sync::Mutex<Option<(u64, Arc<RenderSnapshot>)>>>,
    /// Reusable block buffers. Mutex for `&self` access — the audio
    /// callback is the only steady-state caller, so it's uncontended.
    scratch: std::sync::Mutex<RenderScratch>,
    /// Live hardware input (project mode). Installed by `AudioEngine`
    /// when a track records from a hardware channel or `want_input` is
    /// set; `None` for pure-playback projects. Read once per block in
    /// stage 0, alongside the scratch lock.
    #[cfg(not(target_arch = "wasm32"))]
    live_input: std::sync::Mutex<Option<LiveInput>>,
    /// Live / programmatic MIDI queue (project mode). Installed by
    /// `AudioEngine`; events pushed by the UI thread through `Standalone`
    /// are drained once per block (before the FX stage) and merged into
    /// the target track's per-block MIDI events. `None` until installed.
    #[cfg(not(target_arch = "wasm32"))]
    live_midi: std::sync::Mutex<Option<LiveMidiQueue>>,
    /// Where a live render sends live MIDI aimed at a track it does not
    /// cover: the ahead render's queue.
    #[cfg(not(target_arch = "wasm32"))]
    midi_forward: std::sync::Mutex<Option<rtrb::Producer<LiveMidiEvent>>>,
    /// Realtime mode (see [`set_realtime`](Self::set_realtime)): the
    /// snapshot builder the audio thread hands project changes to — shared
    /// by a dual render's two renderers, so an edit is built once.
    #[cfg(not(target_arch = "wasm32"))]
    realtime: std::sync::OnceLock<Arc<SnapshotBuilder>>,
    /// The worker threads a block's tracks render on beside the render
    /// thread ([`schedule`]): a realtime renderer's own pool, or an offline
    /// render's, borrowed from the process's for its life.
    #[cfg(not(target_arch = "wasm32"))]
    lanes: std::sync::OnceLock<Lanes>,
    /// Workers an offline render asks for ([`set_render_threads`](Self::set_render_threads));
    /// `usize::MAX` until asked: the machine's default.
    #[cfg(not(target_arch = "wasm32"))]
    offline_threads: std::sync::atomic::AtomicUsize,
    /// An offline render has asked for its pool (on its first block).
    #[cfg(not(target_arch = "wasm32"))]
    pool_asked: std::sync::atomic::AtomicBool,
    /// The live set of the snapshot the last block rendered with
    /// ([`graph::live_print`]): what a dual-buffer engine checks the feeds
    /// it hands over against.
    live_print: std::sync::atomic::AtomicU64,
    /// Realtime mode, held: the snapshot moves on only when
    /// [`refresh`](Self::refresh) says so, never in the middle of a run of
    /// renders — the render ahead's renderer, which must know which
    /// snapshot each frame it renders was made from.
    held: std::sync::atomic::AtomicBool,
    /// Whether this renderer writes the backend's meter bank. A backend
    /// has one bank, by track index, for its current song; a set engine
    /// renders several songs at once and lets only the current one meter
    /// (see [`set_metering`](Self::set_metering)). Shared with a render
    /// ahead's renderer of the same song.
    metering: Arc<std::sync::atomic::AtomicBool>,
    /// The bank a renderer that does not meter writes into: empty, so
    /// every write is a no-op. Made once here, never on the audio thread.
    no_meters: Arc<crate::metering::Meters>,
    /// Each track's chain latency, as last measured ([`pdc`]): shared with
    /// a render ahead's renderer of the same song, so both plan alike.
    latencies: Arc<pdc::ChainLatencies>,
    /// What reached master in the last block was this many samples late.
    song_latency: std::sync::atomic::AtomicU32,
    /// An offline render's own plugins, solos, bypasses and taps
    /// ([`offline`]); `None` for the song's own renders.
    offline: Option<offline::Offline>,
    /// A render not realtime: its one wide event, opened on its first
    /// block, its fields written when the renderer goes ([`OfflineStats`]).
    span: std::sync::OnceLock<tracing::Span>,
}

impl Drop for ProjectRenderer {
    /// An offline render's wide event: what it rendered, how fast, on how
    /// many threads, and the tracks whose FX took longest.
    fn drop(&mut self) {
        let Some(span) = self.span.take() else {
            return;
        };
        let scratch = self
            .scratch
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stats = &scratch.offline_stats;
        if stats.blocks == 0 {
            return;
        }
        let audio_seconds = stats.frames as f64 / f64::from(self.sample_rate.max(1));
        let wall_ms = stats.wall_us as f64 / 1e3;
        let snap = self
            .cache
            .lock()
            .ok()
            .and_then(|c| c.as_ref().map(|(_, s)| s.clone()));
        let mut slowest: Vec<(usize, u64)> = stats
            .fx_us
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, us)| *us > 0)
            .collect();
        slowest.sort_unstable_by_key(|(_, us)| std::cmp::Reverse(*us));
        let slowest_fx = slowest
            .iter()
            .take(5)
            .map(|(ti, us)| {
                let name = snap
                    .as_ref()
                    .and_then(|s| s.tracks.get(*ti))
                    .map_or("?", |t| t.name.as_str());
                format!("{name}={:.1}ms", *us as f64 / 1e3)
            })
            .collect::<Vec<_>>()
            .join(", ");
        span.record("render.audio_seconds", audio_seconds);
        span.record("render.wall_ms", wall_ms);
        span.record(
            "render.realtime_factor",
            if wall_ms > 0.0 {
                audio_seconds * 1e3 / wall_ms
            } else {
                0.0
            },
        );
        span.record("render.tracks", stats.fx_us.len() as u64);
        span.record(
            "render.tracks_rendered",
            stats.rendered.iter().filter(|r| **r).count() as u64,
        );
        span.record("render.threads", stats.threads as u64);
        span.record("render.blocks", stats.blocks);
        span.record("render.slowest_fx", slowest_fx.as_str());
    }
}

/// Which tracks a snapshot renders live, and what its tracks ahead are, as
/// numbers ([`RenderSnapshot::live_print`], [`RenderSnapshot::ahead_print`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Prints {
    pub(crate) live: u64,
    pub(crate) ahead: u64,
}

/// Builds render snapshots off the audio thread, for a realtime renderer.
///
/// The audio thread must never wait: not on the project lock (every control
/// operation — a fader, a mute, a status query — holds it, and a holder
/// preempted mid-hold turns into a multi-millisecond block), and not on a
/// snapshot build (a walk of the whole project, allocating). So it only
/// ever *tries* the project lock to read the revision, asks this thread for
/// a snapshot when the revision has moved, and keeps playing the one it has
/// until the new one is handed over. A mixer change is heard a block or two
/// later; a late block is heard as a click.
#[cfg(not(target_arch = "wasm32"))]
struct SnapshotBuilder {
    /// The revision the audio thread wants built (`NO_REVISION`: none).
    wanted: Arc<std::sync::atomic::AtomicU64>,
    /// A built snapshot waiting for the audio thread.
    ready: Arc<std::sync::Mutex<Option<(u64, Arc<RenderSnapshot>)>>>,
    /// Snapshots the audio thread let go of, freed here rather than there.
    retired: Arc<std::sync::Mutex<Vec<Arc<RenderSnapshot>>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::Thread,
}

#[cfg(not(target_arch = "wasm32"))]
const NO_REVISION: u64 = u64::MAX;

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SnapshotBuilder {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.thread.unpark();
    }
}

impl ProjectRenderer {
    pub fn new(daw: &Standalone, project_guid: &str, sample_rate: u32) -> Self {
        Self {
            daw: daw.clone(),
            project_guid: project_guid.to_string(),
            sample_rate,
            cache: Arc::new(std::sync::Mutex::new(None)),
            scratch: std::sync::Mutex::new(RenderScratch::default()),
            #[cfg(not(target_arch = "wasm32"))]
            live_input: std::sync::Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            live_midi: std::sync::Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            midi_forward: std::sync::Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            realtime: std::sync::OnceLock::new(),
            #[cfg(not(target_arch = "wasm32"))]
            lanes: std::sync::OnceLock::new(),
            #[cfg(not(target_arch = "wasm32"))]
            offline_threads: std::sync::atomic::AtomicUsize::new(usize::MAX),
            #[cfg(not(target_arch = "wasm32"))]
            pool_asked: std::sync::atomic::AtomicBool::new(false),
            live_print: std::sync::atomic::AtomicU64::new(0),
            held: std::sync::atomic::AtomicBool::new(false),
            metering: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            no_meters: crate::metering::Meters::empty(),
            latencies: Arc::default(),
            song_latency: std::sync::atomic::AtomicU32::new(0),
            offline: None,
            span: std::sync::OnceLock::new(),
        }
    }

    /// Whether this renderer renders offline (not for a realtime callback).
    fn is_offline_render(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.realtime.get().is_none();
        #[cfg(target_arch = "wasm32")]
        true
    }

    /// How late what reaches master was in the last block rendered,
    /// samples: the song's plugin delay, compensated everywhere inside it
    /// ([`pdc`]). A bounce takes this many samples off its front.
    pub fn latency(&self) -> u32 {
        self.song_latency.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether this renderer's blocks write the backend's meter bank (on
    /// by default). Any thread; takes effect on the next block.
    pub fn set_metering(&self, on: bool) {
        self.metering
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// Meter (or not) together with `with`: one switch for a song's two
    /// renderers (a render ahead's and the callback's).
    #[cfg(all(not(target_arch = "wasm32"), feature = "audio"))]
    pub(crate) fn share_metering(&mut self, with: &ProjectRenderer) {
        self.metering = with.metering.clone();
    }

    /// Render for a realtime callback: from here on the render thread never
    /// blocks on the project lock or the meter lock and never builds a
    /// snapshot — see [`SnapshotBuilder`]. Project edits reach the audio a
    /// block or two late. Offline renders (tests, bounces) leave this off
    /// and see every edit on the very next block. Both render a block the
    /// same way, on several threads ([`schedule`]): a realtime renderer on
    /// its own pool at the callback's priority (`FTS_RENDER_THREADS`
    /// workers, else a few), an offline one on a pool sized to the machine
    /// ([`set_render_threads`](Self::set_render_threads)). Idempotent.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_realtime(&self) {
        self.go_realtime("render-lane", 0, parallel::default_threads());
    }

    /// [`set_realtime`](Self::set_realtime) for the renderer of a render
    /// ahead: its lane pool its own, named for it and running below the
    /// audio callback.
    #[cfg(all(not(target_arch = "wasm32"), feature = "audio"))]
    pub(crate) fn set_realtime_ahead(&self) {
        self.held.store(true, std::sync::atomic::Ordering::Relaxed);
        self.go_realtime(
            "render-ahead",
            audio::io::realtime::AHEAD_PRIORITY,
            parallel::ahead_threads(),
        );
    }

    /// Take snapshots from `from`'s builder (it in realtime mode already):
    /// an edit is built once for both, and both see the same snapshot.
    /// Before this one goes realtime.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn share_snapshots(&mut self, from: &ProjectRenderer) {
        if let Some(builder) = from.realtime.get() {
            let _ = self.realtime.set(builder.clone());
        }
        // One latency table for both: their plans agree.
        self.latencies = from.latencies.clone();
    }

    /// Hold this realtime renderer's snapshot: it moves on only at
    /// [`refresh`](Self::refresh) — for a dual render, which must know which
    /// live set the block it renders is of before it renders it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn hold(&self) {
        self.held.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// A held renderer's snapshot moved on to the newest built (and a newer
    /// one asked for when the project has moved on): the
    /// [ahead print](RenderSnapshot::ahead_print) and the live print of
    /// the snapshot its renders use from here on. `None` before realtime
    /// mode, or with no song.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn refresh(&self) -> Option<Prints> {
        let b = self.realtime.get()?;
        let mut scratch = self
            .scratch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snap = self.snapshot_rt(b, &mut scratch.current)?;
        Some(Prints {
            ahead: snap.ahead_print,
            live: snap.live_print,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn go_realtime(&self, pool_name: &str, priority: i32, threads: usize) {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        if threads > 0 {
            let pool = self.lanes.get_or_init(|| {
                Lanes::Own(parallel::LanePool::named(threads, pool_name, priority))
            });
            tracing::debug!(workers = pool.pool().threads(), "render: lane pool");
        }
        self.realtime.get_or_init(|| {
            let wanted = Arc::new(AtomicU64::new(NO_REVISION));
            let ready = Arc::new(std::sync::Mutex::new(None));
            let retired = Arc::new(std::sync::Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (daw, guid) = (self.daw.clone(), self.project_guid.clone());
            let (w, r, g, st) = (wanted.clone(), ready.clone(), retired.clone(), stop.clone());
            let handle = std::thread::Builder::new()
                .name("render-snapshot".into())
                .spawn(move || {
                    loop {
                        std::thread::park_timeout(std::time::Duration::from_millis(250));
                        if st.load(Ordering::Acquire) {
                            return;
                        }
                        // What the audio thread let go of is freed here.
                        if let Ok(mut old) = g.lock() {
                            old.clear();
                        }
                        if w.swap(NO_REVISION, Ordering::AcqRel) == NO_REVISION {
                            continue;
                        }
                        let hints = daw.live_hints();
                        let built = daw.read_project(&guid, |p| {
                            (p.revision, RenderSnapshot::build_live(p, &hints))
                        });
                        if let Some(built) = built {
                            // The one it replaces is dropped here, on this
                            // thread, unless a renderer still plays it.
                            let stale = r.lock().ok().and_then(|mut slot| slot.replace(built));
                            drop(stale);
                        }
                    }
                })
                .expect("spawn render-snapshot thread");
            Arc::new(SnapshotBuilder {
                wanted,
                ready,
                retired,
                stop,
                thread: handle.thread().clone(),
            })
        });
    }

    /// Realtime mode's snapshot: the one being played, swapped for the
    /// builder's newer one if it has handed one over, and a newer one asked
    /// for when the project has moved on. Never waits for a lock; builds
    /// only the very first snapshot here (before there is anything to play).
    #[cfg(not(target_arch = "wasm32"))]
    fn snapshot_rt(
        &self,
        b: &SnapshotBuilder,
        current: &mut Option<(u64, Arc<RenderSnapshot>)>,
    ) -> Option<Arc<RenderSnapshot>> {
        use std::sync::atomic::Ordering;
        // Take the newest snapshot built, if newer than the one played
        // (the builder may serve two renderers: it is copied, not taken).
        let handed = match b.ready.try_lock() {
            Ok(slot) => slot
                .as_ref()
                .filter(|(rev, _)| current.as_ref().is_none_or(|(now, _)| rev > now))
                .cloned(),
            Err(_) => None,
        };
        if let Some(new) = handed {
            if let Some((_, old)) = current.replace(new) {
                match b.retired.try_lock() {
                    Ok(mut bin) => bin.push(old),
                    // The builder is emptying it this instant: the old one
                    // is freed here — rare, and still no wait.
                    Err(_) => drop(old),
                }
            }
        }
        // Has the project moved on? Read without waiting.
        let revision = self.daw.try_project_revision(&self.project_guid);
        match (&*current, revision) {
            (Some((rev, _)), Some(now)) if *rev != now => {
                if b.wanted.swap(now, Ordering::AcqRel) != now {
                    b.thread.unpark();
                }
            }
            (None, _) => {
                // Nothing to play yet: the first snapshot is built here.
                let hints = self.daw.live_hints();
                let built = self.daw.read_project(&self.project_guid, |p| {
                    (p.revision, RenderSnapshot::build_live(p, &hints))
                })?;
                *current = Some(built);
            }
            _ => {}
        }
        current.as_ref().map(|(_, s)| s.clone())
    }

    /// How many worker threads an offline render (one not
    /// [realtime](Self::set_realtime)) renders on beside the calling one: 0
    /// renders on the calling thread alone. Before its first block; by
    /// default the machine's (`FTS_OFFLINE_RENDER_THREADS`, else its cores
    /// but two). The samples are the same however many.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_render_threads(&self, workers: usize) {
        self.offline_threads
            .store(workers, std::sync::atomic::Ordering::Relaxed);
    }

    /// The pool this render's blocks run on, if any: a realtime renderer's
    /// own, else an offline render's, taken on its first block.
    #[cfg(not(target_arch = "wasm32"))]
    fn lanes(&self) -> Option<&parallel::LanePool> {
        // Asked once, on the first block: a render keeps the threads it
        // began with (and never spawns any in the middle).
        if self.realtime.get().is_none()
            && self.lanes.get().is_none()
            && !self
                .pool_asked
                .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let asked = self
                .offline_threads
                .load(std::sync::atomic::Ordering::Relaxed);
            let workers = if asked == usize::MAX {
                parallel::offline_threads()
            } else {
                asked
            };
            if let Some(pool) = parallel::OfflinePool::take(workers, asked == usize::MAX) {
                let _ = self.lanes.set(Lanes::Offline(pool));
            }
        }
        self.lanes.get().map(Lanes::pool)
    }

    /// The live set of the snapshot the last block rendered with.
    pub(crate) fn live_print(&self) -> u64 {
        self.live_print.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The backend this renderer reads its project from. Lets the cpal
    /// callback reach the shared `Meters` bank without a second handle.
    pub(crate) fn daw(&self) -> &Standalone {
        &self.daw
    }

    /// The song this renderer renders.
    pub(crate) fn project_guid(&self) -> &str {
        &self.project_guid
    }

    /// Install the live hardware-input ring consumer (project mode).
    /// `AudioEngine` calls this after opening the cpal input stream so
    /// stage 0 can read mic/DI channels into the matching track buses.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_live_input(&self, cons: rtrb::Consumer<f32>, channels: usize) {
        if let Ok(mut slot) = self.live_input.lock() {
            *slot = Some(LiveInput {
                cons,
                channels,
                scratch: Vec::new(),
                underruns: 0,
                resyncs: 0,
            });
        }
    }

    /// Install the live / programmatic MIDI ring consumer (project mode).
    /// The producer end is fed by `Standalone::push_note_on`/`_off`/`_cc`.
    /// Mirrors `set_live_input`: stage 0.5 drains it once per block and
    /// merges events into the matching track's MIDI event list.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_live_midi(&self, cons: rtrb::Consumer<LiveMidiEvent>) {
        if let Ok(mut slot) = self.live_midi.lock() {
            *slot = Some(LiveMidiQueue { cons });
        }
    }

    /// Send the live MIDI a [`Part::Live`] render drains for a track it
    /// does not cover to `to` (the ahead render's queue).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_midi_forward(&self, to: rtrb::Producer<LiveMidiEvent>) {
        if let Ok(mut slot) = self.midi_forward.lock() {
            *slot = Some(to);
        }
    }

    /// Wire a fresh live-MIDI ring between this renderer and its backend —
    /// the headless-native path (no `AudioEngine`): after this,
    /// `Standalone::push_live_midi` feeds this renderer's per-block drain,
    /// exactly as under a running engine. Replaces any previous ring.
    #[cfg(all(not(target_arch = "wasm32"), feature = "audio"))]
    pub fn connect_live_midi(&self, capacity: usize) {
        let (prod, cons) = rtrb::RingBuffer::new(capacity.max(1));
        self.daw.set_live_midi_producer(prod);
        self.set_live_midi(cons);
    }

    /// Render `frames` stereo frames starting at `start_frame` (in
    /// output-rate samples). Returns a fresh `StereoBuffer`.
    pub fn render_block(&self, start_frame: u64, frames: usize) -> StereoBuffer {
        self.render_block_varispeed(start_frame as f64, frames, 1.0)
    }

    /// Render one audio buffer as the transport planned it
    /// ([`TransportShared::begin_block`](crate::transport_engine::TransportShared::begin_block)):
    /// each run at its own start and varispeed rate, a scheduled locate
    /// landing on its exact frame.
    ///
    /// `render_stopped`: a run the transport does not roll through is
    /// still rendered at its frozen position, 1x (the duplex path — live
    /// input and FX run while stopped); otherwise it is silence.
    ///
    /// A single run renders as one block, exactly as
    /// [`render_block_varispeed`](Self::render_block_varispeed). A buffer
    /// split by a locate, when only one side plays, renders the full
    /// buffer from that side's clock and silences the other side — so a
    /// plugin never sees its first block short (it sizes itself on its
    /// first block).
    pub fn render_plan(
        &self,
        plan: &crate::transport_engine::BlockPlan,
        render_stopped: bool,
    ) -> StereoBuffer {
        let segs = plan.segments();
        let total: usize = segs.iter().map(|s| s.frames).sum();
        let run = |seg: &crate::transport_engine::BlockSegment| -> Option<StereoBuffer> {
            if seg.playing {
                Some(self.render_block_varispeed(seg.start_samples, seg.frames, seg.rate))
            } else if render_stopped {
                let _stopped = crate::plugin::StoppedRender::enter();
                Some(self.render_block_varispeed(
                    seg.start_samples.max(0.0).floor(),
                    seg.frames,
                    1.0,
                ))
            } else {
                None
            }
        };
        match segs {
            [one] => run(one).unwrap_or_else(|| StereoBuffer::zeroed(total, self.sample_rate)),
            [a, b] if !render_stopped && a.playing != b.playing => {
                // One side plays: render the whole buffer on its clock,
                // silence the side that does not.
                let (on, silent) = if a.playing {
                    (a, b.offset..total)
                } else {
                    (b, 0..b.offset)
                };
                let mut out = self.render_block_varispeed(on.start_at_frame_zero(), total, on.rate);
                if let Some(s) = out.samples.get_mut(silent.start * 2..silent.end * 2) {
                    s.fill(0.0);
                }
                out
            }
            _ => {
                let mut out = StereoBuffer::zeroed(total, self.sample_rate);
                for seg in segs {
                    let Some(block) = run(seg) else { continue };
                    let dst = seg.offset * 2..(seg.offset + seg.frames) * 2;
                    if let (Some(d), Some(src)) = (
                        out.samples.get_mut(dst),
                        block.samples.get(..seg.frames * 2),
                    ) {
                        d.copy_from_slice(src);
                    }
                }
                out
            }
        }
    }

    /// [`render_plan`](Self::render_plan) into `out`, its buffer kept:
    /// a single run — every plan but a locate landing mid-buffer —
    /// allocates nothing once `out` has held a block this size.
    pub fn render_plan_into(
        &self,
        plan: &crate::transport_engine::BlockPlan,
        render_stopped: bool,
        out: &mut StereoBuffer,
    ) {
        match plan.segments() {
            [seg] if seg.playing => {
                self.render_part_into(seg.start_samples, seg.frames, seg.rate, Part::Whole, out);
            }
            [seg] if render_stopped => {
                let _stopped = crate::plugin::StoppedRender::enter();
                self.render_part_into(
                    seg.start_samples.max(0.0).floor(),
                    seg.frames,
                    1.0,
                    Part::Whole,
                    out,
                );
            }
            _ => *out = self.render_plan(plan, render_stopped),
        }
    }

    /// Render `frames` stereo frames of the timeline from `start_samples`
    /// (output-rate samples, fractional allowed) at transport `rate`:
    /// output frame `i` is the timeline at `start_samples + i * rate` —
    /// true varispeed (resampled, so pitch moves with the rate), not a
    /// playhead that skips. Everything timeline-driven follows: item
    /// sources, take / track / send envelopes, MIDI and the metronome.
    /// The next block continues from `start_samples + frames * rate`.
    ///
    /// At `rate == 1.0` with an integer start this is bit-identical to
    /// [`render_block`](Self::render_block).
    pub fn render_block_varispeed(
        &self,
        start_samples: f64,
        frames: usize,
        rate: f64,
    ) -> StereoBuffer {
        self.render_part(start_samples, frames, rate, Part::Whole)
    }

    /// [`render_block_varispeed`](Self::render_block_varispeed) over the
    /// tracks `part` covers — the dual-buffer engine's two halves (see
    /// [`part`]). A [`Part::Ahead`] render returns master unfinished and
    /// leaves what it hands the live tracks in its feed; a [`Part::Live`]
    /// render starts from the feed and finishes the block.
    pub(crate) fn render_part(
        &self,
        start_samples: f64,
        frames: usize,
        rate: f64,
        part: Part<'_>,
    ) -> StereoBuffer {
        let mut out = StereoBuffer::zeroed(0, self.sample_rate);
        self.render_part_into(start_samples, frames, rate, part, &mut out);
        out
    }

    /// [`render_part`](Self::render_part) into `out`, its buffer kept: a
    /// render on the audio thread allocates nothing for its output once it
    /// has held a block this size.
    pub(crate) fn render_part_into(
        &self,
        start_samples: f64,
        frames: usize,
        rate: f64,
        part: Part<'_>,
        out: &mut StereoBuffer,
    ) {
        let side = part.side();
        let (mut ahead, from_ahead) = part.feeds();
        let rate = if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            1.0
        };
        let _frame = crate::plugin::RenderFrameGuard::enter(start_samples.max(0.0).floor() as u64);
        let master = out;
        master.frames = frames;
        master.sample_rate = self.sample_rate;
        master.samples.clear();
        master.samples.resize(frames * 2, 0.0);
        if frames == 0 {
            return;
        }

        let t_start = Stamp::now();
        if self.is_offline_render() {
            self.span.get_or_init(|| {
                tracing::info_span!(
                    "render.offline",
                    render.copies = self.offline.is_some(),
                    render.sample_rate = self.sample_rate,
                    render.audio_seconds = tracing::field::Empty,
                    render.wall_ms = tracing::field::Empty,
                    render.realtime_factor = tracing::field::Empty,
                    render.tracks = tracing::field::Empty,
                    render.tracks_rendered = tracing::field::Empty,
                    render.threads = tracing::field::Empty,
                    render.blocks = tracing::field::Empty,
                    render.slowest_fx = tracing::field::Empty,
                )
            });
        }
        let mut scratch = self
            .scratch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(not(target_arch = "wasm32"))]
        let snap = match self.realtime.get() {
            Some(_) if self.held.load(std::sync::atomic::Ordering::Relaxed) => {
                match scratch.current.as_ref() {
                    Some((_, s)) => Some(s.clone()),
                    None => {
                        drop(scratch);
                        let _ = self.refresh();
                        scratch = self
                            .scratch
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        scratch.current.as_ref().map(|(_, s)| s.clone())
                    }
                }
            }
            Some(b) => self.snapshot_rt(b, &mut scratch.current),
            None => self.snapshot(),
        };
        #[cfg(target_arch = "wasm32")]
        let snap = self.snapshot();
        let Some(snap) = snap else {
            return;
        };
        let snapshot_us = t_start.us();
        let tracks = &snap.tracks;
        let n = tracks.len();
        let start_seconds = start_samples / self.sample_rate as f64;
        let end_seconds = start_seconds + (frames as f64 * rate / self.sample_rate as f64);
        // The block's clock; each plugin is told it held back by how late
        // its own audio is (`transport_late`).
        let playing = !crate::plugin::rendering_stopped();
        let block_transport = crate::plugin::RenderTransport {
            playing,
            ..snap.transport_at(start_seconds)
        };
        _frame.set_transport(block_transport);
        // The clock of audio `late` samples behind the block: what a plugin
        // on a track delay compensation holds back is processing.
        let transport_late = |late: u32| {
            if late == 0 {
                return block_transport;
            }
            crate::plugin::RenderTransport {
                playing,
                ..snap
                    .transport_at(start_seconds - f64::from(late) * rate / self.sample_rate as f64)
            }
        };
        // Output frames per timeline second (MIDI event offsets).
        let frames_per_second = self.sample_rate as f64 / rate;
        // Where FX automation is read: the same clock for every plugin, and
        // for an offline render as for the live one.
        let clock = envelope::BlockClock {
            start: start_seconds,
            seconds_per_frame: rate / self.sample_rate as f64,
            frames: frames as u32,
            sample_rate: self.sample_rate as f64,
        };
        let any_soloed = snap.solo_pass.is_some();
        let pass = |i: usize| -> graph::Pass {
            match &snap.solo_pass {
                Some(p) => p[i],
                None => graph::Pass::On,
            }
        };
        let passes = |i: usize| -> bool { pass(i).passes() };
        let live = snap.live.as_slice();
        let covers = |i: usize| side.covers(live, i);
        self.live_print
            .store(snap.live_print, std::sync::atomic::Ordering::Relaxed);
        if let Some(feed) = ahead.as_deref_mut() {
            feed.reset(frames, snap.live_print, n);
        }
        // A feed of another live set lands on the wrong buses: none then.
        let from_ahead = from_ahead.filter(|f| f.print == snap.live_print && f.frames == frames);

        // Poison-tolerant lock: a control-thread panic while holding one of
        // these mutexes must NOT take down the audio callback (a panic here
        // unwinds across the extern "C" PipeWire boundary → abort → process
        // death mid-song). The guarded state is plain buffers/maps — safe to
        // keep using after another thread's panic.
        scratch.reset(n, frames, self.sample_rate);
        // Delay compensation for this block, from the latencies the chains
        // reported last block.
        {
            let latencies = &self.latencies;
            pdc::plan(
                &snap.tracks,
                &snap.order,
                |ti| latencies.get(ti),
                &mut scratch.plan,
            );
            self.song_latency
                .store(scratch.plan.master, std::sync::atomic::Ordering::Relaxed);
            let s = &mut *scratch;
            s.delays.resize_with(n, Vec::new);
            s.monitor_delays.resize_with(n, pdc::Delay::default);
            for (lines, edges) in s.delays.iter_mut().zip(&s.plan.edges) {
                lines.resize_with(edges.len(), pdc::Delay::default);
            }
            let tapping = self.offline.as_ref().is_some_and(|o| o.tap.is_some());
            if tapping {
                s.taps.resize_with(n, Vec::new);
                for tap in &mut s.taps {
                    tap.clear();
                    tap.resize(frames * 2, 0.0);
                }
            }
            let asked = self
                .offline
                .as_ref()
                .map_or(&[][..], |o| o.probes.as_slice());
            s.probes.resize_with(asked.len(), Vec::new);
            for probe in &mut s.probes {
                probe.clear();
                probe.resize(frames * 2, 0.0);
            }
            s.probe_tracks.clear();
            s.probe_tracks.extend(
                asked
                    .iter()
                    .map(|p| tracks.iter().position(|t| t.guid == p.track)),
            );
        }
        // The meter bank. In realtime mode, re-read only when its lock is
        // free (its readers are UI polls) — the cached one otherwise.
        #[cfg(not(target_arch = "wasm32"))]
        let meters = if !self.metering.load(std::sync::atomic::Ordering::Relaxed) {
            self.no_meters.clone()
        } else if self.realtime.get().is_some() {
            if let Ok(m) = self.daw.meters.try_lock() {
                if !scratch.meters.as_ref().is_some_and(|c| Arc::ptr_eq(c, &m)) {
                    scratch.meters = Some(m.clone());
                }
            }
            scratch.meters.clone().unwrap_or_else(|| self.daw.meters())
        } else {
            self.daw.meters()
        };
        #[cfg(target_arch = "wasm32")]
        let meters = if self.metering.load(std::sync::atomic::Ordering::Relaxed) {
            self.daw.meters()
        } else {
            self.no_meters.clone()
        };
        // Split-borrow every scratch field so the stages below can
        // hold disjoint &muts simultaneously.
        let RenderScratch {
            buses,
            dirty,
            sidechains,
            keyed,
            aux,
            aux_live,
            panicked_fx,
            midi_buckets,
            schedule,
            edges,
            stats,
            cost,
            workers,
            offline_stats,
            plan,
            delays,
            monitor_delays,
            taps,
            probes,
            probe_tracks,
            ..
        } = &mut *scratch;
        let asked = self
            .offline
            .as_ref()
            .map_or(&[][..], |o| o.probes.as_slice());
        // Live taps (a song playing, not an offline render): which track and
        // point each open one reads, worked out once a block.
        #[cfg(any(feature = "decode", feature = "audio"))]
        let (live_taps, live_plan) = {
            let taps = &*self.daw.live_taps;
            let mut plan = [None; live_tap::SLOTS];
            if self.offline.is_none() && taps.any() {
                taps.plan(
                    &self.project_guid,
                    |g| tracks.iter().position(|t| t.guid == g),
                    &mut plan,
                );
            }
            (taps, plan)
        };
        #[cfg(any(feature = "decode", feature = "audio"))]
        let live_on = live_plan.iter().any(Option::is_some);
        #[cfg(not(any(feature = "decode", feature = "audio")))]
        let live_on = false;
        let probing = !asked.is_empty() || live_on;
        let latencies = &*self.latencies;
        let tap_point = self.offline.as_ref().and_then(|o| o.tap);

        // What the tracks rendered ahead handed over: their sums onto the
        // live buses and master, their meters as of these frames.
        if let Some(feed) = from_ahead {
            take_feed(
                feed,
                &mut master.samples,
                buses,
                dirty,
                sidechains,
                keyed,
                frames,
            );
            for (ti, peaks) in feed.peaks.iter().enumerate() {
                if let (Some((l, r)), false) = (peaks, covers(ti)) {
                    meter_peaks(&meters, ti, *l, *r);
                }
            }
        }

        // 0) Live hardware input → per-track buses. Tracks whose
        // `RecordInput::Audio { channel }` taps a valid input channel
        // get that channel's mono sample broadcast to L+R of their bus
        // (and marked dirty); the FX / gain / send / master stages then
        // process it like any other source. No-op when no input stream
        // is installed or no track taps a channel.
        #[cfg(not(target_arch = "wasm32"))]
        if side.finishes() {
            let mut live_guard = self
                .live_input
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(live) = live_guard.as_mut() {
                mix_live_input_into_buses(live, &snap.input_channels, passes, buses, dirty, frames);
            }
            // 0.1) Recording: the armed tracks' input this block, pushed to
            // the recording pass's rings (lock-free, never waits in
            // realtime) — see `crate::recording`.
            crate::recording::capture_block(
                &self.daw,
                &crate::recording::Block {
                    project_guid: &self.project_guid,
                    // Its snapshot cache's heap address: stable while it lives.
                    renderer: Arc::as_ptr(&self.cache) as usize,
                    sample_rate: self.sample_rate,
                    start_samples,
                    frames,
                    rate,
                    offline: self.realtime.get().is_none(),
                    live: live_guard
                        .as_ref()
                        .map(|l| (l.scratch.as_slice(), l.channels)),
                },
            );
        }

        // 0.5) Live / programmatic MIDI → per-track scratch buckets,
        // drained once per block; each track's node merges its bucket into
        // its chain's events. Native: the live-MIDI ring is `rtrb`-backed;
        // on wasm32 the queue is a `VecDeque` on `Standalone`.
        #[allow(unused_mut)]
        let live_midi_buckets = midi_buckets;
        for b in live_midi_buckets.iter_mut() {
            b.clear();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut midi_guard = self
                .live_midi
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(queue) = midi_guard.as_mut()
                && queue.cons.slots() > 0
            {
                live_midi_buckets.resize_with(n, Vec::new);
                let idx_of =
                    |guid: &str| -> Option<usize> { tracks.iter().position(|t| t.guid == guid) };
                let mut forward = self
                    .midi_forward
                    .try_lock()
                    .ok()
                    .filter(|_| side == part::Side::Live);
                drain_live_midi_routed(
                    queue,
                    idx_of,
                    live_midi_buckets,
                    covers,
                    forward.as_mut().and_then(|f| f.as_mut()),
                );
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let events = self.daw.take_live_midi_wasm();
            if !events.is_empty() {
                live_midi_buckets.resize_with(n, Vec::new);
                let idx_of =
                    |guid: &str| -> Option<usize> { tracks.iter().position(|t| t.guid == guid) };
                let mut queue = LiveMidiQueue { events };
                drain_live_midi(&mut queue, idx_of, live_midi_buckets);
            }
        }

        // Debug tracing: FTS_RENDER_DEBUG=1 dumps per-track bus RMS at
        // each stage so silent-master bugs can be localized headlessly.
        // Cached — render_block runs on the audio thread.
        static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let debug = *DEBUG.get_or_init(|| std::env::var("FTS_RENDER_DEBUG").is_ok());

        // Plugin instances for the FX chains. The map lives apart from
        // ProjectState so the audio thread doesn't block project mutations.
        //
        // TRY, never block. A control thread holds it while it swaps an
        // instrument in — normally microseconds, but the audio callback must
        // not gamble on that (and in a wasm AudioWorklet a contended `Mutex`
        // parks the audio thread). Spin briefly, then render this block
        // WITHOUT its plugin stage: dry audio for one block, not an underrun.
        let t_lock = Stamp::now();
        let plugins = self
            .offline
            .as_ref()
            .map_or(&self.daw.plugin_instances, |o| &o.plugins)
            .try_read_spinning(PLUGIN_LOCK_SPINS);
        if plugins.is_none() {
            PLUGIN_STAGE_SKIPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let plugin_lock_us = t_lock.us();

        // Each track's cost, counted for a performance meter from the
        // realtime renders only (a bounce is not the song playing).
        #[cfg(not(target_arch = "wasm32"))]
        let costs = self.realtime.get().map(|_| &*self.daw.track_costs);
        #[cfg(target_arch = "wasm32")]
        let costs: Option<&super::perf::TrackCosts> = None;

        // 1–4) Every track this render covers, as a node of the routing
        // graph ([`schedule`], [`node`]): its media, the sums into it, its
        // FX, fader, sends and parent share — on as many threads as the
        // pool has, each track as soon as what feeds it is done.
        #[cfg(not(target_arch = "wasm32"))]
        let pool = self.lanes();
        #[cfg(target_arch = "wasm32")]
        let pool: Option<()> = None;
        let map = plugins.as_deref();
        // A track runs on the render thread when one of its plugins must
        // (not `parallel_safe`) — or when it cannot be asked this block.
        let pinned = |ti: usize| -> bool {
            if pool.is_none() {
                return false;
            }
            let t = &tracks[ti];
            t.fx_chain.iter().enumerate().any(|(i, fx_guid)| {
                if !t.fx_enabled.get(i).copied().unwrap_or(true) || panicked_fx.contains(fx_guid) {
                    return false;
                }
                let Some(cell) = map.and_then(|m| m.cell(fx_guid)) else {
                    return false;
                };
                cell.try_lock_spinning(PLUGIN_LOCK_SPINS)
                    .is_none_or(|p| !p.parallel_safe())
            })
        };
        schedule.build(
            tracks,
            &snap.order,
            covers,
            |ti| u64::from(cost.get(ti).copied().unwrap_or(0)),
            pinned,
        );
        // Input probes of the tracks no node renders here: what their
        // buses hold (their media is another render's).
        if probing {
            for ti in (0..n).filter(|&ti| schedule.pos[ti] == schedule::NOT_A_NODE) {
                for ((p, track), dst) in
                    asked.iter().zip(probe_tracks.iter()).zip(probes.iter_mut())
                {
                    if *track == Some(ti) && p.at == offline::Probe::Input {
                        let k = dst.len().min(buses[ti].samples.len());
                        dst[..k].copy_from_slice(&buses[ti].samples[..k]);
                    }
                }
                #[cfg(any(feature = "decode", feature = "audio"))]
                for (slot, planned) in live_plan.iter().enumerate() {
                    if *planned == Some((ti, offline::Probe::Input)) {
                        live_taps.write(slot, &buses[ti].samples[..frames * 2]);
                    }
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let threads = pool.map_or(1, |p| p.threads() + 1);
        #[cfg(target_arch = "wasm32")]
        let threads = 1;
        workers.resize_with(threads.max(workers.len()), node::Worker::default);
        let block = node::Block {
            snap: &snap,
            tracks,
            sched: schedule,
            sample_rate: self.sample_rate,
            frames,
            rate,
            start_samples,
            start_seconds,
            end_seconds,
            frames_per_second,
            clock,
            block_transport,
            transport_late: &transport_late,
            plan,
            latencies,
            plugins: map,
            panicked_fx,
            meters: &meters,
            feed_meter: ahead.as_deref_mut().map(Feed::meter_cells),
            costs,
            offline: self.offline.as_ref(),
            tap_point,
            probing,
            asked,
            probe_tracks,
            probes: schedule::Cells::new(probes),
            #[cfg(any(feature = "decode", feature = "audio"))]
            live_taps,
            #[cfg(any(feature = "decode", feature = "audio"))]
            live_plan,
            debug,
            buses: schedule::Cells::new(buses),
            dirty: schedule::Cells::new(dirty),
            sidechains: schedule::Cells::new(sidechains),
            keyed: schedule::Cells::new(keyed),
            aux: schedule::Cells::new(aux),
            aux_live: schedule::Cells::new(aux_live),
            delays: schedule::Cells::new(delays),
            monitor_delays: schedule::Cells::new(monitor_delays),
            taps: schedule::Cells::new(taps),
            edges: schedule::Cells::new(edges),
            midi_buckets: schedule::Cells::new(live_midi_buckets),
            stats: schedule::Cells::new(stats),
            workers: schedule::Cells::new(workers),
        };
        #[cfg(not(target_arch = "wasm32"))]
        block.render(pool);
        #[cfg(target_arch = "wasm32")]
        block.render();
        drop(block);
        for &ti in &snap.order {
            if schedule
                .pos
                .get(ti)
                .is_none_or(|&p| p == schedule::NOT_A_NODE)
            {
                continue;
            }
            let out = &edges[ti];
            if out.live.first().copied().unwrap_or(false) && out.share == node::Share::Master {
                for (m, s) in master.samples.iter_mut().zip(out.edge(0, frames)) {
                    *m += *s;
                }
            }
        }
        // What the nodes did: their plugins' panics (bypassed from now on),
        // their costs, the block's profile.
        let (mut fx_us, mut slowest) = (0u32, (0u32, 0u32));
        let (mut items_us, mut slowest_items) = (0u32, (0u32, 0u32));
        for &ti in schedule.nodes() {
            let ti = ti as usize;
            let st = stats[ti];
            if let Some((i, in_prepare)) = st.panicked
                && let Some(fx_guid) = tracks[ti].fx_chain.get(i)
            {
                panicked_fx.insert(fx_guid.clone());
                tracing::error!(
                    "plugin {fx_guid} PANICKED in {} on track '{}' — bypassing it from now on",
                    if in_prepare {
                        "prepare"
                    } else {
                        "process_block"
                    },
                    tracks[ti].name,
                );
            }
            let us = st.fx_us.unwrap_or(0);
            fx_us = fx_us.saturating_add(us);
            if us > slowest.1 {
                slowest = (ti as u32, us);
            }
            items_us = items_us.saturating_add(st.items_us);
            if st.items_us > slowest_items.1 {
                slowest_items = (ti as u32, st.items_us);
            }
            cost[ti] = us.saturating_add(st.items_us);
        }
        if self.is_offline_render() {
            offline_stats.add_block(frames, threads, t_start.us(), stats, schedule.nodes());
        }

        // Rendered ahead: what reached the live tracks and master is the
        // feed; the live render finishes the block.
        if let Some(feed) = ahead {
            give_feed(feed, &master.samples, buses, dirty, sidechains, keyed, live);
            return;
        }

        let inv_rate = 1.0 / self.sample_rate as f64;
        // Metronome click: short sine bursts on the beat grid,
        // accented on measure starts. Overlaid pre-master-fader so
        // the click rides the master level like REAPER's default
        // routing. Gated by the transport engine's atomic (no
        // snapshot rebuild on toggle).
        // Peek the engine map WITHOUT creating an engine —
        // `transport_engine_for` lazily spawns pump tasks (needs a
        // runtime), and offline render paths run without one. No
        // engine yet ⇒ transport never started ⇒ no click.
        let metronome_on = self
            .daw
            .transport_engines
            .lock()
            .ok()
            .and_then(|m| m.get(&self.project_guid).map(|b| b.shared.metronome()))
            .unwrap_or(false);
        if metronome_on {
            let bpm_grid = &snap.tempo_map;
            let first_beat = bpm_grid.seconds_to_beat(start_seconds).ceil() as i64;
            let last_beat = bpm_grid.seconds_to_beat(end_seconds).floor() as i64;
            // Also catch a click whose tail started before this block.
            let click_len = 0.012f64; // 12ms burst
            for beat in (first_beat - 1)..=last_beat {
                if beat < 0 {
                    continue;
                }
                let t0 = bpm_grid.beat_to_seconds(beat as f64);
                if t0 + click_len <= start_seconds || t0 >= end_seconds {
                    continue;
                }
                let accent = (beat as u64).is_multiple_of(snap.beats_per_measure as u64);
                let freq = if accent { 1568.0 } else { 1046.5 }; // G6 / C6
                let gain = if accent { 0.5 } else { 0.35 };
                for frame in 0..frames {
                    let t = start_seconds + frame as f64 * rate * inv_rate - t0;
                    if t < 0.0 || t >= click_len {
                        continue;
                    }
                    // Exponential-ish decay envelope.
                    let env = (1.0 - t / click_len).powi(2);
                    let sample = ((t * freq * std::f64::consts::TAU).sin() * env * gain) as f32;
                    master.samples[frame * 2] += sample;
                    master.samples[frame * 2 + 1] += sample;
                }
            }
        }

        // Master fader (RPP `MASTER_VOLUME` / `MASTERMUTESOLO`).
        // Balance pan law: centre = unity (no constant-power dip on
        // an already-mixed stereo bus), panning attenuates the
        // opposite channel.
        if snap.master_muted {
            master.fill(0.0);
        } else if snap.master_volume != 1.0 || snap.master_pan != 0.0 {
            let vol = snap.master_volume as f32;
            let pan = (snap.master_pan as f32).clamp(-1.0, 1.0);
            let lg = vol * (1.0 - pan.max(0.0));
            let rg = vol * (1.0 + pan.min(0.0));
            for f in 0..master.frames {
                master.samples[f * 2] *= lg;
                master.samples[f * 2 + 1] *= rg;
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        if self.realtime.get().is_some() {
            self.daw.block_profile.record(&BlockProfile {
                total_us: t_start.us(),
                snapshot_us,
                plugin_lock_us,
                fx_us,
                slowest_track: slowest.0,
                slowest_track_us: slowest.1,
                frames: frames as u32,
                items_us,
                slowest_items_track: slowest_items.0,
                slowest_items_us: slowest_items.1,
            });
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (
            snapshot_us,
            plugin_lock_us,
            fx_us,
            slowest,
            items_us,
            slowest_items,
        );
    }

    /// Transport-derived clock info at `seconds` for the aux post-render
    /// hook: `(pos_beats, tempo_bpm, time_sig_num, time_sig_den)` from
    /// the revision-cached snapshot (tempo map + project transport).
    /// Falls back to 120 bpm 4/4 when the project has no snapshot.
    ///
    /// Realtime mode reads the snapshot being played and never builds one:
    /// this runs in the audio callback after every block, and a build there
    /// (the whole project, every item cloned) is what froze playback once
    /// an edit had cut the kit into thousands of slices.
    pub(crate) fn clock_info(&self, seconds: f64) -> (f64, f64, u32, u32) {
        #[cfg(not(target_arch = "wasm32"))]
        let snap = if self.realtime.get().is_some() {
            self.scratch
                .try_lock()
                .ok()
                .and_then(|s| s.current.as_ref().map(|(_, snap)| snap.clone()))
        } else {
            self.snapshot()
        };
        #[cfg(target_arch = "wasm32")]
        let snap = self.snapshot();
        match snap {
            Some(snap) => (
                snap.tempo_map.seconds_to_beat(seconds),
                snap.tempo_map.tempo_at(seconds),
                snap.beats_per_measure,
                snap.time_sig_den,
            ),
            None => (seconds * 2.0, 120.0, 4, 4),
        }
    }

    /// Revision-cached [`RenderSnapshot`] — rebuilt only when the
    /// project's mutation revision moves. (A realtime renderer takes its
    /// snapshots from [`SnapshotBuilder`] instead.)
    fn snapshot(&self) -> Option<Arc<RenderSnapshot>> {
        // An offline render renders the song as it was when it began: its
        // first snapshot holds for all of it, whatever is edited while it
        // runs (an edit made then is the next render's). Its plugin
        // copies were made from that song too.
        if self.offline.is_some()
            && let Ok(cache) = self.cache.lock()
            && let Some((_, snap)) = cache.as_ref()
        {
            return Some(snap.clone());
        }
        let revision = self.daw.read_project(&self.project_guid, |p| p.revision)?;
        if let Ok(cache) = self.cache.lock()
            && let Some((cached_rev, snap)) = cache.as_ref()
            && *cached_rev == revision
        {
            return Some(snap.clone());
        }
        // Build + revision in ONE lock pass so the cached pair is
        // always internally consistent.
        let offline = self.offline.as_ref();
        // A render of the song's own instances honours their claims on
        // their tracks' MIDI; an offline render plays its own copies,
        // which claim nothing.
        #[cfg(not(target_arch = "wasm32"))]
        let hints = offline.is_none().then(|| LiveHints {
            played_live: Vec::new(),
            ..self.daw.live_hints()
        });
        let (revision, snap) = self.daw.read_project(&self.project_guid, |p| {
            let snap = match offline {
                Some(o) => RenderSnapshot::build_adjusted(p, &[], Some(&|t| o.adjust(t))),
                #[cfg(not(target_arch = "wasm32"))]
                None => match &hints {
                    Some(h) => RenderSnapshot::build_live(p, h),
                    None => RenderSnapshot::build(p),
                },
                #[cfg(target_arch = "wasm32")]
                None => RenderSnapshot::build(p),
            };
            (p.revision, snap)
        })?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((revision, snap.clone()));
        }
        Some(snap)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod live_input_tests {
    use super::*;

    /// Build a `LiveInput` whose ring is pre-filled with `frames`
    /// interleaved frames of `channels` channels, where channel `c`
    /// carries the constant value `(c + 1) as f32 * 0.1`.
    fn filled_live(channels: usize, frames: usize) -> LiveInput {
        let want = frames * channels;
        let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(want.max(1));
        for f in 0..frames {
            for c in 0..channels {
                let _ = prod.push((c as f32 + 1.0) * 0.1);
                let _ = f; // silence unused in some channel counts
            }
        }
        LiveInput {
            cons,
            channels,
            scratch: Vec::new(),
            underruns: 0,
            resyncs: 0,
        }
    }

    #[test]
    fn live_input_routes_channel_into_track_bus() {
        let frames = 8;
        let channels = 4;
        let mut live = filled_live(channels, frames);

        // 3 tracks: track 0 taps ch 2, track 1 taps nothing, track 2 taps ch 0.
        let input_channels = vec![Some(2u32), None, Some(0u32)];
        let mut buses = vec![
            StereoBuffer::zeroed(frames, 48_000),
            StereoBuffer::zeroed(frames, 48_000),
            StereoBuffer::zeroed(frames, 48_000),
        ];
        let mut dirty = vec![false; 3];

        mix_live_input_into_buses(
            &mut live,
            &input_channels,
            |_| true,
            &mut buses,
            &mut dirty,
            frames,
        );

        // Track 0: channel 2 → value 0.3 on both L and R.
        for f in 0..frames {
            assert!((buses[0].samples[f * 2] - 0.3).abs() < 1e-6);
            assert!((buses[0].samples[f * 2 + 1] - 0.3).abs() < 1e-6);
        }
        assert!(dirty[0]);

        // Track 1: no input tap → still silent + clean.
        assert!(buses[1].samples.iter().all(|s| *s == 0.0));
        assert!(!dirty[1]);

        // Track 2: channel 0 → value 0.1.
        for f in 0..frames {
            assert!((buses[2].samples[f * 2] - 0.1).abs() < 1e-6);
            assert!((buses[2].samples[f * 2 + 1] - 0.1).abs() < 1e-6);
        }
        assert!(dirty[2]);
        // Whole block was available — no underrun.
        assert_eq!(live.underruns, 0);
    }

    #[test]
    fn solo_skipped_track_gets_no_live_input() {
        let frames = 4;
        let channels = 2;
        let mut live = filled_live(channels, frames);
        let input_channels = vec![Some(0u32)];
        let mut buses = vec![StereoBuffer::zeroed(frames, 48_000)];
        let mut dirty = vec![false; 1];

        // `passes` returns false → track contributes nothing.
        mix_live_input_into_buses(
            &mut live,
            &input_channels,
            |_| false,
            &mut buses,
            &mut dirty,
            frames,
        );
        assert!(buses[0].samples.iter().all(|s| *s == 0.0));
        assert!(!dirty[0]);
    }

    #[test]
    fn backlog_past_one_spare_block_is_dropped() {
        // Five blocks queued (a stall's worth): the render must play the
        // second-newest block and leave one spare, not the oldest — the
        // oldest would keep the input four blocks late forever.
        let frames = 4;
        let channels = 2;
        let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(5 * frames * channels);
        for f in 0..5 * frames {
            for _ in 0..channels {
                let _ = prod.push(f as f32);
            }
        }
        let mut live = LiveInput {
            cons,
            channels,
            scratch: Vec::new(),
            underruns: 0,
            resyncs: 0,
        };
        let mut buses = vec![StereoBuffer::zeroed(frames, 48_000)];
        let mut dirty = vec![false; 1];

        mix_live_input_into_buses(
            &mut live,
            &[Some(0)],
            |_| true,
            &mut buses,
            &mut dirty,
            frames,
        );

        for f in 0..frames {
            assert_eq!(buses[0].samples[f * 2], (3 * frames + f) as f32);
        }
        assert_eq!(live.cons.slots(), frames * channels);
        assert_eq!(live.resyncs, 1);
        assert_eq!(live.underruns, 0);
    }

    #[test]
    fn short_ring_zero_fills_and_counts_underrun() {
        let frames = 8;
        let channels = 2;
        // Only fill HALF the requested frames.
        let mut live = filled_live(channels, frames / 2);
        let input_channels = vec![Some(1u32)];
        let mut buses = vec![StereoBuffer::zeroed(frames, 48_000)];
        let mut dirty = vec![false; 1];

        mix_live_input_into_buses(
            &mut live,
            &input_channels,
            |_| true,
            &mut buses,
            &mut dirty,
            frames,
        );

        // First half carries channel-1 signal (0.2); second half zero-filled.
        for f in 0..frames / 2 {
            assert!((buses[0].samples[f * 2] - 0.2).abs() < 1e-6);
        }
        for f in frames / 2..frames {
            assert_eq!(buses[0].samples[f * 2], 0.0);
        }
        assert_eq!(live.underruns, 1);
        assert!(dirty[0]);
    }

    #[test]
    fn out_of_range_channel_is_ignored() {
        let frames = 4;
        let channels = 2;
        let mut live = filled_live(channels, frames);
        // Track taps channel 5 which doesn't exist on a 2-channel input.
        let input_channels = vec![Some(5u32)];
        let mut buses = vec![StereoBuffer::zeroed(frames, 48_000)];
        let mut dirty = vec![false; 1];

        mix_live_input_into_buses(
            &mut live,
            &input_channels,
            |_| true,
            &mut buses,
            &mut dirty,
            frames,
        );
        assert!(buses[0].samples.iter().all(|s| *s == 0.0));
        assert!(!dirty[0]);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod live_midi_tests {
    use super::*;
    use daw_proto::{Channel, ControllerNumber, ControllerValue, KeyNumber, MidiEvent, Velocity};

    // Terse constructors for the canonical `MidiEvent` (channel 0).
    fn note_on(key: u8, vel: u8) -> MidiEvent {
        MidiEvent::NoteOn {
            channel: Channel::new(0),
            key: KeyNumber::new(key),
            velocity: Velocity::new(vel),
        }
    }
    fn note_off(key: u8, vel: u8) -> MidiEvent {
        MidiEvent::NoteOff {
            channel: Channel::new(0),
            key: KeyNumber::new(key),
            velocity: Velocity::new(vel),
        }
    }
    fn control_change(controller: u8, value: u8) -> MidiEvent {
        MidiEvent::ControlChange {
            channel: Channel::new(0),
            controller: ControllerNumber::new(controller),
            value: ControllerValue::new(value),
        }
    }

    /// Build a `LiveMidiQueue` pre-filled with the given events.
    fn queue_with(events: Vec<LiveMidiEvent>) -> LiveMidiQueue {
        let (mut prod, cons) = rtrb::RingBuffer::<LiveMidiEvent>::new(events.len().max(1));
        for ev in events {
            let _ = prod.push(ev);
        }
        LiveMidiQueue { cons }
    }

    #[test]
    fn drain_routes_events_to_the_matching_track_bucket() {
        let guids = ["track-a", "track-b", "track-c"];
        let idx_of = |g: &str| guids.iter().position(|x| *x == g);

        let mut queue = queue_with(vec![
            LiveMidiEvent {
                track: "track-b".into(),
                offset: 0,
                message: note_on(60, 100),
            },
            LiveMidiEvent {
                track: "track-a".into(),
                offset: 0,
                message: control_change(7, 64),
            },
            // Unknown guid → dropped, not merged.
            LiveMidiEvent {
                track: "ghost".into(),
                offset: 0,
                message: note_off(60, 0),
            },
        ]);

        let mut buckets: Vec<Vec<crate::plugin::PluginMidiEvent>> = vec![Vec::new(); 3];
        drain_live_midi(&mut queue, idx_of, &mut buckets);

        // track-a (index 0) got the CC; track-b (index 1) got the NoteOn.
        assert_eq!(buckets[0].len(), 1);
        assert!(matches!(
            buckets[0][0].message,
            MidiEvent::ControlChange { controller, value, .. }
                if controller.get() == 7 && value.get() == 64
        ));
        assert_eq!(buckets[1].len(), 1);
        assert!(matches!(
            buckets[1][0].message,
            MidiEvent::NoteOn { key, velocity, .. }
                if key.get() == 60 && velocity.get() == 100
        ));
        // track-c untouched; the unknown-guid event was dropped.
        assert!(buckets[2].is_empty());
    }

    #[test]
    fn drained_events_merge_and_resort_with_collected_events() {
        // Mirror the render_block merge: an existing offset-ordered list
        // gets the live bucket appended, then re-sorted by offset.
        let mut midi_events: Vec<crate::plugin::PluginMidiEvent> = vec![
            crate::plugin::PluginMidiEvent {
                offset: 0,
                message: note_on(48, 90),
            },
            crate::plugin::PluginMidiEvent {
                offset: 256,
                message: note_off(48, 0),
            },
        ];

        let mut bucket = vec![
            crate::plugin::PluginMidiEvent {
                offset: 128,
                message: control_change(1, 100),
            },
            crate::plugin::PluginMidiEvent {
                offset: 0,
                message: note_on(60, 100),
            },
        ];

        midi_events.append(&mut bucket);
        midi_events.sort_by_key(|e| e.offset);

        let offsets: Vec<u32> = midi_events.iter().map(|e| e.offset).collect();
        assert_eq!(offsets, vec![0, 0, 128, 256]);
        assert!(bucket.is_empty());
    }
}

/// A stereo signal's gains at `pan` (−1 left … 1 right): REAPER's default,
/// stereo balance with a 0 dB pan law (`PANMODE 3`, `PANLAW 1`). Centred,
/// both sides pass at unity; panned, the far side is turned down and the
/// near side is left as it is.
///
/// It was a constant-power law, √0.5 (−3 dB) on each side at centre, on
/// every track, folder, bus and send: a centred kick mic summed through
/// its folders and buses lost 3 dB a stage — unbreakable's kit played
/// 24 dB under its gain-staged level.
#[must_use]
pub(crate) fn balance(pan: f64) -> (f64, f64) {
    let pan = pan.clamp(-1.0, 1.0);
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod pdc_tests;

#[cfg(all(test, feature = "clap-host", not(target_arch = "wasm32")))]
mod automation_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod parallel_tests;

#[cfg(test)]
mod poison_recovery_tests {
    use super::*;

    /// A control-thread panic while holding `plugin_instances` (or a
    /// renderer-internal mutex) must not cascade: the next audio block's
    /// lock recovers the poisoned mutex instead of panicking inside the
    /// extern "C" audio callback (which would abort the process).
    #[test]
    fn render_survives_poisoned_plugin_instances() {
        let daw = Standalone::new();
        let guid = daw.seed_project(daw_proto::ProjectInfo {
            guid: "p".into(),
            name: "p".into(),
            path: String::new(),
        });

        // Poison the shared map: panic on another thread while holding it.
        let map = daw.plugin_instances.clone();
        let _ = std::thread::spawn(move || {
            let _guard = map.write();
            panic!("poison plugin_instances");
        })
        .join();
        assert!(
            daw.plugin_instances.is_poisoned(),
            "lock should be poisoned"
        );

        // Control seams recover in place of panicking…
        assert!(!daw.has_plugin_instance("nope"));
        assert!(daw.with_plugin_instance("nope", |_| ()).is_none());

        // …and the render path (which takes the same lock every block)
        // keeps producing blocks.
        let renderer = ProjectRenderer::new(&daw, &guid, 48_000);
        let out = renderer.render_block(0, 128);
        assert_eq!(out.frames, 128);
    }

    /// The renderer's own scratch mutex also recovers from poison.
    #[test]
    fn render_survives_poisoned_scratch() {
        let daw = Standalone::new();
        let guid = daw.seed_project(daw_proto::ProjectInfo {
            guid: "p".into(),
            name: "p".into(),
            path: String::new(),
        });
        let renderer = std::sync::Arc::new(ProjectRenderer::new(&daw, &guid, 48_000));

        let r = renderer.clone();
        let _ = std::thread::spawn(move || {
            let _guard = r.scratch.lock().unwrap();
            panic!("poison render scratch");
        })
        .join();
        assert!(renderer.scratch.lock().is_err(), "mutex should be poisoned");

        let out = renderer.render_block(0, 256);
        assert_eq!(out.frames, 256);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod folder_routing_tests {
    use super::*;
    use crate::audio_engine::decoder::DecodedAudio;
    use crate::audio_engine::materialize::attach_audio_source;
    use crate::media_seed::{StemSpec, seed_media_tracks};
    use daw_proto::{ProjectContext, ProjectInfo, TrackRef, Tracks};

    fn rms(s: &[f32]) -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
    }

    /// Seed the REAL media path — Vocals{Lead,BGV}, Drums{Kick,Snare}, ungrouped
    /// Loop — then inject synthetic constant audio into every take so the items
    /// sound headlessly (no real WAV files needed).
    fn seeded() -> (Standalone, String) {
        let daw = Standalone::new();
        let guid = daw.seed_project(ProjectInfo {
            guid: "route-test".into(),
            name: "RT".into(),
            path: String::new(),
        });
        let stems = vec![
            StemSpec::new("Lead", "/x/lead.wav", Some("Vocals")),
            StemSpec::new("BGV", "/x/bgv.wav", Some("Vocals")),
            StemSpec::new("Kick", "/x/kick.wav", Some("Drums")),
            StemSpec::new("Snare", "/x/snare.wav", Some("Drums")),
            StemSpec::new("Loop", "/x/loop.wav", None),
        ];
        seed_media_tracks(&daw, &guid, &stems, 0.0, 10.0);
        let takes: Vec<String> = daw
            .read_project(&guid, |p| {
                p.takes
                    .values()
                    .flat_map(|tl| tl.takes.iter().map(|t| t.guid.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for tg in takes {
            attach_audio_source(
                &daw,
                &guid,
                &tg,
                DecodedAudio::new(vec![0.5; 4096], 1, 48_000),
            );
        }
        (daw, guid)
    }

    pub(crate) fn guid_of(daw: &Standalone, guid: &str, name: &str) -> String {
        daw.read_project(guid, |p| {
            p.tracks
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.guid.clone())
        })
        .flatten()
        .unwrap_or_else(|| panic!("no track named {name}"))
    }

    // (a) The seed populates folder parents.
    #[test]
    fn seed_populates_folder_parents() {
        let (daw, guid) = seeded();
        let tracks = daw.read_project(&guid, |p| p.tracks.clone()).unwrap();
        let by = |n: &str| tracks.iter().find(|t| t.name == n).expect(n);
        let vocals = by("Vocals");
        let drums = by("Drums");
        assert!(
            vocals.is_folder && vocals.parent_guid.is_none(),
            "Vocals folder"
        );
        assert!(
            drums.is_folder && drums.parent_guid.is_none(),
            "Drums folder"
        );
        assert!(!by("Lead").is_folder);
        assert_eq!(
            by("Lead").parent_guid.as_deref(),
            Some(vocals.guid.as_str())
        );
        assert_eq!(by("BGV").parent_guid.as_deref(), Some(vocals.guid.as_str()));
        assert_eq!(by("Kick").parent_guid.as_deref(), Some(drums.guid.as_str()));
        assert_eq!(
            by("Snare").parent_guid.as_deref(),
            Some(drums.guid.as_str())
        );
        assert_eq!(by("Loop").parent_guid, None);
    }

    // (b) The render snapshot resolves parent_idx for children.
    #[test]
    fn snapshot_resolves_parent_idx() {
        let (daw, guid) = seeded();
        let snap = daw.read_project(&guid, RenderSnapshot::build).unwrap();
        let sidx = |n: &str| snap.tracks.iter().position(|t| t.name == n).expect(n);
        let vi = sidx("Vocals");
        let di = sidx("Drums");
        assert_eq!(snap.tracks[sidx("Lead")].parent_idx, Some(vi));
        assert_eq!(snap.tracks[sidx("BGV")].parent_idx, Some(vi));
        assert_eq!(snap.tracks[sidx("Kick")].parent_idx, Some(di));
        assert_eq!(snap.tracks[sidx("Snare")].parent_idx, Some(di));
        assert_eq!(snap.tracks[sidx("Loop")].parent_idx, None);
    }

    // (c) Muting a folder must silence its children (folder-bus cascade).
    #[test]
    fn muting_folder_silences_children() {
        let (daw, guid) = seeded();
        let ctx = ProjectContext::Project(guid.clone());
        let vocals = guid_of(&daw, &guid, "Vocals");
        let lead = guid_of(&daw, &guid, "Lead");
        let bgv = guid_of(&daw, &guid, "BGV");
        let renderer = ProjectRenderer::new(&daw, &guid, 48_000);

        let base = renderer.render_block(0, 512);
        assert!(rms(&base.samples) > 0.01, "baseline should have audio");

        // Reference: mute the two vocal LEAVES individually.
        Tracks::set_muted(&daw, ctx.clone(), TrackRef::Guid(lead.clone()), true).unwrap();
        Tracks::set_muted(&daw, ctx.clone(), TrackRef::Guid(bgv.clone()), true).unwrap();
        let leaves = renderer.render_block(0, 512);
        Tracks::set_muted(&daw, ctx.clone(), TrackRef::Guid(lead), false).unwrap();
        Tracks::set_muted(&daw, ctx.clone(), TrackRef::Guid(bgv), false).unwrap();

        // Mute the FOLDER — must cascade to the same output.
        Tracks::set_muted(&daw, ctx.clone(), TrackRef::Guid(vocals), true).unwrap();
        let folder = renderer.render_block(0, 512);

        assert!(
            rms(&folder.samples) < rms(&base.samples) - 1e-4,
            "muting the Vocals folder must reduce master (base={:.4}, folder={:.4})",
            rms(&base.samples),
            rms(&folder.samples)
        );
        for (a, b) in folder.samples.iter().zip(leaves.samples.iter()) {
            assert!(
                (a - b).abs() < 1e-4,
                "folder mute must cascade exactly like muting its children"
            );
        }
    }

    // An offline render of only what feeds a point, read at each point
    // along a track: what is left out is silent and runs nothing.
    #[test]
    fn an_offline_render_of_only_some_tracks_probes_them_along_the_way() {
        use offline::{Offline, Probe, ProbeAt};
        let (daw, guid) = seeded();
        let ctx = ProjectContext::Project(guid.clone());
        let drums = guid_of(&daw, &guid, "Drums");
        let kick = guid_of(&daw, &guid, "Kick");
        let lead = guid_of(&daw, &guid, "Lead");
        Tracks::set_volume(&daw, ctx, TrackRef::Guid(kick.clone()), 0.5).unwrap();
        let at = |track: &str, at| ProbeAt {
            track: track.to_owned(),
            at,
        };
        let renderer = ProjectRenderer::offline(
            &daw,
            &guid,
            48_000,
            Offline {
                only: Some([drums.clone(), kick.clone()].into_iter().collect()),
                probes: vec![
                    at(&kick, Probe::Input),
                    at(&kick, Probe::PreFader),
                    at(&kick, Probe::PostFader),
                    at(&drums, Probe::PreFx),
                    at(&lead, Probe::PreFx),
                ],
                ..Offline::default()
            },
        );
        let master = renderer.render_block(0, 512);
        let read = renderer.with_probes(|p| p.iter().map(|s| rms(s)).collect::<Vec<_>>());
        let (input, pre, post, folder, other) = (read[0], read[1], read[2], read[3], read[4]);
        assert!(input > 0.01, "the kick's media: {read:?}");
        assert!(
            (pre - input).abs() < 1e-6,
            "no FX: pre-fader is its input: {read:?}"
        );
        let (l, _) = balance(0.0);
        assert!(
            (post - input * 0.5 * l).abs() < 1e-4,
            "post-fader is its input through the fader: {read:?}"
        );
        assert!(
            (folder - post).abs() < 1e-4,
            "the folder's input is the kick alone, the snare left out: {read:?}"
        );
        assert!(other < 1e-9, "a track not rendered is silent: {read:?}");
        assert!(rms(&master.samples) > 0.0);
    }

    // An offline render is of the song as it was when it began: an edit
    // made while it renders (a fader, a mute) is not heard in it, block
    // for block the same as a render nobody touched — while the song's
    // own renderer hears the edit at once.
    #[test]
    fn an_offline_render_keeps_the_song_it_began_with() {
        use offline::Offline;
        let (daw, guid) = seeded();
        let ctx = ProjectContext::Project(guid.clone());
        let kick = guid_of(&daw, &guid, "Kick");
        let lead = guid_of(&daw, &guid, "Lead");
        let blocks = |edit: bool| {
            let renderer = ProjectRenderer::offline(&daw, &guid, 48_000, Offline::default());
            let mut out = Vec::new();
            for b in 0..4u64 {
                if edit && b == 2 {
                    Tracks::set_volume(&daw, ctx.clone(), TrackRef::Guid(kick.clone()), 0.1)
                        .unwrap();
                    Tracks::set_muted(&daw, ctx.clone(), TrackRef::Guid(lead.clone()), true)
                        .unwrap();
                }
                out.extend(renderer.render_block(b * 512, 512).samples);
            }
            out
        };
        let untouched = blocks(false);
        let edited_during = blocks(true);
        assert!(rms(&untouched) > 0.01);
        assert_eq!(edited_during, untouched, "the edit was heard mid-render");
        // The edit stands: the song's own renderer, and the next offline
        // render, hear it.
        let live = ProjectRenderer::new(&daw, &guid, 48_000).render_block(0, 512);
        let next =
            ProjectRenderer::offline(&daw, &guid, 48_000, Offline::default()).render_block(0, 512);
        assert_eq!(live.samples, next.samples);
        assert_ne!(next.samples[..], untouched[..1_024]);
    }

    // A live tap: while the song plays (not an offline render), a point of
    // a track is copied into its ring, block by block — what an offline
    // probe of the same point reads — and a ring too small drops blocks
    // and says how many.
    #[test]
    fn a_live_tap_hands_over_what_plays_through_its_point() {
        use offline::{Offline, Probe, ProbeAt};
        let (daw, guid) = seeded();
        let kick = guid_of(&daw, &guid, "Kick");
        let at = ProbeAt {
            track: kick.clone(),
            at: Probe::PostFader,
        };
        let mut tap = daw.live_taps().open(&guid, at.clone(), 48_000).unwrap();
        let mut small = daw.live_taps().open(&guid, at.clone(), 600).unwrap();
        let playing = ProjectRenderer::new(&daw, &guid, 48_000);
        for b in 0..4u64 {
            playing.render_block(b * 512, 512);
        }
        let mut heard = Vec::new();
        tap.drain(&mut heard);
        let offline = ProjectRenderer::offline(
            &daw,
            &guid,
            48_000,
            Offline {
                probes: vec![at],
                ..Offline::default()
            },
        );
        let mut want = Vec::new();
        for b in 0..4u64 {
            offline.render_block(b * 512, 512);
            offline.with_probes(|p| want.extend_from_slice(&p[0]));
        }
        assert!(rms(&heard) > 0.01);
        assert_eq!(heard, want);
        assert_eq!(tap.dropped(), 0);
        // 600 frames hold one 512-frame block: the other three are dropped.
        let mut some = Vec::new();
        small.drain(&mut some);
        assert_eq!(some.len(), 1_024);
        assert_eq!(small.dropped(), 3);
        // Closed, nothing more arrives.
        daw.live_taps().close(tap.slot);
        playing.render_block(4 * 512, 512);
        let mut after = Vec::new();
        tap.drain(&mut after);
        assert!(after.is_empty());
    }

    // A track's output recorded by one render and fed into the next in
    // its place: what it feeds hears exactly what it heard.
    #[test]
    fn a_fed_track_sounds_as_it_did_when_it_was_rendered() {
        use offline::{Fed, Offline, Probe, ProbeAt};
        let (daw, guid) = seeded();
        let drums = guid_of(&daw, &guid, "Drums");
        let kick = guid_of(&daw, &guid, "Kick");
        let at = |track: &str, at| ProbeAt {
            track: track.to_owned(),
            at,
        };
        let probes = vec![at(&kick, Probe::PostFader), at(&drums, Probe::PreFx)];
        let render = |fed| {
            let renderer = ProjectRenderer::offline(
                &daw,
                &guid,
                48_000,
                Offline {
                    probes: probes.clone(),
                    fed,
                    ..Offline::default()
                },
            );
            let mut out = vec![Vec::new(); probes.len()];
            for b in 0..4u64 {
                renderer.render_block(b * 512, 512);
                renderer.with_probes(|p| {
                    for (o, s) in out.iter_mut().zip(p) {
                        o.extend_from_slice(s);
                    }
                });
            }
            out
        };
        let first = render(std::collections::HashMap::new());
        let fed = [(
            kick.clone(),
            Fed {
                start: 0,
                samples: first[0].clone().into(),
                latency: 0,
            },
        )]
        .into_iter()
        .collect();
        let again = render(fed);
        assert!(rms(&first[1]) > 0.01);
        assert_eq!(first[1], again[1], "the folder hears the kick as it was");
        assert_eq!(first[0], again[0]);
    }

    // (d) Soloing a leaf must isolate it.
    #[test]
    fn soloing_leaf_isolates_it() {
        let (daw, guid) = seeded();
        let ctx = ProjectContext::Project(guid.clone());
        let kick = guid_of(&daw, &guid, "Kick");
        let others = ["Lead", "BGV", "Snare", "Loop"];
        let renderer = ProjectRenderer::new(&daw, &guid, 48_000);

        // Reference: mute everything except Kick.
        for n in others {
            Tracks::set_muted(
                &daw,
                ctx.clone(),
                TrackRef::Guid(guid_of(&daw, &guid, n)),
                true,
            )
            .unwrap();
        }
        let only_kick = renderer.render_block(0, 512);
        for n in others {
            Tracks::set_muted(
                &daw,
                ctx.clone(),
                TrackRef::Guid(guid_of(&daw, &guid, n)),
                false,
            )
            .unwrap();
        }

        // Solo Kick — must isolate to the same output.
        Tracks::set_soloed(&daw, ctx.clone(), TrackRef::Guid(kick), true).unwrap();
        let solo = renderer.render_block(0, 512);

        assert!(rms(&solo.samples) > 0.01, "soloed kick should be audible");
        for (a, b) in solo.samples.iter().zip(only_kick.samples.iter()) {
            assert!(
                (a - b).abs() < 1e-4,
                "solo must isolate exactly like muting all other tracks"
            );
        }
    }

    // (d′) Soloing a FOLDER must keep its children audible (REAPER: solo a folder
    // → its whole sub-mix plays, everything else muted). Stage 1 must gate item
    // playback on the solo-routing MASK (soloed track + ancestors + descendants),
    // NOT the raw per-track `soloed` flag — a folder's children aren't soloed
    // themselves, so raw-flag gating silences them. This reproduces the live
    // "solo doesn't behave" report for folder solo.
    #[test]
    fn soloing_folder_keeps_children_audible() {
        let (daw, guid) = seeded();
        let ctx = ProjectContext::Project(guid.clone());
        let vocals = guid_of(&daw, &guid, "Vocals");
        let others = ["Kick", "Snare", "Loop"];
        let renderer = ProjectRenderer::new(&daw, &guid, 48_000);

        // Reference: mute everything except the vocal children (what soloing the
        // Vocals folder should sound like).
        for n in others {
            Tracks::set_muted(
                &daw,
                ctx.clone(),
                TrackRef::Guid(guid_of(&daw, &guid, n)),
                true,
            )
            .unwrap();
        }
        let only_vocals = renderer.render_block(0, 512);
        for n in others {
            Tracks::set_muted(
                &daw,
                ctx.clone(),
                TrackRef::Guid(guid_of(&daw, &guid, n)),
                false,
            )
            .unwrap();
        }
        assert!(
            rms(&only_vocals.samples) > 0.01,
            "reference vocal sub-mix must be audible"
        );

        // Solo the Vocals FOLDER — its children must stay audible.
        Tracks::set_soloed(&daw, ctx.clone(), TrackRef::Guid(vocals), true).unwrap();
        let solo_folder = renderer.render_block(0, 512);

        assert!(
            rms(&solo_folder.samples) > 0.01,
            "soloing the Vocals folder silenced its children (rms={:.5}) — stage 1 gated on raw \
             `soloed` instead of the solo-routing mask",
            rms(&solo_folder.samples)
        );
        for (a, b) in solo_folder.samples.iter().zip(only_vocals.samples.iter()) {
            assert!(
                (a - b).abs() < 1e-4,
                "folder solo must isolate to exactly the folder's children"
            );
        }
    }

    /// A test plugin with memory: a one-pole lowpass of its input, plus
    /// half its sidechain — so a block rendered with the wrong state, the
    /// wrong input or the wrong key shows.
    struct Smear {
        state: [f32; 2],
        key: Vec<f32>,
    }

    impl crate::plugin::PluginInstance for Smear {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor::default()
        }
        fn params(&mut self) -> Vec<crate::plugin::PluginParamInfo> {
            Vec::new()
        }
        fn param_value(&mut self, _: u32) -> Option<f64> {
            None
        }
        fn value_to_text(&mut self, _: u32, _: f64) -> Option<String> {
            None
        }
        fn text_to_value(&mut self, _: u32, _: &str) -> Option<f64> {
            None
        }
        fn latency(&mut self) -> u32 {
            0
        }
        fn prepare(&mut self, _: f64, _: u32) -> Result<(), crate::plugin::PluginError> {
            Ok(())
        }
        fn is_prepared(&self) -> bool {
            true
        }
        fn parallel_safe(&self) -> bool {
            true
        }
        fn set_sidechain(&mut self, left: &[f32], _right: &[f32]) {
            self.key.clear();
            self.key.extend_from_slice(left);
        }
        fn process_block(
            &mut self,
            in_l: &[f32],
            in_r: &[f32],
            out_l: &mut [f32],
            out_r: &mut [f32],
            _: &crate::plugin::PluginEvents<'_>,
        ) -> Result<(), crate::plugin::PluginError> {
            for f in 0..in_l.len() {
                self.state[0] += 0.05 * (in_l[f] - self.state[0]);
                self.state[1] += 0.05 * (in_r[f] - self.state[1]);
                let key = self.key.get(f).copied().unwrap_or(0.0) * 0.5;
                out_l[f] = self.state[0] + key;
                out_r[f] = self.state[1] - key;
            }
            self.key.clear();
            Ok(())
        }
        fn deactivate(&mut self) {}
    }

    struct Smears;

    impl crate::plugin::FxFactory for Smears {
        fn installed(&self) -> Vec<daw_proto::fx::InstalledFx> {
            Vec::new()
        }
        fn provides(&self, name: &str) -> bool {
            name == "Smear"
        }
        fn create(&self, name: &str, _: f64) -> Option<Box<dyn crate::plugin::PluginInstance>> {
            self.provides(name).then(|| {
                Box::new(Smear {
                    state: [0.0; 2],
                    key: Vec::new(),
                }) as Box<dyn crate::plugin::PluginInstance>
            })
        }
    }

    /// The seeded song, made to need both halves: Lead armed and
    /// monitoring (live, and so its Vocals folder), Loop sending into
    /// Vocals and Kick keying Lead's sidechain (ahead tracks feeding live
    /// ones), a stateful plugin on Lead, Kick and the Drums folder, BGV
    /// muted and master turned down. `solo` solos that track too.
    pub(crate) fn split_song(solo: Option<&str>) -> (Standalone, String) {
        use daw_proto::fx::{Effects, FxChainContext};
        use daw_proto::track::{InputMonitoringMode, RecordInput};
        use daw_proto::{RouteLocation, RouteRef, RouteType, Routing};
        let (daw, guid) = seeded();
        daw.set_fx_factory(Arc::new(Smears));
        let ctx = ProjectContext::Project(guid.clone());
        let track = |n: &str| TrackRef::Guid(guid_of(&daw, &guid, n));
        Tracks::set_record_input(
            &daw,
            ctx.clone(),
            track("Lead"),
            RecordInput::Audio { channel: 0 },
        )
        .unwrap();
        Tracks::set_input_monitor(
            &daw,
            ctx.clone(),
            track("Lead"),
            InputMonitoringMode::Normal,
        )
        .unwrap();
        Tracks::set_armed(&daw, ctx.clone(), track("Lead"), true).unwrap();
        for n in ["Lead", "Kick", "Drums"] {
            Effects::add(
                &daw,
                ctx.clone(),
                FxChainContext::Track(guid_of(&daw, &guid, n)),
                "Smear",
            )
            .expect("the fx");
        }
        Routing::add_send(&daw, ctx.clone(), track("Loop"), track("Vocals")).expect("a send");
        let key =
            Routing::add_send(&daw, ctx.clone(), track("Kick"), track("Lead")).expect("a send");
        Routing::set_dest_channels(
            &daw,
            ctx.clone(),
            RouteLocation {
                track: track("Kick"),
                route_type: RouteType::Send,
                route: RouteRef::Index(key),
            },
            2,
            2,
        )
        .expect("the key");
        Tracks::set_muted(&daw, ctx.clone(), track("BGV"), true).unwrap();
        daw.write_project(&guid, |p| p.master_volume = 0.8);
        if let Some(n) = solo {
            Tracks::set_soloed(&daw, ctx, track(n), true).unwrap();
        }
        (daw, guid)
    }

    /// `blocks` blocks of `frames` from a renderer of the whole song, and
    /// from an ahead render handing a live one its feed — the same song,
    /// fresh plugins each.
    fn whole_and_split(solo: Option<&str>, blocks: usize, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = split_song(solo);
        let whole = ProjectRenderer::new(&daw, &guid, 48_000);
        let mut a = Vec::new();
        for b in 0..blocks {
            a.extend(whole.render_block((b * frames) as u64, frames).samples);
        }
        let (daw, guid) = split_song(solo);
        let ahead = ProjectRenderer::new(&daw, &guid, 48_000);
        let live = ProjectRenderer::new(&daw, &guid, 48_000);
        let mut feed = Feed::default();
        let mut split = Vec::new();
        for b in 0..blocks {
            let at = (b * frames) as f64;
            let _ = ahead.render_part(at, frames, 1.0, Part::Ahead(&mut feed));
            split.extend(live.render_part(at, frames, 1.0, Part::Live(&feed)).samples);
        }
        (a, split)
    }

    fn assert_same(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        assert!(rms(a) > 0.01, "the song is heard");
        let worst = a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1e-6,
            "ahead + live is the whole song: worst {worst}"
        );
    }

    /// The tracks a live track feeds are live with it; the rest are not.
    #[test]
    fn a_monitored_track_and_what_it_feeds_are_live() {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = split_song(None);
        let snap = daw.read_project(&guid, RenderSnapshot::build).unwrap();
        let live = |n: &str| snap.live[snap.tracks.iter().position(|t| t.name == n).unwrap()];
        assert_eq!(daw.plugin_instances.read().len(), 3, "the plugins are in");
        assert!(live("Lead") && live("Vocals"));
        for n in ["BGV", "Kick", "Snare", "Drums", "Loop"] {
            assert!(!live(n), "{n} renders ahead");
        }
    }

    /// A track set to render live (REAPER's "no anticipative FX"), one
    /// with a plugin's window open and one just played over live MIDI are
    /// live — with what they feed — and go back to rendering ahead after.
    #[test]
    fn a_track_is_live_when_it_is_worked_on() {
        use crate::sync::FxChainKey;
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = split_song(None);
        let ctx = ProjectContext::Project(guid.clone());
        let kick = guid_of(&daw, &guid, "Kick");
        let live = |name: &str| {
            let played = daw.played_live();
            let snap = daw
                .read_project(&guid, |p| RenderSnapshot::build_hinted(p, &played))
                .unwrap();
            snap.live[snap.tracks.iter().position(|t| t.name == name).unwrap()]
        };
        assert!(!live("Kick") && !live("Drums"));

        Tracks::set_render_live(&daw, ctx.clone(), TrackRef::Guid(kick.clone()), true).unwrap();
        assert!(Tracks::render_live(
            &daw,
            ctx.clone(),
            TrackRef::Guid(kick.clone())
        ));
        assert!(live("Kick") && live("Drums"), "it, and the folder it feeds");
        Tracks::set_render_live(&daw, ctx, TrackRef::Guid(kick.clone()), false).unwrap();
        assert!(!live("Kick"));

        let window = |open: bool| {
            daw.write_project(&guid, |p| {
                let chain = p.fx_chains.get_mut(&FxChainKey::Track(kick.clone()));
                chain.expect("Kick's plugin")[0].fx.window_open = open;
            });
        };
        window(true);
        assert!(live("Kick"), "a plugin's window open");
        window(false);
        assert!(!live("Kick"));

        #[cfg(feature = "audio")]
        {
            daw.note_played_live(&kick);
            assert!(live("Kick"), "played over live MIDI");
        }
    }

    /// The ahead half and the live half, the live one started from the
    /// other's feed, play the whole song sample for sample — folders,
    /// sends into a live bus, a key into a live sidechain, plugins with
    /// memory, a mute and master's fader — block after block.
    #[test]
    fn ahead_and_live_together_are_the_whole_song() {
        let (whole, split) = whole_and_split(None, 6, 256);
        assert_same(&whole, &split);
        // And the feed carries something: the live half alone is not it.
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = split_song(None);
        let live = ProjectRenderer::new(&daw, &guid, 48_000);
        let alone = live.render_part(0.0, 256, 1.0, Part::Live(&Feed::default()));
        let differs = alone
            .samples
            .iter()
            .zip(&whole)
            .any(|(x, y)| (x - y).abs() > 1e-3);
        assert!(differs, "the tracks ahead are heard only through the feed");
    }

    /// The same under a solo, which routes some tracks only into what they
    /// feed.
    #[test]
    fn ahead_and_live_together_are_the_whole_song_under_a_solo() {
        for solo in ["Vocals", "Kick", "Lead"] {
            let (whole, split) = whole_and_split(Some(solo), 4, 192);
            assert_same(&whole, &split);
        }
    }

    /// A feed of another live set is not taken: the live render plays
    /// without the tracks ahead rather than on the wrong buses.
    #[test]
    fn a_feed_for_another_live_set_is_left_alone() {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = split_song(None);
        let live = ProjectRenderer::new(&daw, &guid, 48_000);
        let mut feed = Feed::default();
        feed.reset(128, 1, 0);
        feed.master.fill(1.0);
        let out = live.render_part(0.0, 128, 1.0, Part::Live(&feed));
        assert!(out.samples.iter().all(|s| s.abs() < 0.99));
    }
}
