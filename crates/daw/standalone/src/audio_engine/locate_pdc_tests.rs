//! Delay compensation and locates, played block by block through the
//! engine as an audio callback plays them (`TransportShared::begin_block`,
//! then `DualRender::render`):
//!
//! 1. a plugin's latency that changes while playing moves the delays that
//!    compensate it without a click, and the tracks meet again;
//! 2. after a locate while playing, latent and plain tracks meet from the
//!    first frame of the new place, and play after a stop hears nothing
//!    stale from before it;
//! 3. a locate while playing is declicked — rendered whole or ahead.
//!
//! The test plugin delays its input by a latency the test changes while it
//! plays, and says so; it keeps what it has heard (its line) from block to
//! block, as a real lookahead plugin does.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use audio::io::duplex::EngineStats;
use daw_proto::fx::{Effects, FxChainContext};
use daw_proto::{ProjectContext, ProjectInfo};

use super::ahead::{AheadPrefs, DualRender};
use super::declick::DECLICK_FRAMES;
use super::decoder::DecodedAudio;
use super::materialize::attach_audio_source;
use super::render::ProjectRenderer;
use crate::Standalone;
use crate::media_seed::{StemSpec, seed_media_tracks};
use crate::transport_engine::{InstantSamples, PlayStateRepr, TransportShared};

const SR: u32 = 48_000;
const BLOCK: usize = 128;
/// The longest latency the test plugin can take.
const MOST: usize = 8_192;

/// A plugin `latency` samples late, a latency that may change between
/// blocks (its output jumps then, as a real plugin's does).
struct Late {
    line: Vec<[f32; 2]>,
    at: usize,
    latency: Arc<AtomicU32>,
}

impl crate::plugin::PluginInstance for Late {
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
        self.latency.load(Ordering::Relaxed)
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    fn prepare(&mut self, _: f64, _: u32) -> Result<(), crate::plugin::PluginError> {
        Ok(())
    }
    fn is_prepared(&self) -> bool {
        true
    }
    fn process_block(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        _: &crate::plugin::PluginEvents<'_>,
    ) -> Result<(), crate::plugin::PluginError> {
        let late = (self.latency.load(Ordering::Relaxed) as usize).min(MOST - 1);
        for f in 0..in_l.len() {
            self.line[self.at] = [in_l[f], in_r[f]];
            let [l, r] = self.line[(self.at + MOST - late) % MOST];
            self.at = (self.at + 1) % MOST;
            out_l[f] = l;
            out_r[f] = r;
        }
        Ok(())
    }
    fn deactivate(&mut self) {}
}

/// Makes `Late`s that share one latency, the test's to change.
struct Lates(Arc<AtomicU32>);

impl crate::plugin::FxFactory for Lates {
    fn installed(&self) -> Vec<daw_proto::fx::InstalledFx> {
        Vec::new()
    }
    fn provides(&self, name: &str) -> bool {
        name == "Late"
    }
    fn create(&self, name: &str, _: f64) -> Option<Box<dyn crate::plugin::PluginInstance>> {
        self.provides(name).then(|| {
            Box::new(Late {
                line: vec![[0.0; 2]; MOST],
                at: 0,
                latency: self.0.clone(),
            }) as Box<dyn crate::plugin::PluginInstance>
        })
    }
}

/// A song of top-level tracks, each with one item from 0 playing
/// `audio`; `Late` on the tracks named in `late`. The latency handle sets
/// every `Late`'s latency.
struct Song {
    daw: Standalone,
    guid: String,
    latency: Arc<AtomicU32>,
}

fn song(tracks: &[(&str, Vec<f32>)], late: &[&str], latency: u32) -> Song {
    let daw = Standalone::new();
    let handle = Arc::new(AtomicU32::new(latency));
    daw.set_fx_factory(Arc::new(Lates(handle.clone())));
    let guid = daw.seed_project(ProjectInfo {
        guid: "locate-pdc".into(),
        name: "locate-pdc".into(),
        path: String::new(),
    });
    let stems: Vec<StemSpec> = tracks
        .iter()
        .map(|(name, _)| StemSpec::new(*name, format!("/x/{name}.wav"), None))
        .collect();
    seed_media_tracks(&daw, &guid, &stems, 0.0, 10.0);
    let takes: Vec<(String, String)> = daw
        .read_project(&guid, |p| {
            p.takes
                .iter()
                .flat_map(|(item, tl)| {
                    let track = p.items.get(item).map(|e| e.item.track_guid.clone());
                    let name = track
                        .and_then(|g| p.tracks.iter().find(|t| t.guid == g))
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    tl.takes.iter().map(move |t| (t.guid.clone(), name.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    for (take, name) in takes {
        let audio = tracks
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, a)| a.clone())
            .unwrap_or_default();
        attach_audio_source(&daw, &guid, &take, DecodedAudio::new(audio, 1, SR));
    }
    for name in late {
        Effects::add(
            &daw,
            ProjectContext::Project(guid.clone()),
            FxChainContext::Track(super::render::folder_routing_tests::guid_of(
                &daw, &guid, name,
            )),
            "Late",
        )
        .expect("the plugin");
    }
    Song {
        daw,
        guid,
        latency: handle,
    }
}

/// `seconds` of a sine at `hz`, amplitude `amp`.
fn sine(hz: f32, amp: f32, seconds: f32) -> Vec<f32> {
    let n = (seconds * SR as f32) as usize;
    (0..n)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / SR as f32).sin() * amp)
        .collect()
}

/// What a test does before a block: the song, its transport, the block's
/// number.
type Edit<'a> = &'a mut dyn FnMut(&Song, &TransportShared, usize);

/// `blocks` blocks of `BLOCK` played from 0, as a callback plays them
/// (`render_stopped` false, as cpal's: a stopped block is not rendered).
/// The left channel. `ahead`: with a render ahead, paced like a device.
fn play(song: &Song, ahead: bool, blocks: usize, edit: Edit<'_>) -> Vec<f32> {
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let _inside = runtime.enter();
    let tracks = song
        .daw
        .read_project(&song.guid, |p| p.tracks.len())
        .unwrap_or(0);
    song.daw.set_meters(crate::metering::Meters::new(tracks));
    let shared = Arc::new(TransportShared::new(SR, 120.0));
    shared.set_play_state(PlayStateRepr::Playing);
    let live = Arc::new(ProjectRenderer::new(&song.daw, &song.guid, SR));
    live.set_realtime();
    let prefs = ahead.then(AheadPrefs::default);
    let stats = Arc::new(EngineStats::default());
    let mut render = DualRender::new(live, shared.clone(), prefs, stats);
    let period = Duration::from_secs_f64(BLOCK as f64 / f64::from(SR));
    let began = Instant::now();
    let mut out = Vec::with_capacity(blocks * BLOCK);
    for b in 0..blocks {
        edit(song, &shared, b);
        let stamp = b as f64 * period.as_secs_f64() * 1e6;
        let plan = shared.begin_block(BLOCK as u32, stamp, period.as_secs_f64() * 1e6);
        if plan.any_playing() {
            let block = render.render(&plan, false);
            out.extend(block.samples.chunks_exact(2).map(|f| f[0]));
        } else {
            // The callback's stopped block: silence, nothing rendered.
            render.rest();
            out.extend(std::iter::repeat_n(0.0, BLOCK));
        }
        if ahead && let Some(rest) = (period * (b as u32 + 1)).checked_sub(began.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    out
}

/// The largest step from one sample to the next in `signal[from..to]`,
/// and where.
fn worst_step(signal: &[f32], from: usize, to: usize) -> (usize, f32) {
    (from.max(1)..to.min(signal.len()))
        .map(|i| (i, (signal[i] - signal[i - 1]).abs()))
        .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a })
}

/// The longest run of (near) silence in `signal[from..to]`, frames.
fn longest_silence(signal: &[f32], from: usize, to: usize) -> usize {
    let (mut run, mut most) = (0, 0);
    for s in &signal[from..to.min(signal.len())] {
        run = if s.abs() < 1e-4 { run + 1 } else { 0 };
        most = most.max(run);
    }
    most
}

/// A 220 Hz sine at 0.5 moves by at most 0.0145 a sample; a fade over
/// `DECLICK_FRAMES` adds at most 0.5 / `DECLICK_FRAMES` to that. A click is
/// many times either.
const SMOOTH: f32 = 0.0145 + 0.5 * 2.0 / DECLICK_FRAMES as f32 + 0.002;

/// Question 1. Kick carries `Late`; Snare plays a sine and is delayed to
/// meet it. Kick's latency changes while playing — up, down, to none and
/// back — and Snare's compensating delay follows a block later each time:
/// faded or crossfaded, never a cut, and Snare then sits exactly the new
/// latency late.
#[test]
fn a_latency_that_changes_while_playing_moves_its_delays_without_a_click() {
    let song = song(
        &[("Kick", vec![0.0; 4]), ("Snare", sine(220.0, 0.5, 4.0))],
        &["Kick"],
        64,
    );
    let steps: [(usize, u32); 4] = [(100, 300), (200, 32), (300, 0), (400, 100)];
    let mut edit = |s: &Song, _: &TransportShared, b: usize| {
        if let Some((_, l)) = steps.iter().find(|(at, _)| *at == b) {
            s.latency.store(*l, Ordering::Relaxed);
        }
    };
    let heard = play(&song, false, 500, &mut edit);
    let snare = sine(220.0, 0.5, 4.0);
    let (at, step) = worst_step(&heard, 20 * BLOCK, heard.len());
    assert!(
        step < SMOOTH,
        "a click of {step} at frame {at} (block {}), around {:?}",
        at / BLOCK,
        &heard[at.saturating_sub(3)..at + 3]
    );
    let gap = longest_silence(&heard, 20 * BLOCK, heard.len());
    assert!(gap < 300, "a gap of {gap} frames");
    // Settled after each change: Snare exactly the new latency late.
    for (i, &(from, late)) in steps.iter().enumerate() {
        let to = steps.get(i + 1).map_or(500, |s| s.0);
        let late = late as usize;
        // Followed a block later; then the old line, the fade in.
        let settled = from + 2 + (late + DECLICK_FRAMES).div_ceil(BLOCK);
        let worst = (settled * BLOCK..to * BLOCK)
            .map(|f| (heard[f] - snare[f - late]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1e-5,
            "{late} late from block {from}: off by {worst}"
        );
    }
}

/// Question 2. A locate while playing, with Kick `latency` late: Kick and
/// Snare (whose clicks are at the same place, 500 frames past the
/// locate's target) meet in one click, the song's latency after where
/// the locate's target is heard; nothing between but the old place's
/// last frames, faded.
fn a_locate_meets_from_its_first_frame(latency: u32) {
    const TARGET: usize = 60_000;
    const AT: usize = 40;
    let mut kick = sine(110.0, 0.25, 0.5);
    kick.resize(2 * SR as usize, 0.0);
    kick[TARGET + 500] = 0.5;
    let mut snare = sine(220.0, 0.25, 0.5);
    snare.resize(2 * SR as usize, 0.0);
    snare[TARGET + 500] = 0.5;
    let song = song(&[("Kick", kick), ("Snare", snare)], &["Kick"], latency);
    let mut edit = |_: &Song, t: &TransportShared, b: usize| {
        if b == AT {
            t.set_playhead(InstantSamples(TARGET as i64));
        }
    };
    let heard = play(&song, false, 60, &mut edit);
    let jump = AT * BLOCK;
    let late = latency as usize;
    let clicks: Vec<(usize, f32)> = (jump..heard.len())
        // (Each sine is 0.25: the old place, playing on, sums to 0.5.)
        .filter(|&f| heard[f].abs() > 0.6)
        .map(|f| (f, heard[f]))
        .collect();
    assert_eq!(
        clicks,
        vec![(jump + late + 500, 1.0)],
        "one click, both tracks in it, {late} late"
    );
    // The old place, faded out, then nothing until the click.
    let quiet = jump + late.max(DECLICK_FRAMES)..jump + late + 500;
    let worst = quiet
        .clone()
        .map(|f| heard[f].abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 1e-5, "{worst} heard over {quiet:?}: stale audio");
    let (at, step) = worst_step(&heard, jump - BLOCK, jump + late + 400);
    assert!(step < SMOOTH, "a click of {step} at frame {at}");
}

#[test]
fn a_locate_meets_from_its_first_frame_shorter_than_the_declick() {
    a_locate_meets_from_its_first_frame(64);
}

#[test]
fn a_locate_meets_from_its_first_frame_longer_than_the_declick() {
    a_locate_meets_from_its_first_frame(1_000);
}

/// Question 2, from stop. Played, stopped and played again elsewhere: the
/// first `latency` frames heard are what was in the delays and the plugin
/// when it stopped — unless they are not heard.
#[test]
fn play_after_stop_hears_nothing_stale() {
    const LATE: u32 = 500;
    let mut snare = sine(220.0, 0.5, 0.5);
    snare.resize(2 * SR as usize, 0.0);
    let song = song(
        &[("Kick", sine(110.0, 0.5, 0.5)), ("Snare", snare)],
        &["Kick"],
        LATE,
    );
    let mut edit = |_: &Song, t: &TransportShared, b: usize| match b {
        30 => t.set_play_state(PlayStateRepr::Stopped),
        35 => {
            t.set_playhead(InstantSamples(60_000));
            t.set_play_state(PlayStateRepr::Playing);
        }
        _ => {}
    };
    let heard = play(&song, false, 50, &mut edit);
    let worst = heard[35 * BLOCK..]
        .iter()
        .map(|s| s.abs())
        .fold(0.0_f32, f32::max);
    assert!(
        worst < 1e-5,
        "{worst} heard after play, where the song is silent: what was playing at stop"
    );
}

/// Question 3. A locate while playing into a sine of another phase, with
/// no latency: faded out and in, no click, no long gap; then exactly the
/// new place.
fn a_locate_while_playing_is_declicked(ahead: bool) {
    const AT: usize = 100;
    const TARGET: usize = 30_017;
    let tone = sine(220.0, 0.5, 2.0);
    let song = song(&[("Snare", tone.clone())], &[], 0);
    let mut edit = |_: &Song, t: &TransportShared, b: usize| {
        if b == AT {
            t.set_playhead(InstantSamples(TARGET as i64));
        }
    };
    let heard = play(&song, ahead, 160, &mut edit);
    let jump = AT * BLOCK;
    let (at, step) = worst_step(&heard, 10 * BLOCK, heard.len());
    assert!(
        step < SMOOTH,
        "a click of {step} at frame {at} (the locate at {jump})"
    );
    let gap = longest_silence(&heard, jump - BLOCK, jump + 4 * BLOCK);
    assert!(gap <= 4, "a gap of {gap} frames");
    // Faded in, then the new place, frame for frame.
    let from = jump + 2 * DECLICK_FRAMES;
    let worst = (from..heard.len())
        .map(|f| (heard[f] - tone[TARGET + f - jump]).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 1e-5, "off the new place by {worst}");
    // Not doubled: never louder than the tone.
    let loudest = heard.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(loudest <= 0.5 + 1e-4, "{loudest}: two places at once");
}

#[test]
fn a_locate_while_playing_is_declicked_rendered_whole() {
    a_locate_while_playing_is_declicked(false);
}

#[test]
fn a_locate_while_playing_is_declicked_rendered_ahead() {
    a_locate_while_playing_is_declicked(true);
}

/// Question 3, through delay compensation: a locate with a plugin
/// 1 000 frames late is faded where it is heard — the latency after the
/// jump — not where it was rendered.
fn a_locate_is_declicked_where_it_is_heard(ahead: bool) {
    const AT: usize = 100;
    const TARGET: usize = 30_017;
    const LATE: usize = 1_000;
    let tone = sine(220.0, 0.5, 2.0);
    let mut edit = |_: &Song, t: &TransportShared, b: usize| {
        if b == AT {
            t.set_playhead(InstantSamples(TARGET as i64));
        }
    };
    let song = song(
        &[("Snare", tone.clone()), ("Kick", vec![0.0; 4])],
        &["Kick"],
        LATE as u32,
    );
    let heard = play(&song, ahead, 160, &mut edit);
    let jump = AT * BLOCK;
    let (at, step) = worst_step(&heard, 20 * BLOCK, heard.len());
    assert!(
        step < SMOOTH,
        "a click of {step} at frame {at} (the locate at {jump}, heard at {})",
        jump + LATE
    );
    // The old place played on, every track aligned, until it is heard.
    let worst = (jump..jump + LATE - DECLICK_FRAMES)
        .map(|f| (heard[f] - tone[f - LATE]).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 1e-5, "off the old place by {worst}");
    let from = jump + LATE + DECLICK_FRAMES;
    let worst = (from..heard.len())
        .map(|f| (heard[f] - tone[TARGET + f - jump - LATE]).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 1e-5, "off the new place by {worst}");
}

#[test]
fn a_locate_is_declicked_where_it_is_heard_rendered_whole() {
    a_locate_is_declicked_where_it_is_heard(false);
}

/// Rendered ahead, the delays of the tracks ahead are two renderers' (the
/// callback's, while it renders a run's start whole, and the render
/// ahead's): after a locate each plays the latency's worth of what its own
/// lines last held — measured, a block of the old place a block out of
/// place from the jump, then the render ahead's stale frames up to the
/// latency past its first block of the new run. Not the declick's to fix:
/// the lines have to follow the tracks from renderer to renderer.
#[test]
#[ignore = "render ahead + delay compensation: each renderer's delay lines are stale after a locate"]
fn a_locate_is_declicked_where_it_is_heard_rendered_ahead() {
    a_locate_is_declicked_where_it_is_heard(true);
}
