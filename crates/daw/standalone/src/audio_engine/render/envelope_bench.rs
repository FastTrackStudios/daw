//! What take volume envelopes cost an offline render: a song of whole-song
//! takes, each with a leveler's curve (a few points a stroke), rendered
//! with and without it, and what one snapshot of it costs to build.
//!
//! `cargo test -p daw-standalone --release --lib envelope_bench -- --ignored --nocapture`

use std::time::Instant;

use super::ProjectRenderer;
use super::offline::Offline;
use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::materialize::attach_audio_source;
use crate::media_seed::{StemSpec, seed_media_tracks};
use crate::sync::Standalone;
use crate::sync::{EnvelopeData, EnvelopeKey};
use daw_proto::automation::{EnvelopePoint, EnvelopeShape};
use daw_proto::primitives::AutomationMode;
use daw_proto::{PositionInSeconds, ProjectInfo, TakeEnvelopeKind};

const TRACKS: usize = 12;
const SECONDS: f64 = 120.0;
const RATE: u32 = 48_000;
const BLOCK: usize = 1_024;
/// A stroke every 75 ms, three points each (ramp, attack, hold).
const STROKE: f64 = 0.075;

fn song(curves: bool) -> (Standalone, String) {
    let daw = Standalone::new();
    let guid = daw.seed_project(ProjectInfo {
        guid: "env-bench".into(),
        name: "EB".into(),
        path: String::new(),
    });
    let stems: Vec<StemSpec> = (0..TRACKS)
        .map(|i| StemSpec::new(&format!("Mic {i}"), &format!("/x/{i}.wav"), Some("Drums")))
        .collect();
    seed_media_tracks(&daw, &guid, &stems, 0.0, SECONDS);
    let takes: Vec<(String, String)> = daw
        .read_project(&guid, |p| {
            p.takes
                .iter()
                .flat_map(|(item, tl)| tl.takes.iter().map(|t| (item.clone(), t.guid.clone())))
                .collect()
        })
        .unwrap_or_default();
    let frames = (SECONDS * f64::from(RATE)) as usize + RATE as usize;
    for (_, take) in &takes {
        let audio: Vec<f32> = (0..frames)
            .map(|i| ((i % 97) as f32 / 97.0) - 0.5)
            .collect();
        attach_audio_source(&daw, &guid, take, DecodedAudio::new(audio, 1, RATE));
    }
    if curves {
        let points: Vec<EnvelopePoint> = (0..((SECONDS / STROKE) as usize))
            .flat_map(|s| {
                let at = s as f64 * STROKE;
                let gain = 0.6 + 0.3 * ((s % 7) as f64 / 7.0);
                [(at, 1.0), (at + 0.004, gain), (at + 0.06, gain)]
            })
            .enumerate()
            .map(|(i, (t, v))| EnvelopePoint {
                index: i as u32,
                time: PositionInSeconds::from_seconds(t),
                value: v,
                shape: EnvelopeShape::Linear,
                tension: 0.0,
                selected: false,
            })
            .collect();
        daw.write_project(&guid, |p| {
            for (item, take) in &takes {
                let mut data = EnvelopeData::new();
                data.automation_mode = AutomationMode::Read;
                data.points = points.clone();
                p.envelopes.insert(
                    (
                        String::new(),
                        EnvelopeKey::Take {
                            item_guid: item.clone(),
                            take_guid: take.clone(),
                            kind: TakeEnvelopeKind::Volume,
                        },
                    ),
                    data,
                );
            }
        });
    }
    (daw, guid)
}

/// Render the whole song: ms. `bump_every_block`: the song written to
/// before every block, rendered by a renderer that re-snapshots on every
/// revision (the song's own; an offline one did too before it kept its
/// first snapshot).
fn render(daw: &Standalone, guid: &str, bump_every_block: bool) -> f64 {
    let renderer = if bump_every_block {
        ProjectRenderer::new(daw, guid, RATE)
    } else {
        ProjectRenderer::offline(daw, guid, RATE, Offline::default())
    };
    let began = Instant::now();
    let end = (SECONDS * f64::from(RATE)) as usize;
    let mut at = 0;
    while at < end {
        if bump_every_block {
            // Anything written to the song (a window's edit, a step).
            daw.write_project(guid, |_| ());
        }
        let n = BLOCK.min(end - at);
        std::hint::black_box(renderer.render_block(at as u64, n));
        at += n;
    }
    began.elapsed().as_secs_f64() * 1e3
}

#[test]
#[ignore = "a measurement: run by hand, --release"]
fn envelope_bench() {
    let (plain, pg) = song(false);
    let (curved, cg) = song(true);
    let points = curved
        .read_project(&cg, |p| {
            p.envelopes
                .iter()
                .map(|(_, d)| d.points.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    let snap = Instant::now();
    let built = curved.read_project(&cg, super::RenderSnapshot::build);
    let snap_ms = snap.elapsed().as_secs_f64() * 1e3;
    drop(built);
    let blocks = (SECONDS * f64::from(RATE)) as usize / BLOCK;
    let p = render(&plain, &pg, false);
    let c = render(&curved, &cg, false);
    let cb = render(&curved, &cg, true);
    tracing::info!(points, snap_ms, p, c, cb, "envelope bench");
    // Read by hand with --nocapture (no logging set up in a test).
    std::io::Write::write_all(
        &mut std::io::stderr(),
        format!(
            "{TRACKS} takes x {SECONDS}s, {points} envelope points, {blocks} blocks\n\
             snapshot build: {snap_ms:.1} ms\n\
             render, no curves:           {p:.0} ms\n\
             render, curves:              {c:.0} ms ({:.1}x)\n\
             render, curves, re-snapshot each block: {cb:.0} ms\n",
            c / p
        )
        .as_bytes(),
    )
    .ok();
}
