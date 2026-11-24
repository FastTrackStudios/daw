//! Delay compensation and offline renders, end to end on the renderer.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::folder_routing_tests::guid_of;
use super::offline::{Copying, Offline, Tap, copy_plugins};
use super::*;
use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::materialize::attach_audio_source;
use crate::media_seed::{StemSpec, seed_media_tracks};
use daw_proto::fx::{Effects, FxChainContext};
use daw_proto::{ProjectContext, ProjectInfo, RouteRef, Routing};
use daw_proto::{TrackRef, Tracks};

/// Where every take's one click is, in samples.
const CLICK: usize = 1_000;
/// The test plugin's delay.
const LATE: usize = 64;

/// Blocks processed by a `Late` marked live (this file's offline test's
/// original, the one playback would run).
static LIVE_BLOCKS: AtomicU32 = AtomicU32::new(0);

/// A plugin that is `LATE` samples late and says so.
struct Late {
    line: Vec<[f32; 2]>,
    at: usize,
    live: bool,
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
        LATE as u32
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
        if self.live {
            LIVE_BLOCKS.fetch_add(1, Ordering::Relaxed);
        }
        for f in 0..in_l.len() {
            let [l, r] = self.line[self.at];
            self.line[self.at] = [in_l[f], in_r[f]];
            self.at = (self.at + 1) % LATE;
            out_l[f] = l;
            out_r[f] = r;
        }
        Ok(())
    }
    fn deactivate(&mut self) {}
}

struct Lates;

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
                line: vec![[0.0; 2]; LATE],
                at: 0,
                live: false,
            }) as Box<dyn crate::plugin::PluginInstance>
        })
    }
}

/// Drums{Kick, Snare} and a top-level Bus, every take one click at
/// [`CLICK`]; the Bus muted.
fn clicks() -> (Standalone, String) {
    let daw = Standalone::new();
    daw.set_fx_factory(Arc::new(Lates));
    let guid = daw.seed_project(ProjectInfo {
        guid: "pdc".into(),
        name: "pdc".into(),
        path: String::new(),
    });
    let stems = vec![
        StemSpec::new("Kick", "/x/kick.wav", Some("Drums")),
        StemSpec::new("Snare", "/x/snare.wav", Some("Drums")),
        StemSpec::new("Bus", "/x/bus.wav", None),
    ];
    seed_media_tracks(&daw, &guid, &stems, 0.0, 10.0);
    let takes: Vec<String> = daw
        .read_project(&guid, |p| {
            p.takes
                .values()
                .flat_map(|tl| tl.takes.iter().map(|t| t.guid.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut click = vec![0.0; 8192];
    click[CLICK] = 0.5;
    for tg in takes {
        attach_audio_source(
            &daw,
            &guid,
            &tg,
            DecodedAudio::new(click.clone(), 1, 48_000),
        );
    }
    let ctx = ProjectContext::Project(guid.clone());
    Tracks::set_muted(&daw, ctx, TrackRef::Guid(guid_of(&daw, &guid, "Bus")), true).unwrap();
    (daw, guid)
}

fn late_on(daw: &Standalone, guid: &str, track: &str) -> String {
    Effects::add(
        daw,
        ProjectContext::Project(guid.to_owned()),
        FxChainContext::Track(guid_of(daw, guid, track)),
        "Late",
    )
    .expect("the plugin")
}

/// `blocks` blocks of 128 from frame 0, master's left channel.
fn render(renderer: &ProjectRenderer, blocks: usize) -> Vec<f32> {
    (0..blocks)
        .flat_map(|b| {
            let out = renderer.render_block((b * 128) as u64, 128);
            out.samples
                .chunks_exact(2)
                .map(|f| f[0])
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Where the signal is, sample by sample.
fn hits(signal: &[f32]) -> Vec<(usize, f32)> {
    signal
        .iter()
        .enumerate()
        .filter(|(_, s)| s.abs() > 1e-6)
        .map(|(i, s)| (i, *s))
        .collect()
}

#[test]
fn two_tracks_of_different_latency_meet_sample_aligned_at_their_folder() {
    let (daw, guid) = clicks();
    late_on(&daw, &guid, "Kick");
    let renderer = ProjectRenderer::new(&daw, &guid, 48_000);
    // The first block learns the latencies; render on from the start.
    renderer.render_block(0, 128);
    let master = render(&renderer, 12);
    assert_eq!(
        renderer.latency(),
        LATE as u32,
        "the song is the plugin's delay late"
    );
    // One click, both tracks in it, LATE after where they are.
    assert_eq!(
        hits(&master),
        vec![(CLICK + LATE, 1.0)],
        "kick and snare meet"
    );
}

#[test]
fn without_compensation_they_would_not() {
    // The same song, the plugin's delay not reported: two clicks apart.
    let (daw, guid) = clicks();
    let fx = late_on(&daw, &guid, "Kick");
    daw.insert_plugin_instance(
        fx,
        Box::new(Unreported(Late {
            line: vec![[0.0; 2]; LATE],
            at: 0,
            live: false,
        })),
    );
    let renderer = ProjectRenderer::new(&daw, &guid, 48_000);
    renderer.render_block(0, 128);
    let master = render(&renderer, 12);
    assert_eq!(hits(&master), vec![(CLICK, 0.5), (CLICK + LATE, 0.5)]);
}

/// [`Late`], its delay unreported.
struct Unreported(Late);

impl crate::plugin::PluginInstance for Unreported {
    fn descriptor(&self) -> crate::plugin::PluginDescriptor {
        self.0.descriptor()
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
    fn process_block(
        &mut self,
        il: &[f32],
        ir: &[f32],
        ol: &mut [f32],
        or: &mut [f32],
        e: &crate::plugin::PluginEvents<'_>,
    ) -> Result<(), crate::plugin::PluginError> {
        self.0.process_block(il, ir, ol, or, e)
    }
    fn deactivate(&mut self) {}
}

#[test]
fn sends_into_a_bus_are_aligned_with_each_other_and_with_its_own_items() {
    let (daw, guid) = clicks();
    let ctx = ProjectContext::Project(guid.clone());
    let track = |n: &str| TrackRef::Guid(guid_of(&daw, &guid, n));
    late_on(&daw, &guid, "Kick");
    // Kick and Snare reach master only through the Bus, which has a click
    // of its own.
    Tracks::set_muted(&daw, ctx.clone(), track("Bus"), false).unwrap();
    Routing::set_parent_send_enabled(&daw, ctx.clone(), track("Drums"), false).unwrap();
    for n in ["Kick", "Snare"] {
        Routing::add_send(&daw, ctx.clone(), track(n), track("Bus")).expect("a send");
    }
    let renderer = ProjectRenderer::new(&daw, &guid, 48_000);
    renderer.render_block(0, 128);
    let master = render(&renderer, 12);
    // Kick (late), Snare and the Bus's own click, as one.
    assert_eq!(hits(&master), vec![(CLICK + LATE, 1.5)]);
}

#[test]
fn a_pre_fx_send_taps_before_the_latency() {
    let (daw, guid) = clicks();
    let ctx = ProjectContext::Project(guid.clone());
    let track = |n: &str| TrackRef::Guid(guid_of(&daw, &guid, n));
    late_on(&daw, &guid, "Kick");
    Tracks::set_muted(&daw, ctx.clone(), track("Bus"), false).unwrap();
    Tracks::set_muted(&daw, ctx.clone(), track("Drums"), true).unwrap();
    let send = Routing::add_send(&daw, ctx.clone(), track("Kick"), track("Bus")).expect("a send");
    Routing::set_send_mode(
        &daw,
        ctx.clone(),
        track("Kick"),
        RouteRef::Index(send),
        SendMode::PreFx,
    )
    .unwrap();
    let renderer = ProjectRenderer::new(&daw, &guid, 48_000);
    renderer.render_block(0, 128);
    let master = render(&renderer, 12);
    // The dry kick (tapped before its plugin) and the bus's click arrive
    // together. The song stays as late as its latest path, the muted
    // folder's included: a mute does not move the song in time.
    assert_eq!(hits(&master), vec![(CLICK + LATE, 1.0)]);
}

use daw_proto::SendMode;

#[test]
fn an_offline_render_runs_copies_and_leaves_the_live_plugins_alone() {
    let (daw, guid) = clicks();
    let fx = late_on(&daw, &guid, "Kick");
    daw.insert_plugin_instance(
        fx.clone(),
        Box::new(Late {
            line: vec![[0.0; 2]; LATE],
            at: 0,
            live: true,
        }),
    );
    let (plugins, copies) = copy_plugins(&daw, &guid, Copying::default());
    assert_eq!(copies.copied, 1);
    assert!(copies.failed.is_empty());
    let renderer = ProjectRenderer::offline(
        &daw,
        &guid,
        48_000,
        Offline {
            plugins,
            tap: Some(Tap::PostFader),
            ..Offline::default()
        },
    );
    assert_eq!(
        renderer.prime(128),
        LATE as u32,
        "primed, the latency is known at once"
    );
    let before = daw.with_plugin_instance(&fx, |p| p.is_prepared());
    let master = render(&renderer, 12);
    assert_eq!(
        hits(&master),
        vec![(CLICK + LATE, 1.0)],
        "compensated from the first block"
    );
    // The taps: each track's block, Kick late by its own chain.
    let guids = renderer.track_guids();
    let at = |n: &str| {
        guids
            .iter()
            .position(|g| *g == guid_of(&daw, &guid, n))
            .expect(n)
    };
    let lat = renderer.track_latencies();
    assert_eq!(lat[at("Kick")], LATE as u32);
    assert_eq!(lat[at("Snare")], 0);
    assert_eq!(lat[at("Drums")], LATE as u32);
    // The live instance ran no block of it: it hears nothing but playback.
    let ran = renderer.with_taps(|t| t.len());
    assert_eq!(ran, guids.len());
    assert_eq!(daw.with_plugin_instance(&fx, |p| p.is_prepared()), before);
    assert_eq!(LIVE_BLOCKS.load(Ordering::Relaxed), 0);
}

#[test]
fn an_offline_render_bypasses_and_solos_for_itself_only() {
    let (daw, guid) = clicks();
    let fx = late_on(&daw, &guid, "Kick");
    let (plugins, _) = copy_plugins(&daw, &guid, Copying::default());
    let snare = guid_of(&daw, &guid, "Snare");
    let renderer = ProjectRenderer::offline(
        &daw,
        &guid,
        48_000,
        Offline {
            plugins,
            bypass_fx: [fx.clone()].into(),
            solo: Some([snare].into()),
            tap: Some(Tap::PreFader),
            ..Offline::default()
        },
    );
    renderer.prime(128);
    let master = render(&renderer, 12);
    assert_eq!(
        hits(&master),
        vec![(CLICK, 0.5)],
        "the snare alone, nothing late"
    );
    // The song: its FX on, nothing soloed.
    let enabled = Effects::get(
        &daw,
        ProjectContext::Project(guid.clone()),
        daw_proto::fx::FxTarget {
            context: FxChainContext::Track(guid_of(&daw, &guid, "Kick")),
            fx: daw_proto::fx::FxRef::Guid(fx),
        },
    )
    .map(|f| f.enabled);
    assert_eq!(enabled, Some(true));
    let soloed = Tracks::all(&daw, ProjectContext::Project(guid.clone()))
        .iter()
        .any(|t| t.soloed);
    assert!(!soloed);
}
