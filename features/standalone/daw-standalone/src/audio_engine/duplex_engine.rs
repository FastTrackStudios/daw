//! Native PipeWire **duplex** engine — drives the project renderer from one
//! realtime callback (input + output, same cycle, no ring bridge between
//! separate streams). This is the low-latency path real DAWs use; it replaces
//! the cpal output-stream + separate input-stream + lock-free ring of
//! [`AudioEngine`](super::AudioEngine) for live-input work.
//!
//! The callback pushes the block's hardware input into the renderer's live-input
//! ring and immediately calls [`ProjectRenderer::render_block`], which drains it
//! the same block — so the ring carries exactly one block and adds **zero**
//! latency (it exists only because the renderer's input path is ring-shaped).
//! The renderer — FX chains, plugins, routing, metering — is reused unchanged.

use std::sync::Arc;
use std::time::Duration;

use daw_audio_io::duplex::{Backend, DuplexBackend, DuplexConfig, EngineStats, ProcessBlock};
use daw_audio_io::{AudioIoPrefs, pw};

use crate::Standalone;
use crate::audio_engine::render::ProjectRenderer;
use crate::transport_engine::TransportShared;

/// The duplex node name (how it appears in the PipeWire graph / patchbays).
const NODE_NAME: &str = "FTS-Signal";

/// A live audio engine driven by a native PipeWire duplex `pw_filter`.
/// Dropping it stops audio (the backend tears down its node + thread loop).
pub struct DuplexAudioEngine {
    shared: Arc<TransportShared>,
    _backend: Backend,
    _renderer: Arc<ProjectRenderer>,
    stats: Arc<EngineStats>,
    sample_rate: u32,
}

/// Live phones-bus levels — a process-wide handle the host app writes
/// (headphone volume + self-mix) and the duplex callback reads lock-free.
pub struct PhonesBus {
    volume: std::sync::atomic::AtomicU32,
    self_mix: std::sync::atomic::AtomicU32,
}

impl PhonesBus {
    /// The process-wide bus.
    pub fn shared() -> &'static PhonesBus {
        static BUS: PhonesBus = PhonesBus {
            volume: std::sync::atomic::AtomicU32::new(0x3f800000),   // 1.0
            self_mix: std::sync::atomic::AtomicU32::new(0x3f800000), // 1.0
        };
        &BUS
    }

    pub fn set(&self, volume: f32, self_mix: f32) {
        use std::sync::atomic::Ordering::Relaxed;
        self.volume.store(volume.to_bits(), Relaxed);
        self.self_mix.store(self_mix.to_bits(), Relaxed);
    }

    fn levels(&self) -> (f32, f32) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            f32::from_bits(self.volume.load(Relaxed)),
            f32::from_bits(self.self_mix.load(Relaxed)),
        )
    }
}

impl DuplexAudioEngine {
    /// Build a duplex engine for `project_guid` on `daw`, sharing `shared`'s
    /// transport clock. Opens one input port per hardware channel up to the
    /// highest a track records from, plus stereo output, and wires the duplex
    /// node to the prefs' input/output devices.
    pub fn with_project_prefs(
        daw: Standalone,
        project_guid: String,
        shared: Arc<TransportShared>,
        prefs: &AudioIoPrefs,
    ) -> Result<Self, String> {
        let sample_rate = prefs.sample_rate_opt().unwrap_or(48_000);
        let buffer = prefs.buffer_size_opt().unwrap_or(128);
        shared.set_sample_rate(sample_rate);

        let renderer = Arc::new(ProjectRenderer::new(&daw, &project_guid, sample_rate));

        // Live / programmatic MIDI ring (same wiring as the cpal engine).
        {
            let (prod, cons) =
                rtrb::RingBuffer::<crate::audio_engine::render::LiveMidiEvent>::new(4096);
            renderer.set_live_midi(cons);
            daw.set_live_midi_producer(prod);
        }

        // Open enough input channels to reach the highest a track taps.
        let in_channels = max_armed_channel(&daw, &project_guid)
            .map(|c| c + 1)
            .unwrap_or(1)
            .max(1);

        // Live-input ring: producer fed by the duplex callback, consumer drained
        // by the renderer in the SAME callback. Modest capacity (a few blocks) is
        // plenty since it never accumulates.
        let capacity = (in_channels * buffer as usize * 4).max(8192);
        let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(capacity);
        renderer.set_live_input(cons, in_channels);

        let r = renderer.clone();
        let sh = shared.clone();
        // Output routing: master lands on `main_out`; an optional phones bus
        // (master × self-mix + an external monitor-mix input pair, × volume)
        // lands on `phones_out`. Legacy prefs (routing off) stay 2-out 0/1.
        let routing = prefs.phones_routing;
        let (main_l, main_r) = if routing {
            (prefs.main_out_l, prefs.main_out_r)
        } else {
            (0, 1)
        };
        let (ph_l, ph_r) = (prefs.phones_out_l, prefs.phones_out_r);
        let (mix_l, mix_r) = (prefs.phones_mix_in_l, prefs.phones_mix_in_r);
        let phones = PhonesBus::shared();
        let ph = phones.clone();
        let out_ports = if routing {
            main_l.max(main_r).max(ph_l).max(ph_r) + 1
        } else {
            2
        };
        let in_ports = if routing {
            in_channels.max(mix_l.max(mix_r) + 1)
        } else {
            in_channels
        };
        let cfg = DuplexConfig {
            name: NODE_NAME.into(),
            inputs: in_ports,
            outputs: out_ports,
            latency: Some((buffer, sample_rate)),
        };
        let backend = Backend::start(
            cfg,
            Box::new(move |b: &mut ProcessBlock| {
                let frames = b.frames;
                // Push this block's hardware input, interleaved [f0c0,f0c1,…], so
                // the renderer's stage-0 de-interleave matches the cpal path.
                for f in 0..frames {
                    for c in 0..in_channels {
                        let s = b.inputs.get(c).map(|ch| ch[f]).unwrap_or(0.0);
                        let _ = prod.push(s);
                    }
                }
                // Render this block (drains exactly what we just pushed).
                let start = sh.playhead_samples().0.max(0) as u64;
                let block = r.render_block(start, frames);
                let outs = b.outputs.len();
                let (vol, self_mix) = ph.levels();
                for f in 0..frames {
                    let l = block.samples.get(f * 2).copied().unwrap_or(0.0);
                    let rr = block.samples.get(f * 2 + 1).copied().unwrap_or(0.0);
                    if main_l < outs {
                        b.outputs[main_l][f] = l;
                    }
                    if main_r < outs {
                        b.outputs[main_r][f] = rr;
                    }
                    if routing && (ph_l != main_l || ph_r != main_r) {
                        let ext_l = b.inputs.get(mix_l).map(|ch| ch[f]).unwrap_or(0.0);
                        let ext_r = b.inputs.get(mix_r).map(|ch| ch[f]).unwrap_or(0.0);
                        if ph_l < outs {
                            b.outputs[ph_l][f] = (l * self_mix + ext_l) * vol;
                        }
                        if ph_r < outs {
                            b.outputs[ph_r][f] = (rr * self_mix + ext_r) * vol;
                        }
                    }
                }
                sh.advance(frames as u32);
            }),
        )?;
        let stats = backend.stats();

        // Wire the duplex node to the hardware (a DSP filter isn't auto-linked).
        // Async + retried: the node's ports take a beat to appear in the graph.
        spawn_linker(prefs.clone(), in_channels);

        Ok(Self {
            shared,
            _backend: backend,
            _renderer: renderer,
            stats,
            sample_rate,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    /// Live realtime metrics (render time, block size, xruns) — the source of the
    /// rig's DSP-load + xrun meters.
    pub fn stats(&self) -> Arc<EngineStats> {
        self.stats.clone()
    }
    pub fn shared(&self) -> &Arc<TransportShared> {
        &self.shared
    }
}

/// Highest hardware input channel any track records from, or `None`.
fn max_armed_channel(daw: &Standalone, project_guid: &str) -> Option<usize> {
    daw.read_project(project_guid, |p| {
        p.tracks
            .iter()
            .filter_map(|t| match p.track_ext.get(&t.guid).map(|e| e.record_input) {
                Some(daw_proto::track::RecordInput::Audio { channel }) => Some(channel as usize),
                _ => None,
            })
            .max()
    })
    .flatten()
}

/// Link the hardware device's capture/playback ports to the duplex node, on a
/// background thread that retries until the node's ports appear (best-effort).
fn spawn_linker(prefs: AudioIoPrefs, in_channels: usize) {
    let input = prefs.input_name().map(str::to_string);
    // Default output to the input interface (a guitar rig monitors through the
    // same device) when no explicit output device is set.
    let output = prefs
        .output_name()
        .or(prefs.input_name())
        .map(str::to_string);
    std::thread::spawn(move || {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            let mut any = false;
            if let Some(dev) = input.as_deref().and_then(|n| pw::device_node_name(n, true)) {
                for c in 0..in_channels {
                    any |= pw::link(
                        &format!("{dev}:capture_{}", c + 1),
                        &format!("{NODE_NAME}:input_{c}"),
                    );
                }
            }
            if let Some(dev) = output
                .as_deref()
                .and_then(|n| pw::device_node_name(n, false))
            {
                any |= pw::link(
                    &format!("{NODE_NAME}:output_0"),
                    &format!("{dev}:playback_1"),
                );
                any |= pw::link(
                    &format!("{NODE_NAME}:output_1"),
                    &format!("{dev}:playback_2"),
                );
            }
            if any {
                break; // linked something — done
            }
        }
    });
}
