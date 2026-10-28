//! The set engine's audio: one output stream that mixes every song of a
//! set, each on its own transport (`docs/design/set-engine.md`, step 1).
//!
//! A [`Voice`] is one song that may sound: its project's realtime
//! [`ProjectRenderer`] (snapshot built when the voice is made, off the
//! audio thread) and the project's own [`TransportShared`]. [`SetAudio`]
//! owns the stream and a [`Voices`] list that control threads swap in
//! whole; the callback only ever `load`s it (no lock, no allocation).
//!
//! Per block ([`render_set_block`]): one [`BufferClock`] stamp; every
//! voice opens the block on its transport with that same stamp
//! (`begin_block`), so a [`ScheduledLocate`] lands on the same frame in
//! every song; the voices that play render their plan into their own
//! block buffer and are summed. A stopped voice costs its `begin_block`.
//! Then, as in the one-song engine (`mixer.rs`), the aux hook (its clock
//! from the *current* voice), the output routing and the device format.
//!
//! **Soft clocks.** Making a voice disables its project's soft clock —
//! the callback drives that transport from then on — and removing it puts
//! the soft clock back as it was. A dead device turns every voice's soft
//! clock back on, so no playhead freezes.
//!
//! **Meters.** A backend has one meter bank, by track index, read as the
//! current project's (`Standalone::meters`, the meter pump). So only the
//! current voice meters ([`ProjectRenderer::set_metering`]); the others
//! render without touching the bank, and while the current voice is
//! stopped its cells fall to silence, as they do in the one-song engine.
//! Making a voice current sizes the bank for its tracks.
//!
//! **Live MIDI** goes to the current voice (the backend has one live-MIDI
//! producer): making a voice current connects its renderer.
//!
//! [`BufferClock`]: daw_transport_sync::BufferClock

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use audio::io::AudioIoPrefs;
use audio::io::duplex::EngineStats;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

use crate::metering::{HOLD_DECAY, Meters};
use crate::sync::Standalone;
use crate::transport_engine::{BlockPlan, ScheduledLocate, TransportShared};

use super::ahead::{AheadPrefs, DualRender};
use super::aux_render::{AuxClock, AuxRenderer, AuxSlot};
use super::mixer::{AUX_MAX_FRAMES, ResolvedOutput, is_xrun};
use super::prefetch::PrefetchWorker;
use super::render::ProjectRenderer;
use super::routing;

/// Capacity of the live-MIDI ring a current voice is connected with.
const LIVE_MIDI_CAPACITY: usize = 4096;

/// How long a control thread waits for the callback to let go of a voice
/// list it replaced, so a voice is never freed on the audio thread. One
/// block is enough; this is the bound for a stalled callback.
const RETIRE_WAIT: Duration = Duration::from_millis(500);

/// What went wrong with a set operation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SetAudioError {
    /// The backend has no project with this guid.
    #[error("no project {0}")]
    NoProject(String),
    /// The set has no voice for this project.
    #[error("no voice for project {0}")]
    NoVoice(String),
    /// A handoff needs the song it leaves to be playing.
    #[error("project {0} is not playing")]
    NotPlaying(String),
    /// A handoff's moment is behind the song's playhead already.
    #[error("project {project} is past {end_seconds} s")]
    EndPassed { project: String, end_seconds: f64 },
}

/// How voices are made: the device's rate and largest block, and whether
/// their tracks render ahead.
#[derive(Clone)]
pub struct VoiceSetup {
    /// Output sample rate; each voice's transport is set to it.
    pub sample_rate: u32,
    /// Largest block the callback renders at once, frames — each voice's
    /// block buffer is reserved for it.
    pub max_frames: usize,
    /// The render ahead per voice (`None`: every track renders in the
    /// callback).
    pub ahead: Option<AheadPrefs>,
    /// Where the render ahead reports.
    pub stats: Arc<EngineStats>,
    /// Warm each voice's media pages ahead of its playhead (a thread per
    /// voice).
    pub prefetch: bool,
}

impl VoiceSetup {
    /// Voices for rendering without a device: no render ahead, no
    /// prefetch.
    #[must_use]
    pub fn offline(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            max_frames: AUX_MAX_FRAMES,
            ahead: None,
            stats: Arc::default(),
            prefetch: false,
        }
    }
}

/// One song of the set that may sound.
pub struct Voice {
    guid: String,
    renderer: Arc<ProjectRenderer>,
    shared: Arc<TransportShared>,
    /// The callback's render of this song. Only the callback locks it
    /// (`try_lock`, so never waits); a mutex because the voice is shared.
    render: Mutex<DualRender>,
    /// The project's soft-clock switch, and how it was before this voice.
    soft_clock: Arc<AtomicBool>,
    soft_clock_before: bool,
    _prefetch: Option<PrefetchWorker>,
}

impl Voice {
    /// Make the voice of project `guid`: its renderer realtime and its
    /// first snapshot built here, its transport set to the output rate
    /// and its soft clock off (the set's callback drives it from now on).
    /// The voice does not meter until it is made current.
    pub fn open(daw: &Standalone, guid: &str, setup: &VoiceSetup) -> Result<Self, SetAudioError> {
        if daw.read_project(guid, |_| ()).is_none() {
            return Err(SetAudioError::NoProject(guid.to_owned()));
        }
        let bundle = daw.transport_engine_for(guid);
        let soft_clock_before = bundle.soft_clock_enabled.load(Ordering::Relaxed);
        bundle.disable_soft_clock();
        let shared = bundle.shared.clone();
        shared.set_sample_rate(setup.sample_rate);

        let renderer = Arc::new(ProjectRenderer::new(daw, guid, setup.sample_rate));
        renderer.set_realtime();
        renderer.set_metering(false);
        let mut render = DualRender::new(
            renderer.clone(),
            shared.clone(),
            setup.ahead,
            setup.stats.clone(),
        );
        render.reserve(setup.max_frames);
        // The first snapshot, built here rather than on the audio thread.
        let _ = renderer.refresh();

        let prefetch = setup
            .prefetch
            .then(|| PrefetchWorker::spawn(daw.clone(), guid.to_owned(), shared.clone()));
        Ok(Self {
            guid: guid.to_owned(),
            renderer,
            shared,
            render: Mutex::new(render),
            soft_clock: bundle.soft_clock_enabled.clone(),
            soft_clock_before,
            _prefetch: prefetch,
        })
    }

    /// The project this voice plays.
    #[must_use]
    pub fn guid(&self) -> &str {
        &self.guid
    }

    /// The project's transport.
    #[must_use]
    pub fn shared(&self) -> &Arc<TransportShared> {
        &self.shared
    }

    /// The project's renderer.
    #[must_use]
    pub fn renderer(&self) -> &Arc<ProjectRenderer> {
        &self.renderer
    }

    /// Give the transport back to its soft clock, as it was before this
    /// voice took it.
    pub fn release(&self) {
        self.soft_clock
            .store(self.soft_clock_before, Ordering::Relaxed);
    }

    /// Render this voice's block as `plan` says and add it into `out`
    /// (interleaved stereo). `false` when nothing played: a stopped
    /// voice renders nothing (its render ahead rests).
    fn mix_into(&self, plan: &BlockPlan, out: &mut [f32]) -> bool {
        let mut render = match self.render.try_lock() {
            Ok(render) => render,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if !plan.any_playing() {
            render.rest();
            return false;
        }
        let block = render.render(plan, false);
        for (o, s) in out.iter_mut().zip(&block.samples) {
            *o += *s;
        }
        true
    }
}

/// The set's voices, and which is current — swapped in whole.
#[derive(Clone, Default)]
pub struct Voices {
    list: Vec<Arc<Voice>>,
    current: Option<usize>,
}

impl Voices {
    /// The voices, in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<Voice>> {
        self.list.iter()
    }

    /// The voice of project `guid`.
    #[must_use]
    pub fn get(&self, guid: &str) -> Option<&Arc<Voice>> {
        self.list.iter().find(|v| v.guid == guid)
    }

    /// The current voice: the one the aux hook's clock, the meters and
    /// live MIDI follow.
    #[must_use]
    pub fn current(&self) -> Option<&Arc<Voice>> {
        self.current.and_then(|i| self.list.get(i))
    }

    /// How many voices there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// These voices and `voice`.
    #[must_use]
    pub fn with(&self, voice: Arc<Voice>) -> Self {
        let mut next = self.clone();
        next.list.push(voice);
        next
    }

    /// These voices without project `guid`'s (no longer current if it was).
    #[must_use]
    pub fn without(&self, guid: &str) -> Self {
        let current = self.current().map(|v| v.guid.clone());
        let list: Vec<_> = self
            .list
            .iter()
            .filter(|v| v.guid != guid)
            .cloned()
            .collect();
        let current = current.and_then(|c| list.iter().position(|v| v.guid == c));
        Self { list, current }
    }

    /// These voices with project `guid`'s current (none for `None`, or a
    /// guid without a voice).
    #[must_use]
    pub fn with_current(&self, guid: Option<&str>) -> Self {
        let current = guid.and_then(|g| self.list.iter().position(|v| v.guid == g));
        Self {
            list: self.list.clone(),
            current,
        }
    }
}

/// One block's timing: its length and when its first frame starts in the
/// sync clock ([`crate::transport_sync::now_micros`], filtered by a
/// [`daw_transport_sync::BufferClock`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockClock {
    pub frames: u32,
    pub stamp_micros: f64,
    pub period_micros: f64,
}

/// What a block of the set did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetBlock {
    /// Whether any voice played in the block.
    pub any_playing: bool,
    /// The current voice's plan for the block (its clock, for the aux
    /// hook); `None` with no current voice.
    pub current: Option<BlockPlan>,
}

/// Mix one block of the set into `out` (interleaved stereo, overwritten):
/// every voice opens the block on its own transport with the same stamp,
/// and the voices that play are summed. What the device callback runs per
/// block, and what a test runs without a device.
///
/// Renders `min(clock.frames, out.len() / 2)` frames. Allocation-free once
/// each voice's block buffer has held a block this size, except for a
/// locate landing mid-block (`ProjectRenderer::render_plan`).
pub fn render_set_block(voices: &Voices, clock: BlockClock, out: &mut [f32]) -> SetBlock {
    let frames = clock
        .frames
        .min(u32::try_from(out.len() / 2).unwrap_or(u32::MAX));
    let out = &mut out[..frames as usize * 2];
    out.fill(0.0);
    let mut block = SetBlock {
        any_playing: false,
        current: None,
    };
    for (i, voice) in voices.list.iter().enumerate() {
        let plan = voice
            .shared
            .begin_block(frames, clock.stamp_micros, clock.period_micros);
        if voices.current == Some(i) {
            block.current = Some(plan);
        }
        block.any_playing |= voice.mix_into(&plan, out);
    }
    block
}

/// Hand the set from one song to the next on one frame: `to` starts at
/// its beginning, playing, at the moment `from` reaches `from_end_seconds`
/// of its timeline, and `from` stops there — both as scheduled locates in
/// the sync clock, so the block that holds the moment lands both on the
/// same frame. Returns the moment (sync clock, µs).
///
/// The moment is reckoned from `from`'s latest block (its sync snapshot):
/// `from` must be playing, driven by a callback, and not yet past the end.
/// Each transport holds one scheduled locate: this replaces any armed on
/// either.
pub fn schedule_handoff(
    from: &TransportShared,
    to: &TransportShared,
    from_end_seconds: f64,
) -> Result<f64, HandoffRefused> {
    let snap = from
        .sync_snapshot()
        .filter(|s| s.is_playing)
        .ok_or(HandoffRefused::NotPlaying)?;
    if from_end_seconds < snap.playhead_seconds {
        return Err(HandoffRefused::EndPassed);
    }
    let rate = if snap.playrate.is_finite() && snap.playrate > 0.0 {
        snap.playrate
    } else {
        1.0
    };
    let at_micros = snap.host_micros + (from_end_seconds - snap.playhead_seconds) / rate * 1e6;
    to.schedule_locate(ScheduledLocate {
        at_micros,
        position_seconds: 0.0,
        playing: true,
        rate: 1.0,
    });
    from.schedule_locate(ScheduledLocate {
        at_micros,
        position_seconds: from_end_seconds,
        playing: false,
        rate,
    });
    Ok(at_micros)
}

/// Why [`schedule_handoff`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffRefused {
    /// The song handed from is not playing (or no callback drives it yet).
    NotPlaying,
    /// Its playhead is past the end already.
    EndPassed,
}

/// The set's one realtime audio engine: one output stream mixing every
/// voice. Drop it to stop the stream (every voice's soft clock is put
/// back).
pub struct SetAudio {
    daw: Standalone,
    voices: Arc<ArcSwap<Voices>>,
    /// Serialises control-side edits of the voice list (never the audio
    /// thread's).
    edit: Mutex<()>,
    setup: VoiceSetup,
    aux: AuxSlot,
    stream_error: Arc<AtomicBool>,
    stats: Arc<EngineStats>,
    _stream: Stream,
}

impl SetAudio {
    /// Open the output device `prefs` choose and start the stream, with
    /// no voices yet (silence, the aux hook still runs).
    pub fn open(daw: &Standalone, prefs: &AudioIoPrefs) -> Result<Self, String> {
        let out = ResolvedOutput::open(prefs)?;
        tracing::info!(
            audio.channels = out.channels,
            audio.sample_rate = out.sample_rate,
            audio.format = ?out.sample_format,
            "set audio: output open"
        );
        let stats = Arc::new(EngineStats::default());
        let setup = VoiceSetup {
            sample_rate: out.sample_rate,
            max_frames: AUX_MAX_FRAMES,
            ahead: AheadPrefs::chosen(prefs),
            stats: stats.clone(),
            prefetch: true,
        };
        // The bank follows the current song's tracks from here on.
        daw.set_meters(Meters::new(0));
        let voices = Arc::new(ArcSwap::from_pointee(Voices::default()));
        let aux = AuxSlot::default();
        let stream_error = Arc::new(AtomicBool::new(false));
        let wiring = StreamWiring {
            daw: daw.clone(),
            voices: voices.clone(),
            aux: aux.clone(),
            stream_error: stream_error.clone(),
            stats: stats.clone(),
        };
        let stream = match out.sample_format {
            SampleFormat::F32 => build_set_stream::<f32>(&out.device, &out.config, wiring)?,
            SampleFormat::I16 => build_set_stream::<i16>(&out.device, &out.config, wiring)?,
            SampleFormat::U16 => build_set_stream::<u16>(&out.device, &out.config, wiring)?,
            format => return Err(format!("Unsupported sample format: {format:?}")),
        };
        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {e}"))?;
        Ok(Self {
            daw: daw.clone(),
            voices,
            edit: Mutex::new(()),
            setup,
            aux,
            stream_error,
            stats,
            _stream: stream,
        })
    }

    /// Add project `guid`'s voice: its renderer made and its snapshot
    /// built here, then swapped into the list. A voice already there is
    /// kept as it is.
    pub fn add_voice(&self, guid: &str) -> Result<(), SetAudioError> {
        let _edit = self.lock_edit();
        if self.voices.load().get(guid).is_some() {
            return Ok(());
        }
        let voice = Arc::new(Voice::open(&self.daw, guid, &self.setup)?);
        let next = self.voices.load().with(voice);
        self.publish(next);
        tracing::debug!(set.project = guid, "set audio: voice added");
        Ok(())
    }

    /// Remove project `guid`'s voice: it stops sounding from the next
    /// block, and its transport goes back to its soft clock. `false` when
    /// there was none.
    pub fn remove_voice(&self, guid: &str) -> bool {
        let _edit = self.lock_edit();
        let now = self.voices.load_full();
        let Some(voice) = now.get(guid).cloned() else {
            return false;
        };
        self.publish(now.without(guid));
        voice.release();
        tracing::debug!(set.project = guid, "set audio: voice removed");
        true
    }

    /// The projects with a voice, in the order they were added.
    #[must_use]
    pub fn voices(&self) -> Vec<String> {
        self.voices.load().iter().map(|v| v.guid.clone()).collect()
    }

    /// The voice list as the callback sees it now.
    #[must_use]
    pub fn voice_list(&self) -> Arc<Voices> {
        self.voices.load_full()
    }

    /// Make project `guid`'s voice current (`None`: no current voice):
    /// the meters, live MIDI and the aux hook's clock follow it. It does
    /// not decide what sounds.
    pub fn set_current(&self, guid: Option<&str>) -> Result<(), SetAudioError> {
        let _edit = self.lock_edit();
        let now = self.voices.load_full();
        let next = match guid {
            Some(g) if now.get(g).is_none() => return Err(SetAudioError::NoVoice(g.to_owned())),
            _ => now.with_current(guid),
        };
        for voice in next.iter() {
            voice.renderer.set_metering(false);
        }
        if let Some(voice) = next.current() {
            let tracks = self
                .daw
                .read_project(&voice.guid, |p| p.tracks.len())
                .unwrap_or(0);
            if self.daw.meters().len() < tracks {
                self.daw.set_meters(Meters::new(tracks));
            }
            voice.renderer.set_metering(true);
            voice.renderer.connect_live_midi(LIVE_MIDI_CAPACITY);
        }
        self.publish(next);
        Ok(())
    }

    /// The current voice's project.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.voices.load().current().map(|v| v.guid.clone())
    }

    /// Hand the set from song `from` to song `to` on one frame: `to`
    /// starts from its beginning at the moment `from` reaches
    /// `from_end_seconds`, and `from` stops there (see
    /// [`schedule_handoff`]). Returns the moment (sync clock, µs).
    pub fn play_after(
        &self,
        from: &str,
        to: &str,
        from_end_seconds: f64,
    ) -> Result<f64, SetAudioError> {
        let voices = self.voices.load();
        let voice = |g: &str| {
            voices
                .get(g)
                .cloned()
                .ok_or_else(|| SetAudioError::NoVoice(g.to_owned()))
        };
        let (a, b) = (voice(from)?, voice(to)?);
        schedule_handoff(&a.shared, &b.shared, from_end_seconds).map_err(|refused| match refused {
            HandoffRefused::NotPlaying => SetAudioError::NotPlaying(from.to_owned()),
            HandoffRefused::EndPassed => SetAudioError::EndPassed {
                project: from.to_owned(),
                end_seconds: from_end_seconds,
            },
        })
    }

    /// Call off a hand-over from `from` to `to` ([`Self::play_after`])
    /// that has not landed: both songs' scheduled locates are dropped, so
    /// `to` does not start by itself and `from` is not stopped at its end.
    /// A hand-over already landed is not undone.
    pub fn call_off(&self, from: &str, to: &str) -> Result<(), SetAudioError> {
        let voices = self.voices.load();
        for guid in [from, to] {
            voices
                .get(guid)
                .ok_or_else(|| SetAudioError::NoVoice(guid.to_owned()))?
                .shared
                .cancel_scheduled_locate();
        }
        Ok(())
    }

    /// Install (or replace) the post-render aux hook — the contract of
    /// [`aux_render`](super::aux_render), its clock the current voice's.
    pub fn set_aux_renderer(&self, hook: AuxRenderer) {
        if let Ok(mut slot) = self.aux.lock() {
            *slot = Some(hook);
        }
    }

    /// Remove the aux hook (dropped here, off the audio thread).
    pub fn clear_aux_renderer(&self) {
        if let Ok(mut slot) = self.aux.lock() {
            *slot = None;
        }
    }

    /// Realtime metrics from the output callback.
    #[must_use]
    pub fn stats(&self) -> Arc<EngineStats> {
        self.stats.clone()
    }

    /// The output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.setup.sample_rate
    }

    /// `true` once the output device's error callback has fired (the
    /// stream is dead; every voice's soft clock is back on). Latched.
    #[must_use]
    pub fn stream_errored(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
    }

    fn lock_edit(&self) -> std::sync::MutexGuard<'_, ()> {
        self.edit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Swap `next` in, and free the list it replaces here once the
    /// callback has let go of it.
    fn publish(&self, next: Voices) {
        retire(self.voices.swap(Arc::new(next)));
    }
}

impl Drop for SetAudio {
    fn drop(&mut self) {
        // The callback stops driving the transports before their soft
        // clocks take them back: never two drivers for one block.
        let _ = self._stream.pause();
        for voice in self.voices.load().iter() {
            voice.release();
        }
    }
}

/// Free a replaced voice list once the callback holds it no more (it
/// holds a list for one block), so no voice is freed on the audio thread.
fn retire(old: Arc<Voices>) {
    let deadline = Instant::now() + RETIRE_WAIT;
    while Arc::strong_count(&old) > 1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(old);
}

/// What the output callback shares with the control side.
struct StreamWiring {
    daw: Standalone,
    voices: Arc<ArcSwap<Voices>>,
    aux: AuxSlot,
    stream_error: Arc<AtomicBool>,
    stats: Arc<EngineStats>,
}

/// The aux hook's clock for a block: the current voice's (the run that
/// plays, when a locate lands mid-block), or a stopped clock at zero.
fn aux_clock(voices: &Voices, block: &SetBlock, sample_rate: u32, channels: usize) -> AuxClock {
    let sr = f64::from(sample_rate.max(1));
    let (playing, pos_seconds, playrate) = block.current.map_or((false, 0.0, 1.0), |plan| {
        let run = plan.last_playing().unwrap_or(plan.first());
        (
            plan.any_playing(),
            run.start_at_frame_zero().max(0.0) / sr,
            run.rate,
        )
    });
    let (pos_beats, tempo_bpm, time_sig_num, time_sig_den) = voices
        .current()
        .map_or((0.0, 120.0, 4, 4), |v| v.renderer.clock_info(pos_seconds));
    AuxClock {
        playing,
        pos_seconds,
        pos_beats,
        tempo_bpm,
        time_sig_num,
        time_sig_den,
        sample_rate: sr,
        channels,
        playrate,
    }
}

/// Run the aux hook over `stage`, unless a control thread is installing
/// one this instant.
fn run_aux(aux: &AuxSlot, stage: &mut [f32], clock: &AuxClock) {
    let Ok(mut slot) = aux.try_lock() else { return };
    if let Some(hook) = slot.as_mut() {
        hook(stage, clock);
    }
}

/// The current song's meters fall to silence while it does not play
/// (nothing renders, so nothing else writes them). Skipped for a block
/// when the bank is being replaced.
fn silence_meters(daw: &Standalone) {
    let Ok(bank) = daw.meters.try_lock() else {
        return;
    };
    for i in 0..bank.len() {
        if let Some(cell) = bank.cell(i) {
            cell.write(0.0, 0.0, HOLD_DECAY);
        }
    }
}

fn build_set_stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &StreamConfig,
    wiring: StreamWiring,
) -> Result<Stream, String> {
    let StreamWiring {
        daw,
        voices,
        aux,
        stream_error,
        stats,
    } = wiring;
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    routing::MixerRouting::shared().set_channel_count(channels);
    // Pre-sized so the callback never allocates: the set's stereo sum,
    // and the interleaved device-channel stage the aux hook adds into.
    let mut mix = vec![0.0f32; AUX_MAX_FRAMES * 2];
    let mut stage = vec![0.0f32; AUX_MAX_FRAMES * channels.max(1)];
    let mut stamps = daw_transport_sync::BufferClock::default();
    let mut realtime = false;
    let voices_on_error = voices.clone();
    let stats_on_error = stats.clone();
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                if !realtime {
                    realtime = true;
                    let _ = audio::io::realtime::promote_current_thread();
                }
                let entry_micros = crate::transport_sync::now_micros();
                if channels == 0 || data.is_empty() {
                    return;
                }
                let frames = data.len() / channels;
                let stamp = stamps.tick(entry_micros, frames as u32, f64::from(sample_rate));
                let period = stamps.period_micros();
                let t0 = Instant::now();
                stats.calls.fetch_add(1, Ordering::Relaxed);
                stats.block_frames.store(frames as u32, Ordering::Relaxed);
                let list = voices.load();
                let mut current_played = false;
                // A device block past the staging buffers renders in
                // pieces, each stamped where it starts.
                let mut done = 0;
                while done < frames {
                    let n = (frames - done).min(AUX_MAX_FRAMES);
                    let clock = BlockClock {
                        frames: n as u32,
                        stamp_micros: stamp + period * done as f64 / frames as f64,
                        period_micros: period * n as f64 / frames as f64,
                    };
                    let block = render_set_block(&list, clock, &mut mix[..n * 2]);
                    current_played |= block.current.is_some_and(|p| p.any_playing());
                    let stage = &mut stage[..n * channels];
                    let snap = routing::MixerRouting::shared().snapshot();
                    routing::stage_main(stage, channels, n, &mix[..n * 2], snap);
                    run_aux(
                        &aux,
                        stage,
                        &aux_clock(&list, &block, sample_rate, channels),
                    );
                    routing::finish_routing(stage, channels, n, snap);
                    let dst = &mut data[done * channels..(done + n) * channels];
                    for (out, &v) in dst.iter_mut().zip(stage.iter()) {
                        *out = T::from_sample(v);
                    }
                    done += n;
                }
                if !current_played {
                    silence_meters(&daw);
                }
                stats.record_render_at(t0.elapsed().as_nanos() as u64, sample_rate);
            },
            move |err| {
                if is_xrun(&err) {
                    stats_on_error.xruns.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(audio.error = %err, "set audio: xrun");
                    return;
                }
                // The callback that drives every voice's transport stops:
                // their soft clocks take over so no playhead freezes, and
                // a supervisor reopens the device.
                for voice in voices_on_error.load().iter() {
                    voice.soft_clock.store(true, Ordering::Relaxed);
                }
                stream_error.store(true, Ordering::Relaxed);
                tracing::error!(
                    audio.error = %err,
                    "set audio: stream died; soft clocks back on, flagged for re-attach"
                );
            },
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))
}

#[cfg(test)]
mod tests;
