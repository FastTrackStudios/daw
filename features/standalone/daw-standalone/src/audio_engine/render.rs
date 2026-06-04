//! Backend-agnostic project renderer.
//!
//! `ProjectRenderer` takes a [`Standalone`] backend + project GUID
//! and produces a stereo master buffer for a given sample range.
//! It walks the routing graph each block (cheap because we snapshot
//! the state once up-front per render call) and honors:
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
//! - Pre-FX vs Post-FX send positions (always treated as post-fader)
//! - Channel mappings beyond stereo (channel count is honored on the
//!   bus but down-mixed to stereo at the master)
//! - Folder bus summing (folders treated as containers; their child
//!   tracks already send to master if parent_send is on)
//!
//! WASM-compatible: pure math + heap, no threads, no cpal. The cpal
//! mixer in `mixer.rs` wraps this on native; an AudioWorklet shim can
//! wrap it on the web.

use std::sync::Arc;

use daw_proto::RouteType;
use daw_proto::automation::{
    EnvelopePoint, EnvelopeShape, EnvelopeType, SendEnvelopeKind, TakeEnvelopeKind,
};
use daw_proto::primitives::AutomationMode;

use crate::sync::EnvelopeData;

use super::source::AudioSource;
use crate::sync::{EnvelopeKey, ProjectState, Standalone};

/// Honor `EnvelopeData.automation_mode` at snapshot time: return the
/// points only when the mode is something other than `Off`.
/// `Touch` / `Latch` / `Write` apply at playback like `Read` —
/// recording is a separate concern (see touch state on `Standalone`).
fn active_envelope(data: Option<&EnvelopeData>) -> Option<Vec<EnvelopePoint>> {
    let d = data?;
    match d.automation_mode {
        AutomationMode::Off => None,
        _ => Some(d.points.clone()),
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

/// Snapshot of routing-relevant track data, copied once per render
/// call so we don't hold the project lock through the inner loops.
struct TrackSnapshot {
    guid: String,
    name: String,
    volume: f64,
    pan: f64,
    /// Polarity invert: flip the signal's sign at the gain stage.
    phase_inverted: bool,
    muted: bool,
    soloed: bool,
    parent_send: bool,
    /// Fixed-lane play mask (bit n = lane n audible). `0` when the
    /// track has no fixed lanes — every item plays.
    lane_play_mask: u64,
    /// Track has hardware-output routes. v0 monitoring model: hw outs
    /// sum to master like a parent send (we only render one stereo
    /// device), so a `MAINSEND 0` track feeding the interface
    /// directly is still audible.
    hw_out: bool,
    /// Folder parent (resolved guid) — children sum into the parent's bus.
    parent_guid: Option<String>,
    sends: Vec<SendSnapshot>,
    items: Vec<ItemSnapshot>,
    /// FX chain — list of `fx_guid`s in chain order. The renderer
    /// resolves each one in `Standalone.plugin_instances` and pipes
    /// the track bus through them in order. `None`/missing entries
    /// (e.g. synthetic-only FX) are skipped.
    fx_chain: Vec<String>,
    /// Per-FX enabled flag from `Fx.enabled`. Matched 1:1 with
    /// `fx_chain` by index.
    fx_enabled: Vec<bool>,
    /// Stored normalized parameter values per FX. Matched 1:1 with
    /// `fx_chain` by index and sent to hosted plugins every block so
    /// service-side parameter edits affect DSP.
    fx_params: Vec<Vec<(u32, f64)>>,
    /// Volume envelope points, sorted by time. Multiplies the
    /// static `volume` field when present.
    volume_env: Option<Vec<EnvelopePoint>>,
    /// Pan envelope points. `0..=1` value range, where 0.5 = center.
    pan_env: Option<Vec<EnvelopePoint>>,
    /// Mute envelope. Values > 0.5 mute the track at that time.
    mute_env: Option<Vec<EnvelopePoint>>,
    /// Pre-FX volume envelope. Without an FX chain to insert between
    /// pre/post, this acts as an additional multiplier alongside the
    /// main volume envelope.
    volume_prefx_env: Option<Vec<EnvelopePoint>>,
    /// Pre-FX pan envelope. Blended additively with the static + main
    /// pan envelope, then clamped.
    pan_prefx_env: Option<Vec<EnvelopePoint>>,
}

#[derive(Default)]
struct FxChainSnapshot {
    chain: Vec<String>,
    enabled: Vec<bool>,
    params: Vec<Vec<(u32, f64)>>,
}

struct SendSnapshot {
    dest_guid: String,
    volume: f64,
    pan: f64,
    muted: bool,
    /// Optional per-send envelopes. Block-evaluated alongside track
    /// envelopes; see `effective_send_*` helpers.
    volume_env: Option<Vec<EnvelopePoint>>,
    pan_env: Option<Vec<EnvelopePoint>>,
    mute_env: Option<Vec<EnvelopePoint>>,
}

struct ItemSnapshot {
    take_guid: Option<String>,
    audio: Option<Arc<AudioSource>>,
    /// Fixed lane this item sits on (lane-enabled tracks only).
    fixed_lane: Option<u32>,
    position_seconds: f64,
    length_seconds: f64,
    fade_in_seconds: f64,
    fade_out_seconds: f64,
    muted: bool,
    item_volume: f64,
    take_volume: f64,
    play_rate: f64,
    start_offset_seconds: f64,
    /// Take pitch in semitones (added to envelope-driven pitch).
    take_pitch_semitones: f64,
    /// Per-take envelopes. All in item-relative time (0 at item
    /// start). Evaluated at block midpoint.
    take_volume_env: Option<Vec<EnvelopePoint>>,
    take_pan_env: Option<Vec<EnvelopePoint>>,
    take_mute_env: Option<Vec<EnvelopePoint>>,
    take_pitch_env: Option<Vec<EnvelopePoint>>,
    /// Sorted absolute-time MIDI notes on this item (only populated
    /// for MIDI takes — empty Vec for audio items). Each entry is
    /// `(start_seconds, length_seconds, channel, pitch, velocity)`
    /// in project time, already converted from PPQ at snapshot time
    /// using the project's static tempo. Per-block note events fall
    /// out by intersecting these against the render window.
    midi_notes: Vec<MidiNoteSnapshot>,
    /// Sorted absolute-time CC / pitch-bend / program-change /
    /// sysex events. Emitted to the plugin per-block as raw MIDI
    /// messages, in the same stream as the note events.
    midi_other: Vec<MidiOtherSnapshot>,
    /// Per-note expression points (MPE-style). Translated to
    /// `PluginNoteExpression` per block.
    note_expressions: Vec<NoteExpressionSnapshot>,
}

#[derive(Clone, Copy)]
struct NoteExpressionSnapshot {
    time_seconds: f64,
    channel: u8,
    note: u8,
    dimension: daw_proto::midi::NoteExpressionDim,
    value: f64,
}

#[derive(Clone, Copy)]
struct MidiNoteSnapshot {
    start_seconds: f64,
    length_seconds: f64,
    channel: u8,
    pitch: u8,
    velocity: u8,
}

#[derive(Clone)]
struct MidiOtherSnapshot {
    time_seconds: f64,
    message: daw_proto::MidiMessage,
}

/// Render a stereo master block.
///
/// Keep one renderer alive across blocks: the track snapshot is cached
/// against the project's mutation revision, so steady-state playback
/// (no edits) re-walks nothing — REAPER's "graph is cheap, only
/// rebuild on change" model. A throwaway renderer still works, it just
/// re-snapshots every call.
pub struct ProjectRenderer {
    daw: Standalone,
    project_guid: String,
    sample_rate: u32,
    /// `(project revision, snapshot)` — refreshed when the revision moves.
    cache: std::sync::Mutex<Option<(u64, Arc<Vec<TrackSnapshot>>)>>,
}

impl ProjectRenderer {
    pub fn new(daw: &Standalone, project_guid: &str, sample_rate: u32) -> Self {
        Self {
            daw: daw.clone(),
            project_guid: project_guid.to_string(),
            sample_rate,
            cache: std::sync::Mutex::new(None),
        }
    }

    /// Render `frames` stereo frames starting at `start_frame` (in
    /// output-rate samples). Returns a fresh `StereoBuffer`.
    pub fn render_block(&self, start_frame: u64, frames: usize) -> StereoBuffer {
        let mut master = StereoBuffer::zeroed(frames, self.sample_rate);
        if frames == 0 {
            return master;
        }

        let snapshot = match self.snapshot_tracks() {
            Some(s) => s,
            None => return master,
        };
        let start_seconds = start_frame as f64 / self.sample_rate as f64;
        let end_seconds = start_seconds + (frames as f64 / self.sample_rate as f64);
        let any_soloed = snapshot.iter().any(|t| t.soloed);

        // Allocate per-track stereo buses keyed by guid.
        let mut buses: std::collections::HashMap<String, StereoBuffer> =
            std::collections::HashMap::with_capacity(snapshot.len());
        for t in snapshot.iter() {
            buses.insert(
                t.guid.clone(),
                StereoBuffer::zeroed(frames, self.sample_rate),
            );
        }

        // Dirty flags: a bus that never receives signal (no items in
        // the window, no sends, no children, no synth FX) is all
        // zeros — every later stage can skip it outright. With a big
        // session only a fraction of tracks are audible in any block,
        // so this is the difference between O(active) and O(all)
        // per-frame work.
        let n = snapshot.len();
        let mut dirty = vec![false; n];

        // 1) Item playback into per-track buses. Item-level envelopes
        // (take volume / pan / mute / pitch) are evaluated per-frame
        // inside mix_item_into_bus via cursors.
        for (ti, t) in snapshot.iter().enumerate() {
            if any_soloed && !t.soloed {
                continue;
            }
            let bus = buses.get_mut(&t.guid).expect("bus pre-allocated");
            for item in &t.items {
                if item.muted {
                    continue;
                }
                // Fixed lanes: only items on PLAYING lanes sound —
                // the rest are alternate takes (REAPER 7 comping).
                if t.lane_play_mask != 0
                    && let Some(lane) = item.fixed_lane
                    && (lane >= 64 || t.lane_play_mask & (1u64 << lane) == 0)
                {
                    continue;
                }
                let Some(audio) = &item.audio else { continue };
                dirty[ti] |= mix_item_into_bus(
                    bus,
                    audio,
                    item,
                    start_seconds,
                    end_seconds,
                    self.sample_rate,
                );
            }
        }

        // Debug tracing: FTS_RENDER_DEBUG=1 dumps per-track bus RMS at
        // each stage so silent-master bugs can be localized headlessly.
        // Cached — render_block runs on the audio thread.
        static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let debug = *DEBUG.get_or_init(|| std::env::var("FTS_RENDER_DEBUG").is_ok());
        if debug {
            let rms = |b: &StereoBuffer| {
                (b.samples
                    .iter()
                    .map(|s| (*s as f64) * (*s as f64))
                    .sum::<f64>()
                    / b.samples.len().max(1) as f64)
                    .sqrt()
            };
            for t in snapshot.iter() {
                if t.items.is_empty() {
                    continue;
                }
                if let Some(b) = buses.get(&t.guid) {
                    let v = rms(b);
                    if v > 1e-9 {
                        eprintln!("[stage1] {:<24} items={} rms={v:.6}", t.name, t.items.len());
                    }
                }
            }
        }

        // 1.5) FX chain: pipe each track's bus through every loaded
        // plugin in order. The plugin instance map lives separately
        // from ProjectState so the audio thread doesn't block project
        // mutations on the proto side. Synthetic / unloaded FX are
        // skipped — no DSP, no work.
        {
            let mut plugins = self
                .daw
                .plugin_instances
                .lock()
                .expect("plugin_instances poisoned");
            // Per-block scratch — same shape as the bus's interleaved
            // f32 stereo. Allocated once per render call, reused per
            // plugin within a track.
            let mut in_l = vec![0.0f32; frames];
            let mut in_r = vec![0.0f32; frames];
            let mut out_l = vec![0.0f32; frames];
            let mut out_r = vec![0.0f32; frames];
            for (ti, t) in snapshot.iter().enumerate() {
                if any_soloed && !t.soloed {
                    continue;
                }
                if t.fx_chain.is_empty() {
                    continue;
                }
                let Some(bus) = buses.get_mut(&t.guid) else {
                    continue;
                };
                // Collect MIDI + note-expression events for this
                // block. Both lists are delivered to every plugin in
                // the chain (REAPER's default) — plugins that don't
                // consume MIDI just ignore them. Limiting MIDI to
                // plugin[0] is a different mode the user can switch
                // to via a future per-track flag.
                let midi_events =
                    collect_midi_events(t, start_seconds, end_seconds, self.sample_rate, frames);
                let note_expr_events = collect_note_expressions(
                    t,
                    start_seconds,
                    end_seconds,
                    self.sample_rate,
                    frames,
                );
                for (i, fx_guid) in t.fx_chain.iter().enumerate() {
                    if !t.fx_enabled.get(i).copied().unwrap_or(true) {
                        continue;
                    }
                    let Some(plugin) = plugins.get_mut(fx_guid) else {
                        continue; // synthetic / not loaded
                    };
                    if !plugin.is_prepared() {
                        // Lazy prepare on first use. If activation
                        // fails just bypass this plugin for now.
                        if plugin
                            .prepare(self.sample_rate as f64, frames as u32)
                            .is_err()
                        {
                            continue;
                        }
                    }
                    // De-interleave → process → re-interleave.
                    for f in 0..frames {
                        in_l[f] = bus.samples[f * 2];
                        in_r[f] = bus.samples[f * 2 + 1];
                    }
                    for v in out_l.iter_mut().chain(out_r.iter_mut()) {
                        *v = 0.0;
                    }
                    let param_events = t.fx_params.get(i).map(Vec::as_slice).unwrap_or(&[]);
                    let events = crate::plugin::PluginEvents {
                        params: param_events,
                        midi: &midi_events,
                        note_expressions: &note_expr_events,
                    };
                    let _ = i; // index is preserved for future
                    // "first-FX-only" mode toggle; currently every
                    // FX in the chain sees the same MIDI stream.
                    if plugin
                        .process_block(&in_l, &in_r, &mut out_l, &mut out_r, &events)
                        .is_err()
                    {
                        continue;
                    }
                    for f in 0..frames {
                        bus.samples[f * 2] = out_l[f];
                        bus.samples[f * 2 + 1] = out_r[f];
                    }
                    // A plugin ran — it may synthesize (MIDI → audio),
                    // so the bus can carry signal even with no items.
                    dirty[ti] = true;
                }
            }
        }

        // 2–4) Per-track processing in CHILDREN-FIRST order: a track's bus
        // is gained/panned, its sends dispatched, and the result summed into
        // its folder parent's bus *before* the parent's own gain applies —
        // REAPER's folder routing (the parent fader/FX scale the children's
        // sum plus the parent's own items). Top-level parent-send tracks sum
        // to master.
        let idx_by_guid: std::collections::HashMap<&str, usize> = snapshot
            .iter()
            .enumerate()
            .map(|(i, t)| (t.guid.as_str(), i))
            .collect();
        // Topological processing order over the ROUTING GRAPH: a track must
        // be fully processed before anything it feeds — its folder parent
        // (children sum upward) and every send destination (busses receive
        // post-fader signal). Depth ordering alone breaks bus mixes: a send
        // landing on an already-processed bus would never be forwarded
        // (the session's whole drum mix flows Drums → DRUM BUS → MIX BUS).
        // Kahn's algorithm; any cycle remainder (feedback loops) appends in
        // index order.
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indeg = vec![0usize; n];
        let mut edge = |a: usize, b: usize| {
            if a != b {
                adj[a].push(b);
                indeg[b] += 1;
            }
        };
        for (i, t) in snapshot.iter().enumerate() {
            if let Some(&pi) = t.parent_guid.as_deref().and_then(|g| idx_by_guid.get(g)) {
                edge(i, pi);
            }
            for snd in &t.sends {
                if let Some(&di) = idx_by_guid.get(snd.dest_guid.as_str()) {
                    edge(i, di);
                }
            }
        }
        let mut order: Vec<usize> = Vec::with_capacity(n);
        let mut queue: std::collections::VecDeque<usize> =
            (0..n).filter(|&i| indeg[i] == 0).collect();
        while let Some(i) = queue.pop_front() {
            order.push(i);
            for &j in &adj[i] {
                indeg[j] -= 1;
                if indeg[j] == 0 {
                    queue.push_back(j);
                }
            }
        }
        if order.len() < n {
            for i in 0..n {
                if indeg[i] > 0 {
                    order.push(i);
                }
            }
        }

        // Solo routing: with folder summing, a soloed track's signal flows
        // THROUGH its ancestors — they (and its descendants) must keep
        // passing audio even though they aren't soloed themselves.
        let solo_pass: Option<Vec<bool>> = any_soloed.then(|| {
            let mut pass = vec![false; snapshot.len()];
            for (i, t) in snapshot.iter().enumerate() {
                if !t.soloed {
                    continue;
                }
                // The soloed track + ancestors.
                pass[i] = true;
                let mut cur = i;
                while let Some(pg) = snapshot[cur].parent_guid.as_deref() {
                    match idx_by_guid.get(pg) {
                        Some(&pi) if !pass[pi] => {
                            pass[pi] = true;
                            cur = pi;
                        }
                        _ => break,
                    }
                }
                // Descendants (soloing a folder solos its children).
                for (j, c) in snapshot.iter().enumerate() {
                    let mut cur = j;
                    let mut hops = 0;
                    while let Some(pg) = snapshot[cur].parent_guid.as_deref() {
                        match idx_by_guid.get(pg) {
                            Some(&pi) if hops < 64 => {
                                if pi == i {
                                    pass[j] = true;
                                    break;
                                }
                                hops += 1;
                                cur = pi;
                            }
                            _ => break,
                        }
                    }
                    let _ = c;
                }
            }
            pass
        });
        let passes = |i: usize| -> bool {
            match &solo_pass {
                Some(p) => p[i],
                None => true,
            }
        };

        // Per-track meter bank: post-fader block peaks, written once
        // per track per block (lock-free atomics — safe from the audio
        // callback). Cell index == project track index == snapshot
        // index, matching `TrackRef::Index` on the Peaks service.
        let meters = self.daw.meters();

        let inv_rate = 1.0 / self.sample_rate as f64;
        for &ti in &order {
            let t = &snapshot[ti];
            if !passes(ti) {
                // Solo-skipped tracks meter as silence.
                if let Some(cell) = meters.cell(ti) {
                    cell.write(0.0, 0.0, crate::metering::HOLD_DECAY);
                }
                continue;
            }
            // Clean bus = all zeros: gain scales nothing, sends add
            // nothing, parent sums add nothing. Skip the per-frame
            // work entirely; only active tracks cost CPU.
            if !dirty[ti] {
                if let Some(cell) = meters.cell(ti) {
                    cell.write(0.0, 0.0, crate::metering::HOLD_DECAY);
                }
                continue;
            }

            // Gain + pan + polarity, per-frame envelopes.
            if let Some(bus) = buses.get_mut(&t.guid) {
                let mut c_vol = t.volume_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_pvol = t.volume_prefx_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_pan = t.pan_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_ppan = t.pan_prefx_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_mute = t.mute_env.as_ref().map(|p| EnvelopeCursor::new(p));
                for frame in 0..bus.frames {
                    let time = start_seconds + frame as f64 * inv_rate;
                    let v_main = c_vol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                    let v_prefx = c_pvol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                    let vol = t.volume * v_main * v_prefx;
                    let p_main = c_pan
                        .as_mut()
                        .and_then(|c| c.eval_at(time))
                        .map(|v| (v - 0.5) * 2.0)
                        .unwrap_or(0.0);
                    let p_prefx = c_ppan
                        .as_mut()
                        .and_then(|c| c.eval_at(time))
                        .map(|v| (v - 0.5) * 2.0)
                        .unwrap_or(0.0);
                    let pan = (t.pan + p_main + p_prefx).clamp(-1.0, 1.0);
                    let env_muted = c_mute
                        .as_mut()
                        .and_then(|c| c.eval_at(time))
                        .map(|v| v > 0.5)
                        .unwrap_or(false);
                    let muted = t.muted || env_muted;
                    if muted {
                        bus.samples[frame * 2] = 0.0;
                        bus.samples[frame * 2 + 1] = 0.0;
                        continue;
                    }
                    // Polarity invert folds into the gain sign.
                    let sign = if t.phase_inverted { -1.0 } else { 1.0 };
                    let lg = ((1.0 - pan) * 0.5).sqrt() * vol * sign;
                    let rg = ((1.0 + pan) * 0.5).sqrt() * vol * sign;
                    bus.samples[frame * 2] *= lg as f32;
                    bus.samples[frame * 2 + 1] *= rg as f32;
                }
            }

            // Sends — additive into destination buses, per-frame. (Read the
            // source once so we can mutably borrow dest buses.)
            let src = match buses.get(&t.guid) {
                Some(b) => b.samples.clone(),
                None => continue,
            };

            // Meter the post-fader signal (folder children already
            // summed in — topo order guarantees the bus is complete).
            if let Some(cell) = meters.cell(ti) {
                let (mut pl, mut pr) = (0.0f32, 0.0f32);
                for fr in src.chunks_exact(2) {
                    pl = pl.max(fr[0].abs());
                    pr = pr.max(fr[1].abs());
                }
                cell.write(pl, pr, crate::metering::HOLD_DECAY);
            }

            if debug {
                let v = (src.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>()
                    / src.len().max(1) as f64)
                    .sqrt();
                if v > 1e-9 {
                    eprintln!(
                        "[stage2] {:<24} rms={v:.6} vol={:.3} ps={} pg={} hw={} sends={}",
                        t.name,
                        t.volume,
                        t.parent_send,
                        t.parent_guid
                            .as_deref()
                            .map(|g| idx_by_guid.contains_key(g))
                            .map(|b| if b { "ok" } else { "MISS" })
                            .unwrap_or("none"),
                        t.hw_out,
                        t.sends.len()
                    );
                }
            }
            for snd in &t.sends {
                if snd.muted {
                    continue;
                }
                let dest_bus = match buses.get_mut(&snd.dest_guid) {
                    Some(b) => b,
                    None => continue,
                };
                if let Some(&di) = idx_by_guid.get(snd.dest_guid.as_str()) {
                    dirty[di] = true;
                }
                let mut c_vol = snd.volume_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_pan = snd.pan_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_mute = snd.mute_env.as_ref().map(|p| EnvelopeCursor::new(p));
                for frame in 0..dest_bus.frames {
                    let time = start_seconds + frame as f64 * inv_rate;
                    let env_muted = c_mute
                        .as_mut()
                        .and_then(|c| c.eval_at(time))
                        .map(|v| v > 0.5)
                        .unwrap_or(false);
                    if env_muted {
                        continue;
                    }
                    let v_env = c_vol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                    let vol = snd.volume * v_env;
                    let p_off = c_pan
                        .as_mut()
                        .and_then(|c| c.eval_at(time))
                        .map(|v| (v - 0.5) * 2.0)
                        .unwrap_or(0.0);
                    let pan = (snd.pan + p_off).clamp(-1.0, 1.0);
                    let lg = (((1.0 - pan) * 0.5).sqrt() * vol) as f32;
                    let rg = (((1.0 + pan) * 0.5).sqrt() * vol) as f32;
                    dest_bus.samples[frame * 2] += src[frame * 2] * lg;
                    dest_bus.samples[frame * 2 + 1] += src[frame * 2 + 1] * rg;
                }
            }

            // Parent / master sum.
            if t.parent_send {
                let parent_idx = t
                    .parent_guid
                    .as_deref()
                    .and_then(|g| idx_by_guid.get(g).copied());
                match parent_idx {
                    Some(pi) => {
                        let pg = snapshot[pi].guid.clone();
                        if let Some(parent_bus) = buses.get_mut(&pg) {
                            for (m, smp) in parent_bus.samples.iter_mut().zip(src.iter()) {
                                *m += *smp;
                            }
                            dirty[pi] = true;
                        }
                    }
                    None => {
                        for (m, smp) in master.samples.iter_mut().zip(src.iter()) {
                            *m += *smp;
                        }
                        if debug {
                            let v = (src.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>()
                                / src.len().max(1) as f64)
                                .sqrt();
                            if v > 1e-9 {
                                eprintln!("[->master] {:<24} rms={v:.6}", t.name);
                            }
                        }
                    }
                }
            } else if t.hw_out {
                // No parent send but routed to hardware — v0 sums hw
                // outs straight to the master device buffer.
                for (m, smp) in master.samples.iter_mut().zip(src.iter()) {
                    *m += *smp;
                }
                if debug {
                    let v = (src.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>()
                        / src.len().max(1) as f64)
                        .sqrt();
                    if v > 1e-9 {
                        eprintln!("[hw->master] {:<24} rms={v:.6}", t.name);
                    }
                }
            }
        }

        master
    }

    fn snapshot_tracks(&self) -> Option<Arc<Vec<TrackSnapshot>>> {
        let revision = self.daw.read_project(&self.project_guid, |p| p.revision)?;
        if let Ok(cache) = self.cache.lock()
            && let Some((cached_rev, snap)) = cache.as_ref()
            && *cached_rev == revision
        {
            return Some(snap.clone());
        }
        // Build + revision in ONE lock pass so the cached pair is
        // always internally consistent.
        let (revision, snap) = self.daw.read_project(&self.project_guid, |p| {
            let tempo_map = TempoMap::from_state(p);
            let mut tracks = Vec::with_capacity(p.tracks.len());
            for t in &p.tracks {
                tracks.push(snapshot_track(p, t, &tempo_map));
            }
            (p.revision, Arc::new(tracks))
        })?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((revision, snap.clone()));
        }
        Some(snap)
    }
}

/// Cumulative tempo-map: sorted list of `(time_seconds, beat, bpm)`
/// segment starts. Each segment runs at constant BPM until the next
/// entry; the final segment extends forever. Built once per
/// `snapshot_track` call from `p.tempo_points`.
///
/// The `beat` field is the project-time beat (quarter notes from the
/// origin) at which the segment begins — i.e. the PPQ of the segment
/// boundary. PPQ values on `MidiNote.start_ppq` are *project-time*
/// quarter notes when the source is a tempo-locked take.
struct TempoMap {
    segments: Vec<TempoSegment>,
}

#[derive(Clone, Copy)]
struct TempoSegment {
    /// Time in seconds at the segment start.
    start_seconds: f64,
    /// Beat (quarter notes from origin) at the segment start.
    start_beat: f64,
    /// Tempo within this segment (constant BPM).
    bpm: f64,
}

impl TempoMap {
    /// Build from `ProjectState`. Always returns at least one
    /// segment — a fallback `(0, 0, project_bpm)` covers projects
    /// with no tempo points.
    fn from_state(p: &ProjectState) -> Self {
        let default_bpm = p.transport.tempo.bpm().max(1.0);
        let mut pts: Vec<(f64, f64)> = p
            .tempo_points
            .iter()
            .map(|tp| (tp.position_seconds(), tp.bpm.max(1.0)))
            .collect();
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // Drop duplicates at the same time (keep the last write).
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);

        let mut segments: Vec<TempoSegment> = Vec::new();
        // Always seed with a segment at time=0 so any PPQ before
        // the first tempo point still resolves to a number.
        if pts.first().map(|p| p.0 > 1e-9).unwrap_or(true) {
            segments.push(TempoSegment {
                start_seconds: 0.0,
                start_beat: 0.0,
                bpm: pts.first().map(|p| p.1).unwrap_or(default_bpm),
            });
        }
        for (time_seconds, bpm) in pts {
            // Beat at this segment start = previous beat + (time delta) * prev_bpm / 60.
            let start_beat = segments
                .last()
                .map(|prev| prev.start_beat + (time_seconds - prev.start_seconds) * prev.bpm / 60.0)
                .unwrap_or(0.0);
            segments.push(TempoSegment {
                start_seconds: time_seconds,
                start_beat,
                bpm,
            });
        }
        Self { segments }
    }

    /// Convert seconds to project-time beats (inverse of `beat_to_seconds`).
    fn seconds_to_beat(&self, seconds: f64) -> f64 {
        let mut idx = 0usize;
        for (i, s) in self.segments.iter().enumerate() {
            if s.start_seconds <= seconds {
                idx = i;
            } else {
                break;
            }
        }
        let seg = self.segments[idx];
        seg.start_beat + (seconds - seg.start_seconds) * seg.bpm / 60.0
    }

    /// Convert project-time beats (= PPQ in quarter notes) to seconds.
    fn beat_to_seconds(&self, beat: f64) -> f64 {
        // Find the segment whose start_beat ≤ beat.
        let mut idx = 0usize;
        for (i, s) in self.segments.iter().enumerate() {
            if s.start_beat <= beat {
                idx = i;
            } else {
                break;
            }
        }
        let seg = self.segments[idx];
        seg.start_seconds + (beat - seg.start_beat) * 60.0 / seg.bpm
    }
}

fn snapshot_track(p: &ProjectState, t: &daw_proto::Track, tempo_map: &TempoMap) -> TrackSnapshot {
    let parent_send = p
        .track_ext
        .get(&t.guid)
        .map(|e| e.parent_send_enabled)
        .unwrap_or(true);

    // Sends: only the Send variant matters (Receives mirror; HW out
    // for v0 also routes to master via parent_send equivalent).
    // `send_index` is the position in the source track's send list
    // — also the addressing key for `EnvelopeKey::Send`.
    let sends: Vec<SendSnapshot> = p
        .sends
        .get(&t.guid)
        .map(|v| {
            v.iter()
                .filter(|r| r.route_type == RouteType::Send)
                .enumerate()
                .filter_map(|(idx, r)| {
                    let dest_guid = r.dest_track_guid.clone()?;
                    let key = |kind| EnvelopeKey::Send {
                        send_index: idx as u32,
                        kind,
                    };
                    let volume_env = active_envelope(
                        p.envelopes
                            .get(&(t.guid.clone(), key(SendEnvelopeKind::Volume))),
                    );
                    let pan_env = active_envelope(
                        p.envelopes
                            .get(&(t.guid.clone(), key(SendEnvelopeKind::Pan))),
                    );
                    let mute_env = active_envelope(
                        p.envelopes
                            .get(&(t.guid.clone(), key(SendEnvelopeKind::Mute))),
                    );
                    Some(SendSnapshot {
                        dest_guid,
                        volume: r.volume,
                        pan: r.pan,
                        muted: r.muted,
                        volume_env,
                        pan_env,
                        mute_env,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Items on this track.
    let mut items = Vec::new();
    if let Some(item_guids) = p.items_by_track.get(&t.guid) {
        for ig in item_guids {
            let Some(ie) = p.items.get(ig) else { continue };
            let item = &ie.item;
            // Active take.
            let take_guid_opt = p
                .takes
                .get(ig)
                .and_then(|tl| tl.takes.get(tl.active_idx as usize).map(|t| t.guid.clone()));
            let audio = take_guid_opt
                .as_ref()
                .and_then(|tg| p.audio_sources.get(tg).cloned());
            let (take_volume, play_rate, start_offset, take_pitch) = p
                .takes
                .get(ig)
                .and_then(|tl| tl.takes.get(tl.active_idx as usize))
                .map(|tk| {
                    (
                        tk.volume,
                        tk.play_rate,
                        tk.start_offset.as_seconds(),
                        tk.pitch,
                    )
                })
                .unwrap_or((1.0, 1.0, 0.0, 0.0));
            // Look up each take envelope by `(owner="", EnvelopeKey::Take{..})`
            // — matches the storage convention in automation.rs.
            // Capture take_guid first so the closure can reuse it
            // without moving from `take_guid_opt`.
            let tg_for_env = take_guid_opt.clone();
            let take_env = |kind: TakeEnvelopeKind| -> Option<Vec<EnvelopePoint>> {
                let tg = tg_for_env.as_ref()?;
                active_envelope(p.envelopes.get(&(
                    String::new(),
                    EnvelopeKey::Take {
                        item_guid: ig.clone(),
                        take_guid: tg.clone(),
                        kind,
                    },
                )))
            };
            let take_volume_env = take_env(TakeEnvelopeKind::Volume);
            let take_pan_env = take_env(TakeEnvelopeKind::Pan);
            let take_mute_env = take_env(TakeEnvelopeKind::Mute);
            let take_pitch_env = take_env(TakeEnvelopeKind::Pitch);

            // PPQ → seconds via the project tempo map. PPQ on a take
            // is *relative* to the item's start (REAPER stores notes
            // local to the source). Convert by:
            //   1. Locate the item start as an absolute beat via
            //      the inverse tempo-map (seconds→beat).
            //   2. Compose: absolute beat = item_start_beat + note_ppq.
            //   3. Tempo-map that absolute beat back to seconds.
            // For constant-tempo projects this collapses to the old
            // formula `item_pos + ppq / qn_per_sec`.
            let item_pos = item.position.as_seconds();
            let item_start_beat = tempo_map.seconds_to_beat(item_pos);
            let to_seconds = |ppq: f64| tempo_map.beat_to_seconds(item_start_beat + ppq);

            let midi_other: Vec<MidiOtherSnapshot> = take_guid_opt
                .as_ref()
                .map(|tg| {
                    use daw_proto::MidiMessage;
                    let mut out: Vec<MidiOtherSnapshot> = Vec::new();
                    if let Some(ccs) = p.midi_ccs.get(tg) {
                        for cc in ccs {
                            out.push(MidiOtherSnapshot {
                                time_seconds: to_seconds(cc.position_ppq),
                                message: MidiMessage::ControlChange {
                                    channel: cc.channel,
                                    controller: cc.controller,
                                    value: cc.value,
                                },
                            });
                        }
                    }
                    if let Some(pbs) = p.midi_pitch_bends.get(tg) {
                        for pb in pbs {
                            out.push(MidiOtherSnapshot {
                                time_seconds: to_seconds(pb.position_ppq),
                                message: MidiMessage::PitchBend {
                                    channel: pb.channel,
                                    value: pb.value,
                                },
                            });
                        }
                    }
                    if let Some(pcs) = p.midi_program_changes.get(tg) {
                        for pc in pcs {
                            out.push(MidiOtherSnapshot {
                                time_seconds: to_seconds(pc.position_ppq),
                                message: MidiMessage::ProgramChange {
                                    channel: pc.channel,
                                    program: pc.program,
                                },
                            });
                        }
                    }
                    if let Some(sx) = p.midi_sysex.get(tg) {
                        for s in sx {
                            out.push(MidiOtherSnapshot {
                                time_seconds: to_seconds(s.position_ppq),
                                message: MidiMessage::SysEx(s.data.clone()),
                            });
                        }
                    }
                    if let Some(cps) = p.midi_channel_pressures.get(tg) {
                        for cp in cps {
                            out.push(MidiOtherSnapshot {
                                time_seconds: to_seconds(cp.position_ppq),
                                message: MidiMessage::ChannelPressure {
                                    channel: cp.channel,
                                    pressure: cp.pressure,
                                },
                            });
                        }
                    }
                    if let Some(pps) = p.midi_poly_pressures.get(tg) {
                        for pp in pps {
                            out.push(MidiOtherSnapshot {
                                time_seconds: to_seconds(pp.position_ppq),
                                message: MidiMessage::PolyPressure {
                                    channel: pp.channel,
                                    note: pp.note,
                                    pressure: pp.pressure,
                                },
                            });
                        }
                    }
                    out.sort_by(|a, b| {
                        a.time_seconds
                            .partial_cmp(&b.time_seconds)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    out
                })
                .unwrap_or_default();

            let note_expressions_snap: Vec<NoteExpressionSnapshot> = take_guid_opt
                .as_ref()
                .and_then(|tg| p.midi_note_expressions.get(tg))
                .map(|ne_list| {
                    let mut out: Vec<NoteExpressionSnapshot> = ne_list
                        .iter()
                        .map(|n| NoteExpressionSnapshot {
                            time_seconds: to_seconds(n.position_ppq),
                            channel: n.channel,
                            note: n.note,
                            dimension: n.dimension,
                            value: n.value,
                        })
                        .collect();
                    out.sort_by(|a, b| {
                        a.time_seconds
                            .partial_cmp(&b.time_seconds)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    out
                })
                .unwrap_or_default();

            let midi_notes: Vec<MidiNoteSnapshot> = take_guid_opt
                .as_ref()
                .and_then(|tg| p.midi_notes.get(tg))
                .map(|notes| {
                    let mut out: Vec<MidiNoteSnapshot> = notes
                        .iter()
                        .filter(|n| !n.muted && n.velocity > 0)
                        .map(|n| {
                            // PPQ length is also tempo-dependent —
                            // compute end via the tempo map and
                            // subtract.
                            let start = to_seconds(n.start_ppq);
                            let end = to_seconds(n.start_ppq + n.length_ppq);
                            MidiNoteSnapshot {
                                start_seconds: start,
                                length_seconds: (end - start).max(0.0),
                                channel: n.channel,
                                pitch: n.pitch,
                                velocity: n.velocity,
                            }
                        })
                        .collect();
                    out.sort_by(|a, b| {
                        a.start_seconds
                            .partial_cmp(&b.start_seconds)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    out
                })
                .unwrap_or_default();

            items.push(ItemSnapshot {
                take_guid: take_guid_opt,
                audio,
                fixed_lane: item.fixed_lane,
                position_seconds: item.position.as_seconds(),
                length_seconds: item.length.as_seconds(),
                fade_in_seconds: item.fade_in_length.as_seconds(),
                fade_out_seconds: item.fade_out_length.as_seconds(),
                muted: item.muted,
                item_volume: item.volume,
                take_volume,
                play_rate: if play_rate.abs() < 1e-9 {
                    1.0
                } else {
                    play_rate
                },
                start_offset_seconds: start_offset,
                take_pitch_semitones: take_pitch,
                take_volume_env,
                take_pan_env,
                take_mute_env,
                take_pitch_env,
                midi_notes,
                midi_other,
                note_expressions: note_expressions_snap,
            });
        }
    }

    let track_env = |ty: EnvelopeType| {
        active_envelope(p.envelopes.get(&(t.guid.clone(), EnvelopeKey::Track(ty))))
    };

    // FX chain (track-side only — input FX is a future addition).
    let fx = p
        .fx_chains
        .get(&crate::sync::FxChainKey::Track(t.guid.clone()))
        .map(|chain| {
            let mut guids = Vec::with_capacity(chain.len());
            let mut enabled = Vec::with_capacity(chain.len());
            let mut params = Vec::with_capacity(chain.len());
            for e in chain {
                guids.push(e.fx.guid.clone());
                enabled.push(e.fx.enabled);
                params.push(e.params.iter().map(|(&id, &value)| (id, value)).collect());
            }
            FxChainSnapshot {
                chain: guids,
                enabled,
                params,
            }
        })
        .unwrap_or_default();

    TrackSnapshot {
        guid: t.guid.clone(),
        name: t.name.clone(),
        volume: t.volume,
        pan: t.pan,
        phase_inverted: t.phase_inverted,
        muted: t.muted,
        soloed: t.soloed,
        parent_send,
        lane_play_mask: if t.lane_count > 0 {
            t.lane_play_mask
        } else {
            0
        },
        hw_out: p
            .hw_outputs
            .get(&t.guid)
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        parent_guid: t.parent_guid.clone(),
        sends,
        items,
        fx_chain: fx.chain,
        fx_enabled: fx.enabled,
        fx_params: fx.params,
        volume_env: track_env(EnvelopeType::Volume),
        pan_env: track_env(EnvelopeType::Pan),
        mute_env: track_env(EnvelopeType::Mute),
        volume_prefx_env: track_env(EnvelopeType::VolumePrefx),
        pan_prefx_env: track_env(EnvelopeType::PanPrefx),
    }
}

/// Walk every MIDI note on the track's items, emit NoteOn / NoteOff
/// events whose timestamps fall inside `[start_seconds, end_seconds)`.
/// Returns events sorted by sample offset — VST3's `IEventList`
/// (and most CLAP hosts) want note events monotonically ordered.
fn collect_midi_events(
    track: &TrackSnapshot,
    start_seconds: f64,
    end_seconds: f64,
    sample_rate: u32,
    frames: usize,
) -> Vec<crate::plugin::PluginMidiEvent> {
    use daw_proto::live_midi::MidiMessage;

    let mut out: Vec<crate::plugin::PluginMidiEvent> = Vec::new();
    let to_sample = |t_seconds: f64| -> u32 {
        let frame = ((t_seconds - start_seconds) * sample_rate as f64).floor() as i64;
        frame.clamp(0, (frames as i64).saturating_sub(1)) as u32
    };
    for item in &track.items {
        if item.muted {
            continue;
        }
        // Quick bail-out: skip items that can't possibly overlap.
        // MIDI events can fire from notes already started before
        // start_seconds (only their NoteOff matters), so we use the
        // full item span [position, position+length) plus the
        // outstanding-tail length already encoded in start_seconds.
        let item_end = item.position_seconds + item.length_seconds;
        if item_end < start_seconds || item.position_seconds >= end_seconds + 0.0 {
            // Note: items entirely before the block window may still
            // have notes that *end* during the block — but if the
            // item itself ends before start_seconds, no on-going note
            // can survive past it.
            if item_end < start_seconds {
                continue;
            }
        }
        for n in &item.midi_notes {
            // NoteOn falls in this block?
            if n.start_seconds >= start_seconds && n.start_seconds < end_seconds {
                out.push(crate::plugin::PluginMidiEvent {
                    offset: to_sample(n.start_seconds),
                    message: MidiMessage::NoteOn {
                        channel: n.channel,
                        note: n.pitch,
                        velocity: n.velocity,
                    },
                });
            }
            // NoteOff falls in this block?
            let end = n.start_seconds + n.length_seconds;
            if end >= start_seconds && end < end_seconds {
                out.push(crate::plugin::PluginMidiEvent {
                    offset: to_sample(end),
                    message: MidiMessage::NoteOff {
                        channel: n.channel,
                        note: n.pitch,
                        velocity: 0,
                    },
                });
            }
        }
        // CC / pitch bend / program change / sysex — all delivered at
        // their `time_seconds`, clamped into the block window.
        for ev in &item.midi_other {
            if ev.time_seconds >= start_seconds && ev.time_seconds < end_seconds {
                out.push(crate::plugin::PluginMidiEvent {
                    offset: to_sample(ev.time_seconds),
                    message: ev.message.clone(),
                });
            }
        }
    }
    out.sort_by_key(|e| e.offset);
    out
}

/// Walk every per-note expression point on the track's items and
/// emit those that fall inside `[start_seconds, end_seconds)` as
/// `PluginNoteExpression`s with sample offsets clamped to the block.
fn collect_note_expressions(
    track: &TrackSnapshot,
    start_seconds: f64,
    end_seconds: f64,
    sample_rate: u32,
    frames: usize,
) -> Vec<crate::plugin::PluginNoteExpression> {
    let mut out: Vec<crate::plugin::PluginNoteExpression> = Vec::new();
    let to_sample = |t_seconds: f64| -> u32 {
        let frame = ((t_seconds - start_seconds) * sample_rate as f64).floor() as i64;
        frame.clamp(0, (frames as i64).saturating_sub(1)) as u32
    };
    for item in &track.items {
        if item.muted {
            continue;
        }
        for ev in &item.note_expressions {
            if ev.time_seconds >= start_seconds && ev.time_seconds < end_seconds {
                out.push(crate::plugin::PluginNoteExpression {
                    offset: to_sample(ev.time_seconds),
                    channel: ev.channel,
                    note: ev.note,
                    dimension: ev.dimension,
                    value: ev.value,
                });
            }
        }
    }
    out.sort_by_key(|e| e.offset);
    out
}

/// Stateful per-frame envelope cursor. `eval_at(t)` advances through
/// the points list without rescanning from the start each call.
/// Construct fresh per render block.
///
/// Time *must* be monotonically non-decreasing across `eval_at` calls
/// — meets the audio callback contract where frames march forward.
struct EnvelopeCursor<'a> {
    points: &'a [EnvelopePoint],
    /// Index of the next point ahead of the cursor. Once
    /// `points[next].time > t`, the active segment is
    /// `points[next-1]..points[next]`.
    next: usize,
}

impl<'a> EnvelopeCursor<'a> {
    fn new(points: &'a [EnvelopePoint]) -> Self {
        Self { points, next: 1 }
    }

    /// Evaluate the envelope at `time_seconds`. Returns `None` only
    /// if the envelope is empty.
    fn eval_at(&mut self, time_seconds: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        // Clamp left of first point.
        let first = &self.points[0];
        if time_seconds <= first.time.as_seconds() {
            return Some(first.value);
        }
        // Clamp right of last point.
        let last = self.points.last().unwrap();
        if time_seconds >= last.time.as_seconds() {
            return Some(last.value);
        }
        // Advance `next` while it's still in the past.
        while self.next < self.points.len()
            && self.points[self.next].time.as_seconds() < time_seconds
        {
            self.next += 1;
        }
        if self.next == 0 {
            // Shouldn't happen given the clamp above, but be safe.
            self.next = 1;
        }
        let a = &self.points[self.next - 1];
        let b = &self.points[self.next];
        match a.shape {
            EnvelopeShape::Square => Some(a.value),
            _ => {
                let ta = a.time.as_seconds();
                let tb = b.time.as_seconds();
                let span = tb - ta;
                if span <= 0.0 {
                    return Some(b.value);
                }
                let f = (time_seconds - ta) / span;
                Some(a.value + (b.value - a.value) * f)
            }
        }
    }
}

/// Linear (or held, for Square) interpolation across an envelope's
/// points. Returns `None` if the envelope is empty (caller falls back
/// to the static fader value). Matches the `Automation::value_at`
/// algorithm in src/automation.rs.
fn eval_envelope(points: &[EnvelopePoint], time_seconds: f64) -> Option<f64> {
    if points.is_empty() {
        return None;
    }
    let first = &points[0];
    if time_seconds <= first.time.as_seconds() {
        return Some(first.value);
    }
    let last = points.last().unwrap();
    if time_seconds >= last.time.as_seconds() {
        return Some(last.value);
    }
    for i in 0..points.len() - 1 {
        let a = &points[i];
        let b = &points[i + 1];
        let ta = a.time.as_seconds();
        let tb = b.time.as_seconds();
        if time_seconds >= ta && time_seconds <= tb {
            return Some(match a.shape {
                EnvelopeShape::Square => a.value,
                _ => {
                    let span = tb - ta;
                    if span <= 0.0 {
                        b.value
                    } else {
                        let f = (time_seconds - ta) / span;
                        a.value + (b.value - a.value) * f
                    }
                }
            });
        }
    }
    None
}

/// Returns `true` if any sample was mixed into the bus (drives the
/// caller's dirty-bus tracking).
fn mix_item_into_bus(
    bus: &mut StereoBuffer,
    audio: &AudioSource,
    item: &ItemSnapshot,
    block_start_seconds: f64,
    block_end_seconds: f64,
    output_rate: u32,
) -> bool {
    let item_start = item.position_seconds;
    let item_end = item_start + item.length_seconds;

    // Quick out: item doesn't overlap this block.
    if item_end <= block_start_seconds || item_start >= block_end_seconds {
        return false;
    }

    let base_gain = (item.item_volume * item.take_volume) as f32;
    if base_gain == 0.0 {
        return false;
    }

    // Per-frame envelope cursors. All in item-relative time, which
    // advances monotonically as `frame` increments.
    let mut c_vol = item
        .take_volume_env
        .as_ref()
        .map(|p| EnvelopeCursor::new(p));
    let mut c_pan = item.take_pan_env.as_ref().map(|p| EnvelopeCursor::new(p));
    let mut c_mute = item.take_mute_env.as_ref().map(|p| EnvelopeCursor::new(p));
    let mut c_pitch = item.take_pitch_env.as_ref().map(|p| EnvelopeCursor::new(p));
    let has_take_pan = item.take_pan_env.is_some();

    let fade_in = item.fade_in_seconds.max(0.0);
    let fade_out = item.fade_out_seconds.max(0.0);

    let audio_rate = audio.sample_rate().max(1) as f64;
    let output_rate_f = output_rate as f64;

    // Pitch → rate multiplier. `powf` costs ~an entire frame's mixing
    // work, so fold the static case out of the loop; per-frame powf
    // only happens while a pitch envelope is actually present.
    let static_pitch_mult = if item.take_pitch_semitones == 0.0 {
        1.0
    } else {
        2f64.powf(item.take_pitch_semitones / 12.0)
    };
    let source_len = audio.frame_count() as f64;
    let last_frame = audio.frame_count().saturating_sub(1);

    let mut wrote = false;
    for frame in 0..bus.frames {
        let block_time = block_start_seconds + (frame as f64 / output_rate_f);
        if block_time < item_start || block_time >= item_end {
            continue;
        }
        let item_time = block_time - item_start;

        // Mute env first — early-out before paying for source lookup.
        let env_muted = c_mute
            .as_mut()
            .and_then(|c| c.eval_at(item_time))
            .map(|v| v > 0.5)
            .unwrap_or(false);
        if env_muted {
            continue;
        }

        // Pitch: static + envelope semitones → rate multiplier.
        let pitch_mult = match c_pitch.as_mut() {
            Some(c) => {
                let st = c.eval_at(item_time).unwrap_or(0.0);
                2f64.powf((item.take_pitch_semitones + st) / 12.0)
            }
            None => static_pitch_mult,
        };

        let source_time = item.start_offset_seconds + item_time * item.play_rate * pitch_mult;
        if source_time < 0.0 {
            continue;
        }
        let source_frame_f = source_time * audio_rate;
        if source_frame_f >= source_len {
            continue;
        }

        // Linear interpolation between two source frames.
        // (`as usize` truncates toward zero == floor for the
        // non-negative values guaranteed above.)
        let i0 = source_frame_f as usize;
        let frac = (source_frame_f - i0 as f64) as f32;
        let i1 = (i0 + 1).min(last_frame);
        let (l, r) = audio.stereo_interp(i0, i1, frac);

        // Volume envelope per-frame.
        let env_vol = c_vol
            .as_mut()
            .and_then(|c| c.eval_at(item_time))
            .unwrap_or(1.0) as f32;

        // Pan envelope per-frame (only engaged if envelope exists).
        let (pan_lg, pan_rg) = if has_take_pan {
            let v = c_pan
                .as_mut()
                .and_then(|c| c.eval_at(item_time))
                .unwrap_or(0.5);
            let p = ((v - 0.5) * 2.0).clamp(-1.0, 1.0) as f32;
            (((1.0 - p) * 0.5).sqrt(), ((1.0 + p) * 0.5).sqrt())
        } else {
            (1.0, 1.0)
        };

        // Fade in / out (linear).
        let mut fade = 1.0f32;
        if fade_in > 0.0 && item_time < fade_in {
            fade *= (item_time / fade_in) as f32;
        }
        let time_until_end = item_end - block_time;
        if fade_out > 0.0 && time_until_end < fade_out {
            fade *= (time_until_end / fade_out).max(0.0) as f32;
        }
        let g = base_gain * env_vol * fade;

        bus.samples[frame * 2] += l * g * pan_lg;
        bus.samples[frame * 2 + 1] += r * g * pan_rg;
        wrote = true;
    }
    wrote
}

fn sample_stereo_interp(
    samples: &[f32],
    channels: usize,
    i0: usize,
    i1: usize,
    frac: f32,
) -> (f32, f32) {
    let off0 = i0 * channels;
    let off1 = i1 * channels;
    let l = match channels {
        0 => 0.0,
        1 => {
            // Mono → both channels.
            let s0 = samples.get(off0).copied().unwrap_or(0.0);
            let s1 = samples.get(off1).copied().unwrap_or(0.0);
            s0 + (s1 - s0) * frac
        }
        _ => {
            let s0 = samples.get(off0).copied().unwrap_or(0.0);
            let s1 = samples.get(off1).copied().unwrap_or(0.0);
            s0 + (s1 - s0) * frac
        }
    };
    let r = match channels {
        0 | 1 => l,
        _ => {
            let s0 = samples.get(off0 + 1).copied().unwrap_or(0.0);
            let s1 = samples.get(off1 + 1).copied().unwrap_or(0.0);
            s0 + (s1 - s0) * frac
        }
    };
    (l, r)
}

fn apply_volume_pan(bus: &mut StereoBuffer, volume: f32, pan: f32) {
    // Constant-power pan; clamps pan to [-1, 1].
    let p = pan.clamp(-1.0, 1.0);
    let lg = ((1.0 - p) * 0.5).sqrt();
    let rg = ((1.0 + p) * 0.5).sqrt();
    for i in 0..bus.frames {
        let l = bus.samples[i * 2];
        let r = bus.samples[i * 2 + 1];
        bus.samples[i * 2] = l * lg * volume;
        bus.samples[i * 2 + 1] = r * rg * volume;
    }
}

fn add_with_volume_pan(dest: &mut StereoBuffer, src: &[f32], volume: f32, pan: f32) {
    let p = pan.clamp(-1.0, 1.0);
    let lg = ((1.0 - p) * 0.5).sqrt() * volume;
    let rg = ((1.0 + p) * 0.5).sqrt() * volume;
    for i in 0..dest.frames {
        dest.samples[i * 2] += src[i * 2] * lg;
        dest.samples[i * 2 + 1] += src[i * 2 + 1] * rg;
    }
}
