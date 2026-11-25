//! Plugins asleep while they hear silence ([`crate::plugin_sleep`]), end to
//! end on the renderer: a CLAP echo built into this test binary, on a track
//! whose one item plays only the first quarter second of four, rendered
//! asleep and always processing and nulled against each other.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::tail::{PluginTail, PluginTailImpl, TailLength};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use daw_proto::fx::{Effects, FxChainContext};
use daw_proto::{MidiNote, ProjectContext, ProjectInfo, TrackRef};

use super::folder_routing_tests::guid_of;
use super::offline::{Copying, Offline, copy_plugins};
use super::*;
use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::materialize::attach_audio_source;
use crate::media_seed::{StemSpec, seed_media_tracks};
use crate::plugin_sleep::SILENCE;

const RATE: u32 = 48_000;
const BLOCK: usize = 1024;
/// The echo's spacing and feedback: 25 repeats take a full line under
/// -150 dBFS.
const ECHO: usize = 4_800;
const FEEDBACK: f32 = 0.5;
/// Its tail: 26 repeats, and a note's burst.
const TAIL: u32 = 26 * ECHO as u32 + 20_000;
/// A note's burst: 0.5, decaying by this each frame (under -150 dBFS in
/// about 16 600 frames).
const BURST_DECAY: f32 = 0.999;

// ── The plugin ───────────────────────────────────────────────────────

/// A stereo echo (one line, both channels the same) whose status is its
/// mode: 0 `Tail` (with `tail`), 1 `Sleep` once its line and burst are
/// under -150 dBFS, 2 `Continue`. A note adds a decaying burst; parameter
/// 1 a constant tone; parameter 2 counts the blocks it ran.
struct TestEcho;

struct Shared {
    mode: AtomicU32,
    /// The tone, as `f64` bits.
    tone: AtomicU64,
    blocks: AtomicU32,
}

impl Shared {
    fn tone(&self) -> f32 {
        f64::from_bits(self.tone.load(Ordering::Relaxed)) as f32
    }

    fn take(&self, event: &UnknownEvent) {
        if let Some(CoreEventSpace::ParamValue(e)) = event.as_core_event() {
            match e.param_id().map(ClapId::get) {
                Some(0) => self.mode.store(e.value() as u32, Ordering::Relaxed),
                Some(1) => self.tone.store(e.value().to_bits(), Ordering::Relaxed),
                _ => {}
            }
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
    line: Vec<f32>,
    pos: usize,
    burst: f32,
}

impl Plugin for TestEcho {
    type AudioProcessor<'a> = Processor<'a>;
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _: Option<&Shared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>()
            .register::<PluginTail>();
    }
}

impl DefaultPluginFactory for TestEcho {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("com.fasttrackstudio.test.echo", "Test Echo")
    }

    fn new_shared(_: HostSharedHandle<'_>) -> Result<Shared, PluginError> {
        Ok(Shared {
            mode: AtomicU32::new(2),
            tone: AtomicU64::new(0f64.to_bits()),
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
        Ok(Self {
            shared,
            line: vec![0.0; ECHO],
            pos: 0,
            burst: 0.0,
        })
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
        let mut ins = [[0.0f32; 8192]; 2];
        let mut bufs: [Option<&mut [f32]>; 2] = [None, None];
        for (c, (pair, slot)) in channels.iter_mut().zip(&mut bufs).enumerate() {
            *slot = match pair {
                ChannelPair::InPlace(b) => {
                    ins[c][..frames].copy_from_slice(b);
                    Some(b)
                }
                ChannelPair::InputOutput(i, o) => {
                    ins[c][..frames].copy_from_slice(i);
                    Some(o)
                }
                _ => None,
            };
        }
        let mut notes = [false; 8192];
        for e in events.input {
            self.shared.take(e);
            let note = matches!(
                e.as_core_event(),
                Some(CoreEventSpace::NoteOn(_) | CoreEventSpace::Midi(_))
            );
            if note {
                notes[(e.header().time() as usize).min(frames - 1)] = true;
            }
        }
        let tone = self.shared.tone();
        for f in 0..frames {
            if notes[f] {
                self.burst = 0.5;
            }
            let x = ins[0][f] + self.burst;
            self.burst *= BURST_DECAY;
            let echo = self.line[self.pos];
            self.line[self.pos] = x + FEEDBACK * echo;
            self.pos = (self.pos + 1) % ECHO;
            let y = ins[0][f] + echo + tone;
            for b in bufs.iter_mut().flatten() {
                b[f] = y;
            }
        }
        self.shared.blocks.fetch_add(1, Ordering::Relaxed);
        let decayed = self.line.iter().all(|s| s.abs() <= SILENCE / 4.0)
            && self.burst <= SILENCE / 4.0
            && tone == 0.0;
        Ok(match self.shared.mode.load(Ordering::Relaxed) {
            0 => ProcessStatus::Tail,
            1 if decayed => ProcessStatus::Sleep,
            _ => ProcessStatus::Continue,
        })
    }
}

impl PluginTailImpl for Processor<'_> {
    fn get(&self) -> TailLength {
        TailLength::Finite(TAIL)
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
        let (name, max, flags): (&[u8], f64, ParamInfoFlags) = match index {
            0 => (b"Mode", 2.0, ParamInfoFlags::IS_AUTOMATABLE),
            1 => (b"Tone", 1.0, ParamInfoFlags::IS_AUTOMATABLE),
            2 => (b"Blocks", 1e9, ParamInfoFlags::IS_READONLY),
            _ => return,
        };
        info.set(&ParamInfo {
            id: index.into(),
            flags,
            cookie: Default::default(),
            name,
            module: b"",
            min_value: 0.0,
            max_value: max,
            default_value: if index == 0 { 2.0 } else { 0.0 },
        });
    }

    fn get_value(&mut self, id: ClapId) -> Option<f64> {
        match id.get() {
            0 => Some(f64::from(self.shared.mode.load(Ordering::Relaxed))),
            1 => Some(f64::from(self.shared.tone())),
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
        write!(writer, "{value:.3}")
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

fn entry() -> &'static clack_host::prelude::PluginEntry {
    static ENTRY: OnceLock<clack_host::prelude::PluginEntry> = OnceLock::new();
    ENTRY.get_or_init(|| {
        clack_host::prelude::PluginEntry::load_from_clack::<SinglePluginEntry<TestEcho>>(
            c"/test/test-echo.clap",
        )
        .expect("the test echo's entry")
    })
}

/// "FTS Test Echo Tail", "… Sleep" and "… Continue": the echo in each mode.
struct Echoes;

impl crate::plugin::FxFactory for Echoes {
    fn installed(&self) -> Vec<daw_proto::fx::InstalledFx> {
        Vec::new()
    }
    fn provides(&self, name: &str) -> bool {
        name.starts_with("FTS Test Echo ")
    }
    fn create(&self, name: &str, _: f64) -> Option<Box<dyn crate::plugin::PluginInstance>> {
        let mode = match name.strip_prefix("FTS Test Echo ")? {
            "Tail" => 0.0,
            "Sleep" => 1.0,
            "Continue" => 2.0,
            _ => return None,
        };
        let mut echo = crate::audio_engine::plugin_host::ClapHost::new("daw tests", "FTS")
            .load_entry(entry(), 0)
            .expect("the test echo loads")
            .into_send();
        assert!(crate::plugin::PluginInstance::set_param_value(
            &mut echo, 0, mode
        ));
        Some(Box::new(echo))
    }
}

// ── The song ─────────────────────────────────────────────────────────

/// A track playing 0.5 for its first quarter second, through the echo in
/// `mode`, and, with `note`, a MIDI note that many seconds in. The project
/// and the FX.
fn song(mode: &str, note: Option<f64>) -> (Standalone, String, String) {
    let daw = Standalone::new();
    daw.set_fx_factory(Arc::new(Echoes));
    let guid = daw.seed_project(ProjectInfo {
        guid: format!("sleep-{mode}-{note:?}"),
        name: "sleep".into(),
        path: String::new(),
    });
    seed_media_tracks(
        &daw,
        &guid,
        &[StemSpec::new("Bass", "/x/bass.wav", None)],
        0.0,
        0.25,
    );
    let takes: Vec<String> = daw
        .read_project(&guid, |p| {
            p.takes
                .values()
                .flat_map(|tl| tl.takes.iter().map(|t| t.guid.clone()))
                .collect()
        })
        .unwrap_or_default();
    for take in takes {
        attach_audio_source(
            &daw,
            &guid,
            &take,
            DecodedAudio::new(vec![0.5; RATE as usize], 1, RATE),
        );
    }
    let track = guid_of(&daw, &guid, "Bass");
    if let Some(at) = note {
        let loc = daw_proto::Midi::create_midi_item(
            &daw,
            ProjectContext::Project(guid.clone()),
            TrackRef::Guid(track.clone()),
            at,
            at + 0.5,
        )
        .expect("a MIDI item");
        let daw_proto::ItemRef::Guid(item) = loc.item else {
            panic!("by guid")
        };
        daw.write_project(&guid, |p| {
            let take = p.takes.get(&item).unwrap().takes[0].guid.clone();
            p.midi_notes.insert(
                take,
                vec![MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_ppq: 0.0,
                    length_ppq: 120.0,
                    ..Default::default()
                }],
            );
        });
    }
    let fx = Effects::add(
        &daw,
        ProjectContext::Project(guid.clone()),
        FxChainContext::Track(track),
        &format!("FTS Test Echo {mode}"),
    )
    .expect("the echo is added");
    (daw, guid, fx)
}

/// Blocks `from..to` of [`BLOCK`]: master's left channel.
fn render(renderer: &ProjectRenderer, from: usize, to: usize) -> Vec<f32> {
    (from..to)
        .flat_map(|b| {
            let out = renderer.render_block((b * BLOCK) as u64, BLOCK);
            out.samples
                .chunks_exact(2)
                .map(|f| f[0])
                .collect::<Vec<_>>()
        })
        .collect()
}

fn blocks_run(daw: &Standalone, fx: &str) -> u32 {
    daw.with_plugin_instance(fx, |p| p.param_value(2))
        .flatten()
        .expect("the echo counts its blocks") as u32
}

fn worst(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max)
}

/// The block after the last frame louder than -150 dBFS.
fn quiet_from(out: &[f32]) -> usize {
    out.iter()
        .rposition(|s| s.abs() > SILENCE)
        .map_or(0, |f| f / BLOCK + 1)
}

/// Four seconds.
const BLOCKS: usize = 4 * RATE as usize / BLOCK;

// ── The tests ────────────────────────────────────────────────────────

#[test]
fn a_tail_rings_out_before_its_plugin_sleeps_and_the_render_nulls_against_one_always_running() {
    let (daw, guid, always) = song("Continue", None);
    let want = render(&ProjectRenderer::new(&daw, &guid, RATE), 0, BLOCKS);
    assert_eq!(blocks_run(&daw, &always), BLOCKS as u32);
    let ring_out = quiet_from(&want);
    assert!(
        ring_out > 100 && ring_out < BLOCKS - 20,
        "rings out at block {ring_out}"
    );

    let (daw, guid, fx) = song("Tail", None);
    let out = render(&ProjectRenderer::new(&daw, &guid, RATE), 0, BLOCKS);
    let ran = blocks_run(&daw, &fx) as usize;
    assert!(
        worst(&out, &want) <= SILENCE,
        "off by {}",
        worst(&out, &want)
    );
    // Its input stops at 0.25 s; it runs its tail past that, then sleeps.
    let tail_end = (12_000 + TAIL as usize).div_ceil(BLOCK);
    assert!(
        ran >= ring_out,
        "it slept at block {ran}, before its tail rang out at {ring_out}"
    );
    assert!(
        (tail_end..=tail_end + 1).contains(&ran),
        "ran {ran} blocks, its tail ends at {tail_end}"
    );
}

#[test]
fn a_plugin_that_asks_to_sleep_sleeps_once_quiet_and_a_note_wakes_it() {
    let (daw, guid, _) = song("Continue", Some(3.5));
    let want = render(&ProjectRenderer::new(&daw, &guid, RATE), 0, BLOCKS);
    let (daw, guid, fx) = song("Sleep", Some(3.5));
    let renderer = ProjectRenderer::new(&daw, &guid, RATE);
    let note = (3.5 * f64::from(RATE)) as usize / BLOCK;
    let before = render(&renderer, 0, note);
    let slept_at = blocks_run(&daw, &fx) as usize;
    assert!(
        slept_at < note - 10,
        "still running at the note: {slept_at} blocks"
    );
    let after = render(&renderer, note, BLOCKS);
    let woke = blocks_run(&daw, &fx) as usize - slept_at;
    assert!(
        woke >= BLOCKS - note - 2,
        "ran {woke} of the {} blocks after the note",
        BLOCKS - note
    );
    let out: Vec<f32> = before.into_iter().chain(after).collect();
    assert!(
        worst(&out, &want) <= SILENCE,
        "off by {}",
        worst(&out, &want)
    );
    assert!(
        out[note * BLOCK..].iter().any(|s| s.abs() > 0.1),
        "the note's burst plays"
    );
}

#[test]
fn a_parameter_set_wakes_a_sleeping_plugin() {
    let (daw, guid, fx) = song("Sleep", None);
    let renderer = ProjectRenderer::new(&daw, &guid, RATE);
    let quiet = render(&renderer, 0, BLOCKS);
    assert!(quiet[(BLOCKS - 1) * BLOCK..].iter().all(|s| *s == 0.0));
    let ran = blocks_run(&daw, &fx);
    assert!((ran as usize) < BLOCKS - 20, "asleep: {ran} blocks run");
    // The tone on: the plugin plays it from the next block.
    let track = guid_of(&daw, &guid, "Bass");
    daw_proto::FxParams::set(&daw, FxChainContext::Track(track), 0, 1, 0.25).expect("set");
    let toned = render(&renderer, BLOCKS, BLOCKS + 4);
    assert!(blocks_run(&daw, &fx) >= ran + 4, "woken");
    assert!(
        toned.iter().all(|s| (s - 0.25).abs() < 1e-6),
        "the tone plays: {:?}",
        &toned[..4]
    );
}

#[test]
fn an_offline_render_sleeps_as_the_live_one_does_and_says_how_much() {
    let (daw, guid, _) = song("Sleep", Some(3.5));
    let live = render(&ProjectRenderer::new(&daw, &guid, RATE), 0, BLOCKS);
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
    renderer.prime(BLOCK as u32);
    let offline = render(&renderer, 0, BLOCKS);
    assert!(
        worst(&live, &offline) <= SILENCE,
        "off by {}",
        worst(&live, &offline)
    );
    let stats = &renderer.scratch.lock().unwrap().offline_stats;
    assert!(stats.slept > 10, "{} plugin-blocks slept", stats.slept);
}
