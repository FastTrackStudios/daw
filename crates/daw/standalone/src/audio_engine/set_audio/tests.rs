//! The set's block loop, driven offline: the same `render_set_block` the
//! device callback runs, on stamps chosen so a block's period is exact.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use daw_proto::ProjectInfo;

use super::*;
use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::materialize::attach_audio_source;
use crate::media_seed::{StemSpec, seed_media_tracks};
use crate::transport_engine::PlayStateRepr;

const RATE: u32 = 48_000;
/// 480 frames at 48 kHz: a period of exactly 10 000 µs.
const BLOCK: usize = 480;
const PERIOD_MICROS: f64 = 10_000.0;

/// A song of one track whose item, from 0 s, plays a constant `level`.
fn song(daw: &Standalone, guid: &str, level: f32) {
    daw.seed_project(ProjectInfo {
        guid: guid.into(),
        name: guid.into(),
        path: String::new(),
    });
    let stem = StemSpec::new("Stem", format!("/x/{guid}.wav"), None);
    seed_media_tracks(daw, guid, &[stem], 0.0, 10.0);
    let takes: Vec<String> = daw
        .read_project(guid, |p| {
            p.takes
                .values()
                .flat_map(|tl| tl.takes.iter().map(|t| t.guid.clone()))
                .collect()
        })
        .unwrap_or_default();
    for take in takes {
        attach_audio_source(
            daw,
            guid,
            &take,
            DecodedAudio::new(vec![level; RATE as usize], 1, RATE),
        );
    }
}

fn voice(daw: &Standalone, guid: &str) -> Arc<Voice> {
    Arc::new(Voice::open(daw, guid, &VoiceSetup::offline(RATE)).expect("voice"))
}

/// What song `guid` renders on its own for `frames` frames from `start`.
fn alone(daw: &Standalone, guid: &str, start: u64, frames: usize) -> Vec<f32> {
    ProjectRenderer::new(daw, guid, RATE)
        .render_block(start, frames)
        .samples
}

/// Render `blocks` blocks of the set from block `first`, interleaved stereo.
fn run(voices: &Voices, first: usize, blocks: usize) -> Vec<f32> {
    let mut out = vec![0.0; blocks * BLOCK * 2];
    for (i, chunk) in out.chunks_mut(BLOCK * 2).enumerate() {
        let clock = BlockClock {
            frames: BLOCK as u32,
            stamp_micros: (first + i) as f64 * PERIOD_MICROS,
            period_micros: PERIOD_MICROS,
        };
        render_set_block(voices, clock, chunk);
    }
    out
}

fn assert_close(got: &[f32], want: &[f32]) {
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() < 1e-6, "sample {i}: got {g}, want {w}");
    }
}

fn two_songs() -> (Standalone, Arc<Voice>, Arc<Voice>) {
    let daw = Standalone::new();
    song(&daw, "a", 0.25);
    song(&daw, "b", 0.5);
    let (a, b) = (voice(&daw, "a"), voice(&daw, "b"));
    (daw, a, b)
}

// The backend spawns its transport pumps on the runtime.
#[tokio::test]
async fn two_playing_songs_sound_as_their_sum() {
    let (daw, a, b) = two_songs();
    a.shared().set_play_state(PlayStateRepr::Playing);
    b.shared().set_play_state(PlayStateRepr::Playing);
    let voices = Voices::default().with(a).with(b);

    let got = run(&voices, 0, 4);

    let frames = 4 * BLOCK;
    let want: Vec<f32> = alone(&daw, "a", 0, frames)
        .iter()
        .zip(alone(&daw, "b", 0, frames))
        .map(|(x, y)| x + y)
        .collect();
    assert!(want.iter().all(|s| *s > 0.0), "both songs sound");
    assert_close(&got, &want);
}

// The backend spawns its transport pumps on the runtime.
#[tokio::test]
async fn a_handoff_starts_the_next_song_on_its_frame() {
    let (daw, a, b) = two_songs();
    a.shared().set_play_state(PlayStateRepr::Playing);
    let voices = Voices::default().with(a.clone()).with(b.clone());
    let first = run(&voices, 0, 1);

    // A ends at frame 600 (12.5 ms): inside the second block, at 120.
    let end_frame = 600;
    let at = schedule_handoff(a.shared(), b.shared(), end_frame as f64 / f64::from(RATE))
        .expect("a plays");
    assert_eq!(at, 12_500.0);
    let rest = run(&voices, 1, 2);

    let a_level = alone(&daw, "a", 0, 1)[0];
    let b_level = alone(&daw, "b", 0, 1)[0];
    let want: Vec<f32> = (BLOCK..3 * BLOCK)
        .flat_map(|f| {
            let v = if f < end_frame { a_level } else { b_level };
            [v, v]
        })
        .collect();
    assert_close(&first, &vec![a_level; BLOCK * 2]);
    assert_close(&rest, &want);
    assert_eq!(a.shared().play_state(), PlayStateRepr::Stopped);
    assert_eq!(
        b.shared().playhead_samples_f64(),
        (3 * BLOCK - end_frame) as f64
    );
}

// The backend spawns its transport pumps on the runtime.
#[tokio::test]
async fn removing_a_voice_silences_it_and_frees_its_transport() {
    let (daw, a, b) = two_songs();
    a.shared().set_play_state(PlayStateRepr::Playing);
    b.shared().set_play_state(PlayStateRepr::Playing);
    let soft_clock = daw.transport_engine_for("b").soft_clock_enabled.clone();
    assert!(!soft_clock.load(Ordering::Relaxed), "the set drives b");
    let voices = Voices::default().with(a).with(b.clone());
    run(&voices, 0, 1);

    let voices = voices.without("b");
    b.release();
    let got = run(&voices, 1, 1);

    assert_close(&got, &alone(&daw, "a", BLOCK as u64, BLOCK));
    assert!(soft_clock.load(Ordering::Relaxed), "b's soft clock is back");
}

// The backend spawns its transport pumps on the runtime.
#[tokio::test]
async fn a_stopped_voice_only_opens_its_block() {
    let (_daw, a, b) = two_songs();
    a.shared().set_play_state(PlayStateRepr::Playing);
    let voices = Voices::default()
        .with(a)
        .with(b.clone())
        .with_current(Some("b"));

    let clock = BlockClock {
        frames: BLOCK as u32,
        stamp_micros: 0.0,
        period_micros: PERIOD_MICROS,
    };
    let block = render_set_block(&voices, clock, &mut vec![0.0; BLOCK * 2]);

    assert!(block.any_playing);
    assert!(block.current.is_some_and(|p| !p.any_playing()));
    assert_eq!(b.shared().playhead_samples_f64(), 0.0);
    assert!(b.shared().sync_snapshot().is_some(), "b's block was opened");
}
