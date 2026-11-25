//! FX automation, end to end on the renderer: a CLAP gain plugin (built into
//! this test binary) automated by an envelope, its output read frame by frame.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use daw_proto::automation::EnvelopePoint;
use daw_proto::fx::{Effects, FxChainContext};
use daw_proto::primitives::PositionInSeconds;
use daw_proto::{ProjectContext, ProjectInfo};

use super::envelope::EnvelopeCursor;
use super::folder_routing_tests::guid_of;
use super::offline::{Copying, Offline, copy_plugins};
use super::*;
use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::materialize::attach_audio_source;
use crate::media_seed::{StemSpec, seed_media_tracks};
use crate::sync::{EnvelopeData, EnvelopeKey};

const RATE: u32 = 48_000;
/// The gain parameter's range, dB; an envelope's 0..1 spans it.
const FLOOR_DB: f64 = -24.0;
const CEIL_DB: f64 = 6.0;

// ── The plugin ───────────────────────────────────────────────────────

/// A stereo gain whose one parameter (id 0) is dB, applied from the frame
/// each change lands on. It also reports, as read-only parameters, the most
/// gain events it was sent in one block (id 1) and how many blocks it ran
/// (id 2).
struct TestGain;

struct Shared {
    /// The gain, dB, as `f64` bits.
    gain: std::sync::atomic::AtomicU64,
    most_events: AtomicU32,
    blocks: AtomicU32,
}

impl Shared {
    fn gain(&self) -> f64 {
        f64::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Take a gain event; whether it was one.
    fn take(&self, event: &UnknownEvent) -> bool {
        match event.as_core_event() {
            Some(CoreEventSpace::ParamValue(e)) if ClapId::new(0) == e.param_id() => {
                self.gain.store(e.value().to_bits(), Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }
}

impl PluginShared<'_> for Shared {}

struct MainThread<'a> {
    shared: &'a Shared,
}

impl<'a> PluginMainThread<'a, Shared> for MainThread<'a> {}

struct Processor<'a> {
    shared: &'a Shared,
}

impl Plugin for TestGain {
    type AudioProcessor<'a> = Processor<'a>;
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _: Option<&Shared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>();
    }
}

impl DefaultPluginFactory for TestGain {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("com.fasttrackstudio.test.gain", "Test Gain")
    }

    fn new_shared(_: HostSharedHandle<'_>) -> Result<Shared, PluginError> {
        Ok(Shared {
            gain: std::sync::atomic::AtomicU64::new(0f64.to_bits()),
            most_events: AtomicU32::new(0),
            blocks: AtomicU32::new(0),
        })
    }

    fn new_main_thread<'a>(
        _: HostMainThreadHandle<'a>,
        shared: &'a Shared,
    ) -> Result<MainThread<'a>, PluginError> {
        Ok(MainThread { shared })
    }
}

impl<'a> PluginAudioProcessor<'a, Shared, MainThread<'a>> for Processor<'a> {
    fn activate(
        _: HostAudioProcessorHandle<'a>,
        _: &mut MainThread,
        shared: &'a Shared,
        _: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self { shared })
    }

    fn process(
        &mut self,
        _: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let frames = audio.frames_count() as usize;
        let mut pair = audio.port_pair(0).ok_or(PluginError::Message("no ports"))?;
        let mut channels = pair
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("not f32"))?;
        let mut taken = 0;
        let mut buffers: [Option<&mut [f32]>; 2] = [None, None];
        for (pair, slot) in channels.iter_mut().zip(&mut buffers) {
            *slot = match pair {
                ChannelPair::InPlace(b) => Some(b),
                ChannelPair::InputOutput(i, o) => {
                    o.copy_from_slice(i);
                    Some(o)
                }
                _ => None,
            };
        }
        for batch in events.input.batch() {
            for e in batch.events() {
                taken += u32::from(self.shared.take(e));
            }
            let from = batch.first_sample();
            let to = batch
                .next_batch_first_sample()
                .unwrap_or(frames)
                .min(frames);
            let g = 10f64.powf(self.shared.gain() / 20.0) as f32;
            for b in buffers.iter_mut().flatten() {
                for s in &mut b[from..to] {
                    *s *= g;
                }
            }
        }
        self.shared.most_events.fetch_max(taken, Ordering::Relaxed);
        self.shared.blocks.fetch_add(1, Ordering::Relaxed);
        Ok(ProcessStatus::Continue)
    }
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&mut self, _: bool) -> u32 {
        1
    }

    fn get(&mut self, index: u32, _: bool, writer: &mut AudioPortInfoWriter) {
        if index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: b"main",
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginMainThreadParams for MainThread<'_> {
    fn count(&mut self) -> u32 {
        3
    }

    fn get_info(&mut self, index: u32, info: &mut ParamInfoWriter) {
        let (name, min, max, flags): (&[u8], f64, f64, ParamInfoFlags) = match index {
            0 => (b"Gain", FLOOR_DB, CEIL_DB, ParamInfoFlags::IS_AUTOMATABLE),
            1 => (b"Most events", 0.0, 1e6, ParamInfoFlags::IS_READONLY),
            2 => (b"Blocks", 0.0, 1e9, ParamInfoFlags::IS_READONLY),
            _ => return,
        };
        info.set(&ParamInfo {
            id: index.into(),
            flags,
            cookie: Default::default(),
            name,
            module: b"",
            min_value: min,
            max_value: max,
            default_value: 0.0,
        });
    }

    fn get_value(&mut self, id: ClapId) -> Option<f64> {
        match id.get() {
            0 => Some(self.shared.gain()),
            1 => Some(f64::from(self.shared.most_events.load(Ordering::Relaxed))),
            2 => Some(f64::from(self.shared.blocks.load(Ordering::Relaxed))),
            _ => None,
        }
    }

    fn value_to_text(
        &mut self,
        _: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        use std::fmt::Write as _;
        write!(writer, "{value:.2}")
    }

    fn text_to_value(&mut self, _: ClapId, text: &std::ffi::CStr) -> Option<f64> {
        text.to_str().ok()?.trim().parse().ok()
    }

    fn flush(&mut self, input: &InputEvents, _: &mut OutputEvents) {
        for e in input {
            self.shared.take(e);
        }
    }
}

impl PluginAudioProcessorParams for Processor<'_> {
    fn flush(&mut self, input: &InputEvents, _: &mut OutputEvents) {
        for e in input {
            self.shared.take(e);
        }
    }
}

/// The plugin's entry, loaded once for the test binary.
fn entry() -> &'static clack_host::prelude::PluginEntry {
    static ENTRY: OnceLock<clack_host::prelude::PluginEntry> = OnceLock::new();
    ENTRY.get_or_init(|| {
        clack_host::prelude::PluginEntry::load_from_clack::<SinglePluginEntry<TestGain>>(
            c"/test/test-gain.clap",
        )
        .expect("the test gain's entry")
    })
}

fn load_gain() -> crate::audio_engine::plugin_host::SendableClapPlugin {
    crate::audio_engine::plugin_host::ClapHost::new("daw tests", "FastTrackStudio")
        .load_entry(entry(), 0)
        .expect("the test gain loads")
        .into_send()
}

/// The CLAP gain as the renderer sees it, its timed events hidden: the
/// renderer then sends its automation at each block's start, as it did for
/// every plugin before.
struct Stepped(crate::audio_engine::plugin_host::SendableClapPlugin);

impl crate::plugin::PluginInstance for Stepped {
    fn descriptor(&self) -> crate::plugin::PluginDescriptor {
        crate::plugin::PluginInstance::descriptor(&self.0)
    }
    fn params(&mut self) -> Vec<crate::plugin::PluginParamInfo> {
        crate::plugin::PluginInstance::params(&mut self.0)
    }
    fn param_value(&mut self, id: u32) -> Option<f64> {
        crate::plugin::PluginInstance::param_value(&mut self.0, id)
    }
    fn value_to_text(&mut self, id: u32, v: f64) -> Option<String> {
        crate::plugin::PluginInstance::value_to_text(&mut self.0, id, v)
    }
    fn text_to_value(&mut self, id: u32, t: &str) -> Option<f64> {
        crate::plugin::PluginInstance::text_to_value(&mut self.0, id, t)
    }
    fn latency(&mut self) -> u32 {
        0
    }
    fn prepare(&mut self, sr: f64, bs: u32) -> Result<(), crate::plugin::PluginError> {
        crate::plugin::PluginInstance::prepare(&mut self.0, sr, bs)
    }
    fn is_prepared(&self) -> bool {
        crate::plugin::PluginInstance::is_prepared(&self.0)
    }
    fn process_block(
        &mut self,
        il: &[f32],
        ir: &[f32],
        ol: &mut [f32],
        or: &mut [f32],
        ev: &crate::plugin::PluginEvents<'_>,
    ) -> Result<(), crate::plugin::PluginError> {
        crate::plugin::PluginInstance::process_block(&mut self.0, il, ir, ol, or, ev)
    }
    fn deactivate(&mut self) {
        crate::plugin::PluginInstance::deactivate(&mut self.0);
    }
}

/// Makes "FTS Test Gain" (timed events) and "FTS Test Stepped Gain"
/// (block-start values): names no plugin on disk has.
struct Gains;

impl crate::plugin::FxFactory for Gains {
    fn installed(&self) -> Vec<daw_proto::fx::InstalledFx> {
        Vec::new()
    }
    fn provides(&self, name: &str) -> bool {
        matches!(name, "FTS Test Gain" | "FTS Test Stepped Gain")
    }
    fn create(&self, name: &str, _: f64) -> Option<Box<dyn crate::plugin::PluginInstance>> {
        match name {
            "FTS Test Gain" => Some(Box::new(load_gain())),
            "FTS Test Stepped Gain" => Some(Box::new(Stepped(load_gain()))),
            _ => None,
        }
    }
}

// ── The song ─────────────────────────────────────────────────────────

/// One track playing a constant 0.5 for `seconds`, through `fx`, its gain
/// automated by `points` (0..1 along -24..+6 dB). The project and the FX.
fn song(fx: &str, points: Vec<EnvelopePoint>, seconds: f64) -> (Standalone, String, String) {
    let daw = Standalone::new();
    daw.set_fx_factory(Arc::new(Gains));
    let guid = daw.seed_project(ProjectInfo {
        guid: format!("automation-{fx}"),
        name: "automation".into(),
        path: String::new(),
    });
    seed_media_tracks(
        &daw,
        &guid,
        &[StemSpec::new("Vox", "/x/vox.wav", None)],
        0.0,
        seconds,
    );
    let takes: Vec<String> = daw
        .read_project(&guid, |p| {
            p.takes
                .values()
                .flat_map(|tl| tl.takes.iter().map(|t| t.guid.clone()))
                .collect()
        })
        .unwrap_or_default();
    let frames = (seconds * f64::from(RATE)) as usize + RATE as usize;
    for take in takes {
        attach_audio_source(
            &daw,
            &guid,
            &take,
            DecodedAudio::new(vec![0.5; frames], 1, RATE),
        );
    }
    let track = guid_of(&daw, &guid, "Vox");
    let fx_guid = Effects::add(
        &daw,
        ProjectContext::Project(guid.clone()),
        FxChainContext::Track(track.clone()),
        fx,
    )
    .expect("the gain is added");
    if !points.is_empty() {
        let mut data = EnvelopeData::new();
        data.points = points;
        daw.write_project(&guid, |p| {
            p.envelopes.insert(
                (
                    track,
                    EnvelopeKey::FxParam {
                        fx_guid: fx_guid.clone(),
                        param_index: 0,
                    },
                ),
                data,
            );
        });
    }
    (daw, guid, fx_guid)
}

fn point(seconds: f64, db: f64) -> EnvelopePoint {
    EnvelopePoint {
        time: PositionInSeconds::from_seconds(seconds),
        value: (db - FLOOR_DB) / (CEIL_DB - FLOOR_DB),
        ..EnvelopePoint::default()
    }
}

/// Held at -12 dB, a ramp up 6 dB in 50 ms (2.5 times as fast as the
/// vocal leveler's fastest ride), held, then a jump down 18 dB.
fn ride() -> Vec<EnvelopePoint> {
    vec![
        point(0.0, -12.0),
        point(0.0503, -12.0),
        point(0.1003, -6.0),
        point(0.1500, -6.0),
        point(0.1500, -24.0),
    ]
}

/// The envelope's gain at each frame from 0, dB.
fn expected_db(points: &[EnvelopePoint], frames: usize) -> Vec<f64> {
    let mut cursor = EnvelopeCursor::new(points);
    (0..frames)
        .map(|f| {
            let v = cursor.eval_at(f as f64 / f64::from(RATE)).unwrap_or(0.0);
            (CEIL_DB - FLOOR_DB).mul_add(v.clamp(0.0, 1.0), FLOOR_DB)
        })
        .collect()
}

/// `blocks` blocks of `block` frames from frame 0: master's left channel.
fn render(renderer: &ProjectRenderer, block: usize, blocks: usize) -> Vec<f32> {
    (0..blocks)
        .flat_map(|b| {
            let out = renderer.render_block((b * block) as u64, block);
            out.samples
                .chunks_exact(2)
                .map(|f| f[0])
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Master with the gain at 0 dB everywhere: what the automation scales.
fn unity(frames: usize) -> Vec<f32> {
    let (daw, guid, _) = song("FTS Test Gain", Vec::new(), 1.0);
    let renderer = ProjectRenderer::new(&daw, &guid, RATE);
    render(&renderer, 1024, frames.div_ceil(1024))
}

/// How far `out` is from `want` (dB) at each frame, against `unity`.
fn errors_db(out: &[f32], unity: &[f32], want: &[f64]) -> Vec<f64> {
    out.iter()
        .zip(unity)
        .zip(want)
        .map(|((&o, &u), &w)| (20.0 * (f64::from(o) / f64::from(u)).log10() - w).abs())
        .collect()
}

fn worst(errors: &[f64]) -> f64 {
    errors.iter().copied().fold(0.0, f64::max)
}

// ── The tests ────────────────────────────────────────────────────────

const FRAMES: usize = 1024 * 9;

#[test]
fn a_clap_parameter_follows_its_envelope_within_a_twentieth_of_a_db_at_every_frame() {
    let unity = unity(FRAMES);
    let want = expected_db(&ride(), FRAMES);
    let (daw, guid, _) = song("FTS Test Gain", ride(), 1.0);
    let renderer = ProjectRenderer::new(&daw, &guid, RATE);
    let out = render(&renderer, 1024, FRAMES / 1024);
    let errors = errors_db(&out, &unity, &want);
    assert!(
        worst(&errors) < 0.05,
        "worst {:.4} dB at frame {:?}",
        worst(&errors),
        errors.iter().position(|&e| e == worst(&errors))
    );
    // The jump lands on its frame.
    let jump = (0.15 * f64::from(RATE)).ceil() as usize;
    assert!(errors[jump] < 1e-3 && errors[jump - 1] < 0.05);
}

#[test]
fn played_at_each_blocks_start_the_same_envelope_is_a_staircase() {
    let unity = unity(FRAMES);
    let want = expected_db(&ride(), FRAMES);
    let (daw, guid, _) = song("FTS Test Stepped Gain", ride(), 1.0);
    let renderer = ProjectRenderer::new(&daw, &guid, RATE);
    let out = render(&renderer, 1024, FRAMES / 1024);
    let worst = worst(&errors_db(&out, &unity, &want));
    assert!(worst > 1.0, "a block-stepped ride is off by {worst:.3} dB");
}

#[test]
fn a_flat_lane_sends_at_most_one_event_a_block() {
    let flat = vec![point(0.0, -6.0), point(1.0, -6.0)];
    let (daw, guid, fx) = song("FTS Test Gain", flat, 1.0);
    let renderer = ProjectRenderer::new(&daw, &guid, RATE);
    render(&renderer, 1024, 20);
    let most = daw
        .with_plugin_instance(&fx, |p| p.param_value(1))
        .flatten();
    let blocks = daw
        .with_plugin_instance(&fx, |p| p.param_value(2))
        .flatten();
    assert_eq!(most, Some(1.0), "the most gain events in a block");
    assert!(blocks >= Some(20.0), "{blocks:?} blocks");
}

#[test]
fn an_offline_render_plays_automation_as_the_live_one_does() {
    let (daw, guid, _) = song("FTS Test Gain", ride(), 1.0);
    let live = render(
        &ProjectRenderer::new(&daw, &guid, RATE),
        1024,
        FRAMES / 1024,
    );
    let (plugins, copies) = copy_plugins(&daw, &guid, Copying::default());
    assert_eq!(copies.copied, 1, "{:?}", copies.failed);
    let renderer = ProjectRenderer::offline(
        &daw,
        &guid,
        RATE,
        Offline {
            plugins,
            ..Offline::default()
        },
    );
    renderer.prime(256);
    // Another block size: where the events land must not depend on it.
    let offline = render(&renderer, 256, FRAMES / 256);
    let differ = live
        .iter()
        .zip(&offline)
        .position(|(a, b)| (a - b).abs() > 1e-6);
    assert_eq!(differ, None, "the first frame they differ");
}

/// Allocations the render thread makes over `blocks` blocks of 1024, after
/// as many to warm up: the output buffer kept, as the audio thread keeps it.
fn render_allocations(renderer: &ProjectRenderer, blocks: u64) -> u64 {
    let mut out = StereoBuffer::zeroed(0, RATE);
    let block = |b: u64, out: &mut StereoBuffer| {
        renderer.render_part_into((b * 1024) as f64, 1024, 1.0, part::Part::Whole, out);
    };
    for b in 0..blocks {
        block(b, &mut out);
    }
    let ((), allocations) = crate::alloc_count::allocations(|| {
        for b in blocks..2 * blocks {
            block(b, &mut out);
        }
    });
    allocations
}

#[test]
fn playing_automation_as_timed_events_allocates_nothing_the_render_did_not() {
    let (daw, guid, _) = song("FTS Test Gain", Vec::new(), 1.0);
    let plain = render_allocations(&ProjectRenderer::new(&daw, &guid, RATE), 20);
    let (daw, guid, _) = song("FTS Test Gain", ride(), 1.0);
    let automated = render_allocations(&ProjectRenderer::new(&daw, &guid, RATE), 20);
    assert_eq!(
        automated, plain,
        "allocations over 20 blocks, automated vs not"
    );
    assert_eq!(automated, 0, "the render thread allocates nothing");
}

/// A host's value reaches a plugin that is not processing (the transport
/// stopped): set through the FX's parameters, the plugin itself reports
/// it at once — never activated, and activated with no block run since —
/// not only with the next block a render plays. What a plugin reports is
/// what its state saves and a render's copy is made from.
#[test]
fn a_set_parameter_reaches_a_plugin_that_is_not_processing() {
    let (daw, guid, fx_guid) = song("FTS Test Gain", Vec::new(), 1.0);
    let track = guid_of(&daw, &guid, "Vox");
    let ctx = FxChainContext::Track(track);
    let norm = |db: f64| (db - FLOOR_DB) / (CEIL_DB - FLOOR_DB);
    let reported = || {
        daw.with_plugin_instance(&fx_guid, |p| p.param_value(0))
            .flatten()
            .expect("the gain reports its value")
    };
    // Never activated.
    daw_proto::FxParams::set(&daw, ctx.clone(), 0, 0, norm(-12.0)).expect("set");
    assert!(
        (reported() - -12.0).abs() < 1e-9,
        "inactive: {}",
        reported()
    );
    // Activated, no block since.
    daw.with_plugin_instance(&fx_guid, |p| p.prepare(f64::from(RATE), 512))
        .expect("an instance")
        .expect("activates");
    daw_proto::FxParams::set(&daw, ctx, 0, 0, norm(3.0)).expect("set");
    assert!((reported() - 3.0).abs() < 1e-9, "activated: {}", reported());
}
