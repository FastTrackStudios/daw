//! The render scheduler: a song of nested folders, sends (pre-FX, post-FX,
//! post-fader, a sidechain key) and stateful, latent FX renders the same
//! samples on one thread as on many — and as it did before the scheduler.
//!
//! The benchmark is by hand:
//! `cargo test -p daw-standalone --lib render_scheduler_bench -- --ignored --nocapture`
//! (the dev profile builds this crate at opt-level 3); `BENCH_THREADS=0,7`
//! picks the worker counts, `BENCH_KITS` and `BENCH_STAGES` the song.

use std::sync::Arc;
use std::time::Instant;

use super::folder_routing_tests::guid_of;
use super::offline::{Offline, Tap};
use super::*;
use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::materialize::attach_audio_source;
use crate::media_seed::{StemSpec, seed_media_tracks};
use daw_proto::fx::{Effects, FxChainContext};
use daw_proto::{
    ProjectContext, ProjectInfo, RouteLocation, RouteRef, RouteType, Routing, SendMode, TrackRef,
    Tracks,
};

const RATE: u32 = 48_000;

/// A test FX with memory and a cost: `stages` one-pole filters in a row,
/// then `latency` samples of delay (and it says so), plus half its
/// sidechain — a block run with the wrong state, input, key or delay shows.
struct Work {
    stages: Vec<[f32; 2]>,
    line: Vec<[f32; 2]>,
    at: usize,
    key: Vec<f32>,
}

impl Work {
    fn new(stages: usize, latency: usize) -> Self {
        Self {
            stages: vec![[0.0; 2]; stages],
            line: vec![[0.0; 2]; latency],
            at: 0,
            key: Vec::new(),
        }
    }
}

impl crate::plugin::PluginInstance for Work {
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
        self.line.len() as u32
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
            let (mut l, mut r) = (in_l[f], in_r[f]);
            for (s, st) in self.stages.iter_mut().enumerate() {
                let c = 0.02 + 0.6 * ((s % 13) as f32 / 13.0);
                st[0] += c * (l - st[0]);
                st[1] += c * (r - st[1]);
                l = st[0] * 0.9 + l * 0.1;
                r = st[1] * 0.9 + r * 0.1;
            }
            if !self.line.is_empty() {
                let [dl, dr] = self.line[self.at];
                self.line[self.at] = [l, r];
                self.at = (self.at + 1) % self.line.len();
                (l, r) = (dl, dr);
            }
            let key = self.key.get(f).copied().unwrap_or(0.0) * 0.5;
            out_l[f] = l + key;
            out_r[f] = r - key;
        }
        self.key.clear();
        Ok(())
    }
    fn deactivate(&mut self) {}
}

/// Makes `Work:<stages>:<latency>`.
struct Works;

impl crate::plugin::FxFactory for Works {
    fn installed(&self) -> Vec<daw_proto::fx::InstalledFx> {
        Vec::new()
    }
    fn provides(&self, name: &str) -> bool {
        name.starts_with("Work:")
    }
    fn create(&self, name: &str, _: f64) -> Option<Box<dyn crate::plugin::PluginInstance>> {
        let mut parts = name.strip_prefix("Work:")?.split(':');
        let stages = parts.next()?.parse().ok()?;
        let latency = parts.next()?.parse().ok()?;
        Some(Box::new(Work::new(stages, latency)))
    }
}

/// Noise of its own for each take, `seconds` long.
fn noise(seed: u32, seconds: f64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..(seconds * f64::from(RATE)) as usize)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 - 0.5) * 0.4
        })
        .collect()
}

/// The song: `kits` drum folders, each a Kit folder holding a Kick folder
/// (In, Out), a Snare folder (Top, Bottom), OH and Rooms; a Trig keying
/// each Kick In; a Verb bus every Snare Top sends to pre-FX, every Kick
/// folder post-fader and every Rooms post-FX; and a Bass. Every track has
/// a `Work` of `stages` (more on the folders), two of them latent.
fn kit_song(kits: usize, stages: usize, seconds: f64) -> (Standalone, String) {
    let daw = Standalone::new();
    daw.set_fx_factory(Arc::new(Works));
    let guid = daw.seed_project(ProjectInfo {
        guid: "sched".into(),
        name: "sched".into(),
        path: String::new(),
    });
    let mut stems = Vec::new();
    for k in 0..kits {
        let kick = format!("Kick {k}");
        let snare = format!("Snare {k}");
        let kit = format!("Kit {k}");
        stems.push(StemSpec::new(
            format!("Kick In {k}"),
            "/x/a.wav",
            Some(&kick),
        ));
        stems.push(StemSpec::new(
            format!("Kick Out {k}"),
            "/x/b.wav",
            Some(&kick),
        ));
        stems.push(StemSpec::new(
            format!("Snare Top {k}"),
            "/x/c.wav",
            Some(&snare),
        ));
        stems.push(StemSpec::new(
            format!("Snare Bot {k}"),
            "/x/d.wav",
            Some(&snare),
        ));
        stems.push(StemSpec::new(format!("OH {k}"), "/x/e.wav", Some(&kit)));
        stems.push(StemSpec::new(format!("Rooms {k}"), "/x/f.wav", Some(&kit)));
        stems.push(StemSpec::new(format!("Trig {k}"), "/x/g.wav", None));
    }
    stems.push(StemSpec::new("Verb", "/x/v.wav", None));
    stems.push(StemSpec::new("Bass", "/x/h.wav", None));
    seed_media_tracks(&daw, &guid, &stems, 0.0, seconds);
    // The Kick and Snare folders inside their Kit.
    daw.write_project(&guid, |p| {
        for k in 0..kits {
            let kit = p
                .tracks
                .iter()
                .find(|t| t.name == format!("Kit {k}"))
                .map(|t| t.guid.clone());
            for t in &mut p.tracks {
                if t.name == format!("Kick {k}") || t.name == format!("Snare {k}") {
                    t.parent_guid = kit.clone();
                }
            }
        }
    });
    // Each take's noise seeded by its track's name: the same song every run
    // (the takes' map has no order of its own).
    let mut takes: Vec<(String, String)> = daw
        .read_project(&guid, |p| {
            p.takes
                .iter()
                .flat_map(|(item, tl)| {
                    let track = p
                        .items
                        .get(item)
                        .and_then(|e| p.tracks.iter().find(|t| t.guid == e.item.track_guid))
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    tl.takes
                        .iter()
                        .map(move |t| (track.clone(), t.guid.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    takes.sort();
    for (i, (_, tg)) in takes.iter().enumerate() {
        attach_audio_source(
            &daw,
            &guid,
            tg,
            DecodedAudio::new(noise(i as u32 + 1, seconds.min(8.0)), 1, RATE),
        );
    }
    let ctx = ProjectContext::Project(guid.clone());
    let track = |n: &str| TrackRef::Guid(guid_of(&daw, &guid, n));
    let fx = |n: &str, stages: usize, latency: usize| {
        Effects::add(
            &daw,
            ctx.clone(),
            FxChainContext::Track(guid_of(&daw, &guid, n)),
            &format!("Work:{stages}:{latency}"),
        )
        .expect("the fx");
    };
    for k in 0..kits {
        fx(&format!("Kick In {k}"), stages, 0);
        fx(&format!("Kick Out {k}"), stages, 64);
        fx(&format!("Snare Top {k}"), stages, 32);
        fx(&format!("Snare Bot {k}"), stages, 0);
        fx(&format!("OH {k}"), stages, 0);
        fx(&format!("Rooms {k}"), stages, 0);
        fx(&format!("Kick {k}"), stages * 2, 0);
        fx(&format!("Snare {k}"), stages * 2, 0);
        fx(&format!("Kit {k}"), stages * 3, 0);
        let send = |from: &str, to: &str, mode: SendMode| {
            let s = Routing::add_send(&daw, ctx.clone(), track(from), track(to)).expect("a send");
            Routing::set_send_mode(&daw, ctx.clone(), track(from), RouteRef::Index(s), mode)
                .unwrap();
            s
        };
        send(&format!("Snare Top {k}"), "Verb", SendMode::PreFx);
        send(&format!("Rooms {k}"), "Verb", SendMode::PostFx);
        let s = send(&format!("Kick {k}"), "Verb", SendMode::PostFader);
        let at = |n: String, s: u32| RouteLocation {
            track: track(&n),
            route_type: RouteType::Send,
            route: RouteRef::Index(s),
        };
        Routing::set_volume(&daw, ctx.clone(), at(format!("Kick {k}"), s), 0.5).unwrap();
        Routing::set_pan(&daw, ctx.clone(), at(format!("Kick {k}"), s), -0.3).unwrap();
        let key = send(
            &format!("Trig {k}"),
            &format!("Kick In {k}"),
            SendMode::PostFader,
        );
        Routing::set_dest_channels(&daw, ctx.clone(), at(format!("Trig {k}"), key), 2, 2)
            .expect("the key");
        Routing::set_parent_send_enabled(&daw, ctx.clone(), track(&format!("Trig {k}")), false)
            .unwrap();
        Tracks::set_volume(&daw, ctx.clone(), track(&format!("OH {k}")), 0.7).unwrap();
        Tracks::set_pan(&daw, ctx.clone(), track(&format!("Snare Bot {k}")), 0.4).unwrap();
        Tracks::set_phase_inverted(&daw, ctx.clone(), track(&format!("Snare Bot {k}")), true)
            .unwrap();
    }
    fx("Verb", stages * 2, 0);
    fx("Bass", stages, 0);
    daw.write_project(&guid, |p| p.master_volume = 0.6);
    (daw, guid)
}

/// The master and every track's post-fader tap, `blocks` blocks of
/// `frames`, from a fresh offline render on `threads` threads (`None`:
/// the machine's default).
fn render(
    daw: &Standalone,
    guid: &str,
    threads: Option<usize>,
    blocks: usize,
    frames: usize,
) -> (Vec<f32>, Vec<f32>) {
    let renderer = ProjectRenderer::offline(
        daw,
        guid,
        RATE,
        Offline {
            plugins: offline::copy_plugins(daw, guid, offline::Copying::default()).0,
            tap: Some(Tap::PostFader),
            ..Offline::default()
        },
    );
    if let Some(n) = threads {
        renderer.set_render_threads(n);
    }
    renderer.prime(frames as u32);
    let (mut master, mut taps) = (Vec::new(), Vec::new());
    for b in 0..blocks {
        master.extend(renderer.render_block((b * frames) as u64, frames).samples);
        renderer.with_taps(|t| t.iter().for_each(|t| taps.extend_from_slice(t)));
    }
    (master, taps)
}

/// The bits of every sample, as one number.
fn print(samples: &[f32]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for s in samples {
        s.to_bits().hash(&mut h);
    }
    h.finish()
}

#[test]
fn a_render_on_many_threads_is_the_render_on_one_sample_for_sample() {
    let (daw, guid) = kit_song(3, 4, 2.0);
    let (one, one_taps) = render(&daw, &guid, Some(0), 40, 512);
    let rms = (one.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / one.len() as f64).sqrt();
    assert!(rms > 0.01, "the song is heard (rms {rms})");
    for threads in [1, 3, 7] {
        let (many, many_taps) = render(&daw, &guid, Some(threads), 40, 512);
        assert!(
            one.iter()
                .zip(&many)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{threads} workers: master differs from the serial render"
        );
        assert!(
            one_taps
                .iter()
                .zip(&many_taps)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{threads} workers: a track's tap differs from the serial render"
        );
    }
}

/// What this song rendered to before the scheduler (one thread, the topo
/// loop): the scheduler's renders must be these very samples. (Taken again
/// when delay lines came to fade as their delay changes: this song's
/// latencies are learnt in its first block.)
const BEFORE_MASTER: u64 = 4_633_907_255_700_715_687;
const BEFORE_TAPS: u64 = 17_720_041_530_141_603_407;

#[test]
fn the_scheduler_renders_what_the_topo_loop_rendered() {
    let (daw, guid) = kit_song(3, 4, 2.0);
    let (master, taps) = render(&daw, &guid, Some(0), 40, 512);
    let (pm, pt) = (print(&master), print(&taps));
    let (master, taps) = render(&daw, &guid, Some(5), 40, 512);
    assert_eq!((print(&master), print(&taps)), (pm, pt));
    assert_eq!(
        (pm, pt),
        (BEFORE_MASTER, BEFORE_TAPS),
        "the render moved from the topo loop's"
    );
}

#[test]
#[ignore = "a measurement: run by hand"]
fn render_scheduler_bench() {
    let kits: usize = std::env::var("BENCH_KITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    let stages: usize = std::env::var("BENCH_STAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48);
    let seconds = 20.0;
    let (daw, guid) = kit_song(kits, stages, seconds);
    let tracks = daw.read_project(&guid, |p| p.tracks.len()).unwrap_or(0);
    let blocks = (seconds * f64::from(RATE)) as usize / 1_024;
    let mut report = format!(
        "{tracks} tracks ({kits} kits, {stages} stages a leaf FX), {seconds}s in {blocks} blocks of 1024\n"
    );
    let mut serial_ms = 0.0;
    let threads: Vec<Option<usize>> = std::env::var("BENCH_THREADS")
        .ok()
        .map(|v| {
            v.split(',')
                .filter_map(|n| n.parse().ok())
                .map(Some)
                .collect()
        })
        .unwrap_or_else(|| vec![Some(0), None]);
    for t in threads {
        let began = Instant::now();
        let (master, _) = render(&daw, &guid, t, blocks, 1_024);
        let ms = began.elapsed().as_secs_f64() * 1e3;
        if t == Some(0) {
            serial_ms = ms;
        }
        let speedup = if serial_ms > 0.0 { serial_ms / ms } else { 1.0 };
        report.push_str(&format!(
            "  workers {:>7}: {ms:>8.0} ms  {:>5.1}x realtime  {speedup:.2}x  (print {:x})\n",
            t.map_or("default".to_owned(), |n| n.to_string()),
            seconds * 1e3 / ms,
            print(&master)
        ));
    }
    std::io::Write::write_all(&mut std::io::stderr(), report.as_bytes()).ok();
}

#[test]
fn a_realtime_render_is_the_offline_render_sample_for_sample() {
    let blocks = |renderer: &ProjectRenderer| -> Vec<f32> {
        (0..40)
            .flat_map(|b| renderer.render_block(b * 256, 256).samples)
            .collect()
    };
    // Each on a song of its own: both play the song's own plugins.
    let (daw, guid) = kit_song(2, 4, 1.0);
    let offline = ProjectRenderer::new(&daw, &guid, RATE);
    offline.set_render_threads(0);
    let one = blocks(&offline);
    let (daw, guid) = kit_song(2, 4, 1.0);
    let live = ProjectRenderer::new(&daw, &guid, RATE);
    live.set_realtime();
    let many = blocks(&live);
    assert_eq!(one.len(), many.len());
    assert!(
        one.iter()
            .zip(&many)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "the realtime render (its own pool) differs from the serial offline one"
    );
}
