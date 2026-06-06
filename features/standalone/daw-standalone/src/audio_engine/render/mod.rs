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
//!
//! WASM-compatible: pure math + heap, no threads, no cpal. The cpal
//! mixer in `mixer.rs` wraps this on native; an AudioWorklet shim can
//! wrap it on the web.

mod envelope;
mod graph;
mod item_mix;
mod midi;
mod snapshot;

use std::sync::Arc;

use envelope::EnvelopeCursor;
use item_mix::mix_item_into_bus;
use midi::{collect_midi_events, collect_note_expressions};
use snapshot::{RenderSnapshot, TrackSnapshot};

use crate::sync::Standalone;

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
    /// De-interleave scratch for the FX stage.
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    /// Post-fader copy of the current track's bus, so sends / parent
    /// sums can read it while mutating destination buses.
    src: Vec<f32>,
    /// Send-tap copies (only filled when a send actually taps there).
    pre_fx_tap: Vec<f32>,
    pre_fader_tap: Vec<f32>,
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
        for v in [
            &mut self.in_l,
            &mut self.in_r,
            &mut self.out_l,
            &mut self.out_r,
        ] {
            if v.len() != frames {
                v.resize(frames, 0.0);
            }
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
pub struct ProjectRenderer {
    daw: Standalone,
    project_guid: String,
    sample_rate: u32,
    /// `(project revision, snapshot)` — refreshed when the revision moves.
    cache: std::sync::Mutex<Option<(u64, Arc<RenderSnapshot>)>>,
    /// Reusable block buffers. Mutex for `&self` access — the audio
    /// callback is the only steady-state caller, so it's uncontended.
    scratch: std::sync::Mutex<RenderScratch>,
}

impl ProjectRenderer {
    pub fn new(daw: &Standalone, project_guid: &str, sample_rate: u32) -> Self {
        Self {
            daw: daw.clone(),
            project_guid: project_guid.to_string(),
            sample_rate,
            cache: std::sync::Mutex::new(None),
            scratch: std::sync::Mutex::new(RenderScratch::default()),
        }
    }

    /// Render `frames` stereo frames starting at `start_frame` (in
    /// output-rate samples). Returns a fresh `StereoBuffer`.
    pub fn render_block(&self, start_frame: u64, frames: usize) -> StereoBuffer {
        let mut master = StereoBuffer::zeroed(frames, self.sample_rate);
        if frames == 0 {
            return master;
        }

        let snap = match self.snapshot() {
            Some(s) => s,
            None => return master,
        };
        let tracks = &snap.tracks;
        let n = tracks.len();
        let start_seconds = start_frame as f64 / self.sample_rate as f64;
        let end_seconds = start_seconds + (frames as f64 / self.sample_rate as f64);
        let any_soloed = snap.solo_pass.is_some();
        let passes = |i: usize| -> bool {
            match &snap.solo_pass {
                Some(p) => p[i],
                None => true,
            }
        };

        let mut scratch = self.scratch.lock().expect("render scratch poisoned");
        scratch.reset(n, frames, self.sample_rate);
        // Split-borrow every scratch field so the stages below can
        // hold disjoint &muts simultaneously.
        let RenderScratch {
            buses,
            dirty,
            in_l,
            in_r,
            out_l,
            out_r,
            src,
            pre_fx_tap,
            pre_fader_tap,
        } = &mut *scratch;

        // 1) Item playback into per-track buses. Item-level envelopes
        // (take volume / pan / mute / pitch) are evaluated per-frame
        // inside mix_item_into_bus via cursors.
        for (ti, t) in tracks.iter().enumerate() {
            if any_soloed && !t.soloed {
                continue;
            }
            let bus = &mut buses[ti];
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
            let rms = |s: &[f32]| {
                (s.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / s.len().max(1) as f64)
                    .sqrt()
            };
            for (ti, t) in tracks.iter().enumerate() {
                if t.items.is_empty() {
                    continue;
                }
                let v = rms(&buses[ti].samples);
                if v > 1e-9 {
                    eprintln!("[stage1] {:<24} items={} rms={v:.6}", t.name, t.items.len());
                }
            }
        }

        // Per-track meter bank: post-fader block peaks, written once
        // per track per block (lock-free atomics — safe from the audio
        // callback). Cell index == project track index == snapshot
        // index, matching `TrackRef::Index` on the Peaks service.
        let meters = self.daw.meters();

        // Plugin instances for the per-track FX stage inside the loop.
        // The map lives separately from ProjectState so the audio
        // thread doesn't block project mutations on the proto side.
        let mut plugins = self
            .daw
            .plugin_instances
            .lock()
            .expect("plugin_instances poisoned");

        // 2–4) Per-track processing in topo order over the routing
        // graph (children before folder parents, senders before their
        // destinations): each track's bus is gained/panned, its sends
        // dispatched, and the result summed into its folder parent's
        // bus *before* the parent's own gain applies — REAPER's folder
        // routing (the parent fader/FX scale the children's sum plus
        // the parent's own items). Top-level parent-send tracks sum to
        // master.
        let inv_rate = 1.0 / self.sample_rate as f64;
        for &ti in &snap.order {
            let t = &tracks[ti];
            if !passes(ti) {
                // Solo-skipped tracks meter as silence.
                if let Some(cell) = meters.cell(ti) {
                    cell.write(0.0, 0.0, crate::metering::HOLD_DECAY);
                }
                continue;
            }
            // Clean bus = all zeros: gain scales nothing, sends add
            // nothing, parent sums add nothing. Skip the per-frame
            // work entirely; only active tracks cost CPU. (A track
            // with FX must still run — synths generate from MIDI.)
            if !dirty[ti] && t.fx_chain.is_empty() {
                if let Some(cell) = meters.cell(ti) {
                    cell.write(0.0, 0.0, crate::metering::HOLD_DECAY);
                }
                continue;
            }

            // Pre-FX send tap: the bus BEFORE the FX chain runs. At
            // this point in topo order the bus already holds items +
            // children sums + received sends — the track's input.
            let has_pre_fx_tap = t
                .sends
                .iter()
                .any(|s| s.mode == daw_proto::routing::SendMode::PreFx);
            if has_pre_fx_tap {
                pre_fx_tap.clear();
                pre_fx_tap.extend_from_slice(&buses[ti].samples);
            }

            // FX chain: pipe the bus through every loaded plugin in
            // order. Running INSIDE the topo loop means a folder's FX
            // process the children's mix, and bus FX process their
            // received sends — REAPER's signal flow. Synthetic /
            // unloaded FX are skipped — no DSP, no work.
            if !t.fx_chain.is_empty() {
                let bus = &mut buses[ti];
                // MIDI + note-expression events for this block. Both
                // lists go to every plugin in the chain (REAPER's
                // default) — non-MIDI plugins ignore them.
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
                        // Lazy prepare on first use; bypass on failure.
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
                    if plugin
                        .process_block(in_l, in_r, out_l, out_r, &events)
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
            if !dirty[ti] {
                if let Some(cell) = meters.cell(ti) {
                    cell.write(0.0, 0.0, crate::metering::HOLD_DECAY);
                }
                continue;
            }

            // Pre-fader send tap: post-FX, before the gain stage.
            let has_pre_fader_tap = t
                .sends
                .iter()
                .any(|s| s.mode == daw_proto::routing::SendMode::PostFx);
            if has_pre_fader_tap {
                pre_fader_tap.clear();
                pre_fader_tap.extend_from_slice(&buses[ti].samples);
            }

            // Gain + pan + polarity, per-frame envelopes.
            // VCA grouping (user guide §5.16): a follower's volume is
            // dB-added (= linear-multiplied) with every shared-group
            // lead's fader + volume envelope; the lead's pan (+ pan
            // envelope) offsets the follower's pan; a MUTE ENVELOPE on
            // the lead mutes followers (the lead's mute button does
            // NOT — mute isn't a VCA parameter). Follower faders never
            // move; this is playback-only.
            let vca_leads: Vec<&TrackSnapshot> = if t.vca_follow != 0 {
                tracks
                    .iter()
                    .filter(|l| l.vca_lead & t.vca_follow != 0)
                    .collect()
            } else {
                Vec::new()
            };
            let mut vca_cursors: Vec<(
                Option<EnvelopeCursor>,
                Option<EnvelopeCursor>,
                Option<EnvelopeCursor>,
            )> = vca_leads
                .iter()
                .map(|l| {
                    (
                        l.volume_env.as_ref().map(|p| EnvelopeCursor::new(p)),
                        l.pan_env.as_ref().map(|p| EnvelopeCursor::new(p)),
                        l.mute_env.as_ref().map(|p| EnvelopeCursor::new(p)),
                    )
                })
                .collect();
            {
                let bus = &mut buses[ti];
                let mut c_vol = t.volume_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_pvol = t.volume_prefx_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_pan = t.pan_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_ppan = t.pan_prefx_env.as_ref().map(|p| EnvelopeCursor::new(p));
                let mut c_mute = t.mute_env.as_ref().map(|p| EnvelopeCursor::new(p));
                for frame in 0..bus.frames {
                    let time = start_seconds + frame as f64 * inv_rate;
                    let v_main = c_vol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                    let v_prefx = c_pvol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                    let mut vol = t.volume * v_main * v_prefx;
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
                    let mut pan = t.pan + p_main + p_prefx;
                    // VCA leads ride this follower: gain multiplies,
                    // pan offsets, lead mute envelopes gate.
                    let mut vca_env_muted = false;
                    for (li, lead) in vca_leads.iter().enumerate() {
                        let (c_lv, c_lp, c_lm) = &mut vca_cursors[li];
                        let lv = c_lv.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                        vol *= lead.volume * lv;
                        let lp = c_lp
                            .as_mut()
                            .and_then(|c| c.eval_at(time))
                            .map(|v| (v - 0.5) * 2.0)
                            .unwrap_or(0.0);
                        pan += lead.pan + lp;
                        if c_lm
                            .as_mut()
                            .and_then(|c| c.eval_at(time))
                            .map(|v| v > 0.5)
                            .unwrap_or(false)
                        {
                            vca_env_muted = true;
                        }
                    }
                    let pan = pan.clamp(-1.0, 1.0);
                    let env_muted = c_mute
                        .as_mut()
                        .and_then(|c| c.eval_at(time))
                        .map(|v| v > 0.5)
                        .unwrap_or(false);
                    let muted = t.muted || env_muted || vca_env_muted;
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

            // Sends — additive into destination buses, per-frame. (Copy the
            // post-fader source once so we can mutably borrow dest buses.)
            src.clear();
            src.extend_from_slice(&buses[ti].samples);

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
                        if t.parent_idx.is_some() {
                            "ok"
                        } else if t.has_parent_guid {
                            "MISS"
                        } else {
                            "none"
                        },
                        t.hw_out,
                        t.sends.len()
                    );
                }
            }
            for snd in &t.sends {
                if snd.muted {
                    continue;
                }
                let Some(di) = snd.dest_idx else { continue };
                // Pick the tap matching the send's chain position.
                let send_src: &[f32] = match snd.mode {
                    daw_proto::routing::SendMode::PreFx if has_pre_fx_tap => pre_fx_tap,
                    daw_proto::routing::SendMode::PostFx if has_pre_fader_tap => pre_fader_tap,
                    _ => src,
                };
                let dest_bus = &mut buses[di];
                dirty[di] = true;
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
                    dest_bus.samples[frame * 2] += send_src[frame * 2] * lg;
                    dest_bus.samples[frame * 2 + 1] += send_src[frame * 2 + 1] * rg;
                }
            }

            // Parent / master sum.
            if t.parent_send {
                match t.parent_idx {
                    Some(pi) => {
                        let parent_bus = &mut buses[pi];
                        for (m, smp) in parent_bus.samples.iter_mut().zip(src.iter()) {
                            *m += *smp;
                        }
                        dirty[pi] = true;
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
                    let t = start_seconds + frame as f64 * inv_rate - t0;
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

        master
    }

    /// Revision-cached [`RenderSnapshot`] — rebuilt only when the
    /// project's mutation revision moves.
    fn snapshot(&self) -> Option<Arc<RenderSnapshot>> {
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
            (p.revision, RenderSnapshot::build(p))
        })?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((revision, snap.clone()));
        }
        Some(snap)
    }
}
