//! Render a small project through `ProjectRenderer`, exercising:
//! - Item playback (with fade-in/out)
//! - Track volume / pan / mute / solo
//! - Track-to-track sends
//! - parent_send_enabled
//!
//! No cpal involved — pure number-crunching, so this test works the
//! same on native and WASM.

#![cfg(feature = "decode")]

use daw_proto::midi::Midi;
use daw_proto::primitives::Duration as ProtoDuration;
use daw_proto::project::ProjectContext;
use daw_proto::{ItemRef, Items, ProjectInfo, RouteRef, RouteType, Routing, TrackRef, Tracks};
use daw_standalone::audio_engine::DecodedAudio;
use daw_standalone::audio_engine::materialize::attach_audio_source;
use daw_standalone::audio_engine::render::ProjectRenderer;
use daw_standalone::sync::Standalone;

const SAMPLE_RATE: u32 = 48_000;

fn seeded() -> (Standalone, String) {
    let daw = Standalone::new();
    let guid = daw.seed_project(ProjectInfo {
        guid: "p".into(),
        name: "p".into(),
        path: String::new(),
    });
    (daw, guid)
}

/// Synth a 1s constant-amplitude mono buffer at value `v`.
fn const_audio(v: f32) -> DecodedAudio {
    let frames = SAMPLE_RATE as usize;
    DecodedAudio {
        samples: vec![v; frames],
        channels: 1,
        sample_rate: SAMPLE_RATE,
    }
}

fn create_item_with_audio(
    daw: &Standalone,
    project_guid: &str,
    track_guid: &str,
    start_seconds: f64,
    length_seconds: f64,
    audio: DecodedAudio,
) -> (String, String) {
    let ctx = ProjectContext::Project(project_guid.to_string());
    // Make a MIDI item then convert to audio so we can wire a take
    // GUID we control.
    let loc = Midi::create_midi_item(
        daw,
        ctx.clone(),
        TrackRef::Guid(track_guid.to_string()),
        start_seconds,
        start_seconds + length_seconds,
    )
    .expect("midi item created");
    let item_guid = match &loc.item {
        ItemRef::Guid(g) => g.clone(),
        _ => panic!(),
    };
    // Flip the active take to audio + attach our synthetic source.
    let active =
        daw_proto::Takes::get_active_take(daw, ctx.clone(), ItemRef::Guid(item_guid.clone()))
            .unwrap();
    daw.write_project(project_guid, |p| {
        for tl in p.takes.values_mut() {
            for t in tl.takes.iter_mut() {
                if t.guid == active.guid {
                    t.is_midi = false;
                    t.source_type = daw_proto::item::SourceType::Audio;
                    t.source_file_path = None; // already decoded
                }
            }
        }
    });
    attach_audio_source(daw, project_guid, &active.guid, audio);
    (item_guid, active.guid)
}

fn rms_l(buf: &daw_standalone::audio_engine::render::StereoBuffer) -> f32 {
    let mut s = 0.0;
    for i in 0..buf.frames {
        let x = buf.samples[i * 2] as f64;
        s += x * x;
    }
    ((s / buf.frames.max(1) as f64).sqrt()) as f32
}

fn rms_r(buf: &daw_standalone::audio_engine::render::StereoBuffer) -> f32 {
    let mut s = 0.0;
    for i in 0..buf.frames {
        let x = buf.samples[i * 2 + 1] as f64;
        s += x * x;
    }
    ((s / buf.frames.max(1) as f64).sqrt()) as f32
}

#[test]
fn renders_single_item_to_master() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx, "T", None).unwrap();
    create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(0.5));

    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    // 0.5 s of audio.
    let block = r.render_block(0, (SAMPLE_RATE / 2) as usize);
    // Constant 0.5 → after center pan + unity volume the L/R each
    // see 0.5 * sqrt(0.5) = ~0.354.
    let target = 0.5 * (0.5_f32).sqrt();
    assert!(
        (rms_l(&block) - target).abs() < 0.05,
        "L rms={}, target={target}",
        rms_l(&block)
    );
    assert!(
        (rms_r(&block) - target).abs() < 0.05,
        "R rms={}, target={target}",
        rms_r(&block)
    );
}

#[test]
fn track_pan_routes_to_correct_side() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(0.5));
    Tracks::set_pan(&daw, ctx, TrackRef::Guid(t), 1.0).unwrap(); // hard right

    let block =
        ProjectRenderer::new(&daw, &guid, SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 2);
    assert!(rms_l(&block) < 0.01, "L should be silent on hard-right pan");
    assert!(
        rms_r(&block) > 0.4,
        "R should be loud, got {}",
        rms_r(&block)
    );
}

#[test]
fn muted_track_contributes_silence() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(0.5));
    Tracks::set_muted(&daw, ctx, TrackRef::Guid(t), true).unwrap();

    let block =
        ProjectRenderer::new(&daw, &guid, SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&block) < 0.001);
    assert!(rms_r(&block) < 0.001);
}

#[test]
fn solo_isolates_track() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let a = Tracks::add(&daw, ctx.clone(), "A", None).unwrap();
    let b = Tracks::add(&daw, ctx.clone(), "B", None).unwrap();
    create_item_with_audio(&daw, &guid, &a, 0.0, 1.0, const_audio(0.5));
    create_item_with_audio(&daw, &guid, &b, 0.0, 1.0, const_audio(0.5));

    Tracks::set_soloed(&daw, ctx, TrackRef::Guid(a), true).unwrap();
    let block =
        ProjectRenderer::new(&daw, &guid, SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    // Only A contributes; B's content is suppressed.
    let l = rms_l(&block);
    let target = 0.5 * (0.5_f32).sqrt();
    assert!((l - target).abs() < 0.05, "solo L rms={l}, target={target}");
}

#[test]
fn fade_in_attenuates_block_start() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    let (item, _take) = create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(1.0));
    // 200ms linear fade-in.
    Items::set_fade_in(
        &daw,
        ctx,
        ItemRef::Guid(item),
        ProtoDuration::from_seconds(0.2),
        daw_proto::item::FadeShape::Linear,
    )
    .unwrap();

    // Render 100ms at the very start — fade environment is 0 → 0.5.
    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    let early = r.render_block(0, SAMPLE_RATE as usize / 10); // 100ms
    // Render 100ms past the fade-in — full level.
    let later = r.render_block((SAMPLE_RATE as u64) * 3 / 10, SAMPLE_RATE as usize / 10); // start at 300ms

    let target = (0.5_f32).sqrt(); // gain 1.0 then center pan
    assert!(
        rms_l(&early) < rms_l(&later),
        "fade-in early ({}) should be quieter than past-fade ({})",
        rms_l(&early),
        rms_l(&later)
    );
    assert!(
        rms_l(&later) > target * 0.8,
        "past-fade L should approach unity"
    );
}

#[test]
fn send_routes_audio_into_destination_bus() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let src = Tracks::add(&daw, ctx.clone(), "Src", None).unwrap();
    let bus = Tracks::add(&daw, ctx.clone(), "Bus", None).unwrap();
    create_item_with_audio(&daw, &guid, &src, 0.0, 1.0, const_audio(1.0));

    // Disable Src's parent send so it ONLY reaches master via Bus.
    Routing::set_parent_send_enabled(&daw, ctx.clone(), TrackRef::Guid(src.clone()), false)
        .unwrap();

    // Without the send: master is silent (Src has no parent send, Bus
    // has no items).
    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    let pre = r.render_block(0, SAMPLE_RATE as usize / 4);
    assert!(
        rms_l(&pre) < 0.001 && rms_r(&pre) < 0.001,
        "expected silent master without send"
    );

    // Add send Src → Bus.
    Routing::add_send(
        &daw,
        ctx.clone(),
        TrackRef::Guid(src.clone()),
        TrackRef::Guid(bus.clone()),
    )
    .unwrap();

    let post = r.render_block(0, SAMPLE_RATE as usize / 4);
    assert!(
        rms_l(&post) > 0.1,
        "send should bring audio to master via Bus"
    );

    // Mute the send → master goes silent again.
    Routing::set_muted(
        &daw,
        ctx,
        daw_proto::RouteLocation {
            track: TrackRef::Guid(src),
            route_type: RouteType::Send,
            route: RouteRef::Index(0),
        },
        true,
    )
    .unwrap();
    let muted = r.render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&muted) < 0.001, "muted send should be silent");
}

#[test]
fn parent_send_disabled_excludes_track_from_master() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(1.0));
    Routing::set_parent_send_enabled(&daw, ctx, TrackRef::Guid(t), false).unwrap();

    let block =
        ProjectRenderer::new(&daw, &guid, SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&block) < 0.001);
}

#[test]
fn volume_envelope_attenuates_master() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeType};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Volume envelope ramping 1.0 → 0.0 over the second.
    let loc = EnvelopeLocation::new(
        TrackRef::Guid(t.clone()),
        EnvelopeRef::Type(EnvelopeType::Volume),
    );
    Automation::add_point(
        &daw,
        ctx.clone(),
        loc.clone(),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    Automation::add_point(
        &daw,
        ctx.clone(),
        loc,
        AddPointParams::linear(PositionInSeconds::from_seconds(1.0), 0.0),
    );

    let r = ProjectRenderer::new(&daw, "p", SAMPLE_RATE);
    // First 100ms — envelope ≈ 1.0, full volume.
    let early = r.render_block(0, SAMPLE_RATE as usize / 10);
    // Last 100ms — envelope ≈ 0.0, near silence.
    let late = r.render_block(SAMPLE_RATE as u64 * 9 / 10, SAMPLE_RATE as usize / 10);

    assert!(
        rms_l(&early) > rms_l(&late) + 0.05,
        "early ({}) should be louder than late ({})",
        rms_l(&early),
        rms_l(&late)
    );
}

#[test]
fn pan_envelope_routes_to_right_side() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeType};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Pan envelope held at 1.0 (hard right).
    let loc = EnvelopeLocation::new(TrackRef::Guid(t), EnvelopeRef::Type(EnvelopeType::Pan));
    Automation::add_point(
        &daw,
        ctx,
        loc,
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );

    let r = ProjectRenderer::new(&daw, "p", SAMPLE_RATE);
    let block = r.render_block(0, SAMPLE_RATE as usize / 4);
    assert!(
        rms_l(&block) < 0.05,
        "hard-right pan env should silence L, got {}",
        rms_l(&block)
    );
    assert!(
        rms_r(&block) > 0.5,
        "R should be loud, got {}",
        rms_r(&block)
    );
}

#[test]
fn mute_envelope_gates_output() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeType};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Mute envelope held above 0.5 = muted.
    let loc = EnvelopeLocation::new(TrackRef::Guid(t), EnvelopeRef::Type(EnvelopeType::Mute));
    Automation::add_point(
        &daw,
        ctx,
        loc,
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );

    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&block) < 0.001);
    assert!(rms_r(&block) < 0.001);
}

#[test]
fn send_volume_envelope_rides_send_level() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, SendEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let src = Tracks::add(&daw, ctx.clone(), "Src", None).unwrap();
    let bus = Tracks::add(&daw, ctx.clone(), "Bus", None).unwrap();
    create_item_with_audio(&daw, "p", &src, 0.0, 1.0, const_audio(1.0));
    // Route ONLY through the bus.
    Routing::set_parent_send_enabled(&daw, ctx.clone(), TrackRef::Guid(src.clone()), false)
        .unwrap();
    Routing::add_send(
        &daw,
        ctx.clone(),
        TrackRef::Guid(src.clone()),
        TrackRef::Guid(bus),
    )
    .unwrap();

    // Send vol envelope: full at t=0, fades to silence at t=1.
    let loc = EnvelopeLocation::new(
        TrackRef::Guid(src),
        EnvelopeRef::Send {
            send_index: 0,
            kind: SendEnvelopeKind::Volume,
        },
    );
    Automation::add_point(
        &daw,
        ctx.clone(),
        loc.clone(),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    Automation::add_point(
        &daw,
        ctx,
        loc,
        AddPointParams::linear(PositionInSeconds::from_seconds(1.0), 0.0),
    );

    let r = ProjectRenderer::new(&daw, "p", SAMPLE_RATE);
    let early = r.render_block(0, SAMPLE_RATE as usize / 10);
    let late = r.render_block(SAMPLE_RATE as u64 * 9 / 10, SAMPLE_RATE as usize / 10);
    assert!(
        rms_l(&early) > rms_l(&late) + 0.05,
        "send vol env: early={}, late={}",
        rms_l(&early),
        rms_l(&late)
    );
}

#[test]
fn send_mute_envelope_silences_send() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, SendEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let src = Tracks::add(&daw, ctx.clone(), "Src", None).unwrap();
    let bus = Tracks::add(&daw, ctx.clone(), "Bus", None).unwrap();
    create_item_with_audio(&daw, "p", &src, 0.0, 1.0, const_audio(1.0));
    Routing::set_parent_send_enabled(&daw, ctx.clone(), TrackRef::Guid(src.clone()), false)
        .unwrap();
    Routing::add_send(
        &daw,
        ctx.clone(),
        TrackRef::Guid(src.clone()),
        TrackRef::Guid(bus),
    )
    .unwrap();

    Automation::add_point(
        &daw,
        ctx,
        EnvelopeLocation::new(
            TrackRef::Guid(src),
            EnvelopeRef::Send {
                send_index: 0,
                kind: SendEnvelopeKind::Mute,
            },
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&block) < 0.001, "send-muted master should be silent");
}

#[test]
fn send_pan_envelope_steers_send() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, SendEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let src = Tracks::add(&daw, ctx.clone(), "Src", None).unwrap();
    let bus = Tracks::add(&daw, ctx.clone(), "Bus", None).unwrap();
    create_item_with_audio(&daw, "p", &src, 0.0, 1.0, const_audio(1.0));
    Routing::set_parent_send_enabled(&daw, ctx.clone(), TrackRef::Guid(src.clone()), false)
        .unwrap();
    Routing::add_send(
        &daw,
        ctx.clone(),
        TrackRef::Guid(src.clone()),
        TrackRef::Guid(bus),
    )
    .unwrap();

    Automation::add_point(
        &daw,
        ctx,
        EnvelopeLocation::new(
            TrackRef::Guid(src),
            EnvelopeRef::Send {
                send_index: 0,
                kind: SendEnvelopeKind::Pan,
            },
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0), // hard right
    );
    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(
        rms_l(&block) < 0.05,
        "hard-right send pan: L should be quiet"
    );
    assert!(rms_r(&block) > 0.3, "hard-right send pan: R audible");
}

#[test]
fn volume_prefx_envelope_stacks_with_main_volume_envelope() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeType};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Main env at 0.5 + PreFX env at 0.5 → combined gain 0.25.
    Automation::add_point(
        &daw,
        ctx.clone(),
        EnvelopeLocation::new(
            TrackRef::Guid(t.clone()),
            EnvelopeRef::Type(EnvelopeType::Volume),
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 0.5),
    );
    Automation::add_point(
        &daw,
        ctx,
        EnvelopeLocation::new(
            TrackRef::Guid(t),
            EnvelopeRef::Type(EnvelopeType::VolumePrefx),
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 0.5),
    );
    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    // After center pan: gain 0.25 × sqrt(0.5) ≈ 0.177.
    let target = 0.25 * (0.5_f32).sqrt();
    assert!(
        (rms_l(&block) - target).abs() < 0.05,
        "stacked vol envs: L rms={}, target={target}",
        rms_l(&block)
    );
}

#[test]
fn take_volume_envelope_attenuates_item() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, TakeEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    let (item_guid, take_guid) = create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Take vol envelope: 1.0 at item start, 0.0 at end.
    let loc = EnvelopeLocation::new(
        TrackRef::Guid(t),
        EnvelopeRef::Take {
            item_guid,
            take_guid,
            kind: TakeEnvelopeKind::Volume,
        },
    );
    Automation::add_point(
        &daw,
        ctx.clone(),
        loc.clone(),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    Automation::add_point(
        &daw,
        ctx,
        loc,
        AddPointParams::linear(PositionInSeconds::from_seconds(1.0), 0.0),
    );

    let r = ProjectRenderer::new(&daw, "p", SAMPLE_RATE);
    let early = r.render_block(0, SAMPLE_RATE as usize / 10);
    let late = r.render_block(SAMPLE_RATE as u64 * 9 / 10, SAMPLE_RATE as usize / 10);
    assert!(
        rms_l(&early) > rms_l(&late) + 0.05,
        "take vol env: early={}, late={}",
        rms_l(&early),
        rms_l(&late)
    );
}

#[test]
fn take_mute_envelope_silences_item() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, TakeEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    let (item_guid, take_guid) = create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    Automation::add_point(
        &daw,
        ctx,
        EnvelopeLocation::new(
            TrackRef::Guid(t),
            EnvelopeRef::Take {
                item_guid,
                take_guid,
                kind: TakeEnvelopeKind::Mute,
            },
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&block) < 0.001);
    assert!(rms_r(&block) < 0.001);
}

#[test]
fn take_pan_envelope_pans_item_within_track() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, TakeEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    let (item_guid, take_guid) = create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Hard right take pan.
    Automation::add_point(
        &daw,
        ctx,
        EnvelopeLocation::new(
            TrackRef::Guid(t),
            EnvelopeRef::Take {
                item_guid,
                take_guid,
                kind: TakeEnvelopeKind::Pan,
            },
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 4);
    assert!(
        rms_l(&block) < 0.05,
        "L should be near-silent, got {}",
        rms_l(&block)
    );
    assert!(
        rms_r(&block) > 0.3,
        "R should be audible, got {}",
        rms_r(&block)
    );
}

#[test]
fn take_pitch_envelope_shifts_play_rate() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, TakeEnvelopeKind};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    // Item is 1s long; source is also 1s of constant audio. With
    // +12 semitones, source advances 2x → after 0.5s of wall time
    // we've consumed all 1s of the source.
    let (item_guid, take_guid) = create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    Automation::add_point(
        &daw,
        ctx.clone(),
        EnvelopeLocation::new(
            TrackRef::Guid(t.clone()),
            EnvelopeRef::Take {
                item_guid: item_guid.clone(),
                take_guid: take_guid.clone(),
                kind: TakeEnvelopeKind::Pitch,
            },
        ),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 12.0),
    );
    // Sanity: the envelope shows up via the proto getter.
    let points = Automation::points(
        &daw,
        ctx,
        EnvelopeLocation::new(
            TrackRef::Guid(t),
            EnvelopeRef::Take {
                item_guid,
                take_guid,
                kind: TakeEnvelopeKind::Pitch,
            },
        ),
    );
    assert_eq!(
        points.len(),
        1,
        "pitch env should have 1 point, got {points:?}"
    );
    assert!((points[0].value - 12.0).abs() < 1e-6);

    let r = ProjectRenderer::new(&daw, "p", SAMPLE_RATE);
    // First half-second: source is in range, audible.
    let first = r.render_block(0, SAMPLE_RATE as usize / 8);
    assert!(
        rms_l(&first) > 0.1,
        "early block with +12st pitch should still be audible"
    );
    // After 0.6s (past the source-exhausted point at 0.5s) source
    // index is past the end and frames are skipped — much quieter.
    let late = r.render_block(SAMPLE_RATE as u64 * 6 / 10, SAMPLE_RATE as usize / 8);
    assert!(
        rms_l(&late) < rms_l(&first) * 0.5 + 0.01,
        "post-source-exhaust block ({}) should be much quieter than early ({})",
        rms_l(&late),
        rms_l(&first)
    );
}

#[test]
fn automation_mode_off_bypasses_envelope() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeType};
    use daw_proto::primitives::{AutomationMode, PositionInSeconds};

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    let loc = EnvelopeLocation::new(
        TrackRef::Guid(t.clone()),
        EnvelopeRef::Type(EnvelopeType::Volume),
    );
    // Envelope ramps to silence — but with mode=Off it should be
    // ignored, and full volume should reach the master.
    Automation::add_point(
        &daw,
        ctx.clone(),
        loc.clone(),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 0.0),
    );
    Automation::set_automation_mode(&daw, ctx.clone(), loc, AutomationMode::Off);

    let block =
        ProjectRenderer::new(&daw, "p", SAMPLE_RATE).render_block(0, SAMPLE_RATE as usize / 10);
    // Without Off this would be silent; with Off the static volume
    // (1.0) passes through.
    let target = (0.5_f32).sqrt(); // const 1.0 audio × center pan
    assert!(
        (rms_l(&block) - target).abs() < 0.05,
        "mode=Off should bypass envelope: L rms={}, target≈{target}",
        rms_l(&block)
    );
}

#[test]
fn envelope_eval_is_per_sample_not_block() {
    use daw_proto::Automation;
    use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeType};
    use daw_proto::primitives::PositionInSeconds;

    let (daw, guid) = seeded();
    let ctx = ProjectContext::Project(guid);
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, "p", &t, 0.0, 1.0, const_audio(1.0));

    // Ramp 1.0 → 0.0 across 10ms. With block-midpoint eval over a
    // 100ms block, both halves would see a single midpoint value
    // (~0.5) and rms would be uniform. With per-sample eval, the
    // first-half rms (≈0.75) is clearly louder than the second-half
    // rms (≈0.25).
    let loc = EnvelopeLocation::new(TrackRef::Guid(t), EnvelopeRef::Type(EnvelopeType::Volume));
    Automation::add_point(
        &daw,
        ctx.clone(),
        loc.clone(),
        AddPointParams::linear(PositionInSeconds::from_seconds(0.0), 1.0),
    );
    Automation::add_point(
        &daw,
        ctx,
        loc,
        AddPointParams::linear(PositionInSeconds::from_seconds(0.01), 0.0),
    );

    let r = ProjectRenderer::new(&daw, "p", SAMPLE_RATE);
    // Render the first 100ms in two halves and compare.
    let half = SAMPLE_RATE as usize / 20; // 50ms
    let a = r.render_block(0, half);
    let b = r.render_block(half as u64, half);
    // First half hears the full ramp (loud) then silence. Second
    // half is fully past the ramp (silent).
    assert!(
        rms_l(&a) > rms_l(&b) + 0.05,
        "per-sample eval: first half ({}) should be louder than second ({})",
        rms_l(&a),
        rms_l(&b)
    );
    assert!(rms_l(&b) < 0.02, "post-ramp half should be near-silent");
}

#[test]
fn item_position_shifts_into_block() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx, "T", None).unwrap();
    // Item starts at 0.5s, lasts 0.5s.
    create_item_with_audio(&daw, &guid, &t, 0.5, 0.5, const_audio(1.0));

    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    // Render first 0.25s — silent.
    let early = r.render_block(0, SAMPLE_RATE as usize / 4);
    assert!(rms_l(&early) < 0.001, "early block should be silent");
    // Render 0.5-0.75s — audible.
    let mid = r.render_block(SAMPLE_RATE as u64 / 2, SAMPLE_RATE as usize / 4);
    assert!(
        rms_l(&mid) > 0.3,
        "mid block should be audible: {}",
        rms_l(&mid)
    );
}

#[test]
fn render_writes_post_fader_track_meters() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx.clone(), "T", None).unwrap();
    create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(0.5));

    // Install a meter bank like AudioEngine::attached_to does.
    daw.set_meters(daw_standalone::metering::Meters::new(1));

    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    let _ = r.render_block(0, 512);
    let meters = daw.meters();
    let cell = meters.cell(0).expect("cell 0");
    // Constant 0.5, center pan, unity volume → ~0.354 per side.
    let target = 0.5 * (0.5_f32).sqrt();
    assert!(
        (cell.peak(0) - target).abs() < 0.05,
        "L peak={}, target={target}",
        cell.peak(0)
    );
    assert!((cell.peak(1) - target).abs() < 0.05);

    // Halve the fader: meter is post-fader, peak follows.
    Tracks::set_volume(&daw, ctx, TrackRef::Guid(t.clone()), 0.5).unwrap();
    let _ = r.render_block(0, 512);
    assert!(
        (meters.cell(0).unwrap().peak(0) - target * 0.5).abs() < 0.05,
        "post-fader peak={}",
        meters.cell(0).unwrap().peak(0)
    );
}

#[test]
fn loop_source_tiles_past_source_end() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx, "T", None).unwrap();
    // 2s item over a 1s source (const_audio is 1s) — the second half
    // only sounds when LOOP is honored.
    let (item_guid, _take) = create_item_with_audio(&daw, &guid, &t, 0.0, 2.0, const_audio(0.5));
    daw.write_project(&guid, |p| {
        p.items.get_mut(&item_guid).unwrap().item.loop_source = true;
    });

    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    // Render half a second starting at 1.5s — past the source length.
    let block = r.render_block(
        (SAMPLE_RATE + SAMPLE_RATE / 2) as u64,
        (SAMPLE_RATE / 4) as usize,
    );
    let target = 0.5 * (0.5_f32).sqrt();
    assert!(
        (rms_l(&block) - target).abs() < 0.05,
        "looped tail should sound: rms={}",
        rms_l(&block)
    );

    // Sanity: with loop off the same window is silent.
    daw.write_project(&guid, |p| {
        p.items.get_mut(&item_guid).unwrap().item.loop_source = false;
    });
    let block = r.render_block(
        (SAMPLE_RATE + SAMPLE_RATE / 2) as u64,
        (SAMPLE_RATE / 4) as usize,
    );
    assert!(rms_l(&block) < 1e-6, "unlooped tail must be silent");
}

#[test]
fn master_volume_pan_mute_apply() {
    let (daw, guid) = seeded();
    let ctx = ProjectContext::Current;
    let t = Tracks::add(&daw, ctx, "T", None).unwrap();
    create_item_with_audio(&daw, &guid, &t, 0.0, 1.0, const_audio(0.5));
    let r = ProjectRenderer::new(&daw, &guid, SAMPLE_RATE);
    let unity = 0.5 * (0.5_f32).sqrt();

    daw.write_project(&guid, |p| p.master_volume = 0.5);
    let block = r.render_block(0, 512);
    assert!(
        (rms_l(&block) - unity * 0.5).abs() < 0.05,
        "master gain halves output: rms={}",
        rms_l(&block)
    );

    daw.write_project(&guid, |p| p.master_muted = true);
    let block = r.render_block(0, 512);
    assert!(rms_l(&block) < 1e-6, "master mute silences output");
}
