//! A stretched take played at its own pitch.
//!
//! Stretch markers say where in the source each instant of a take plays.
//! Read straight through that map — what the mixer does with any take —
//! a stretch is varispeed: a drum slice's tail drawn out to a fifth of
//! its speed slides down in pitch and snaps back at the next hit, heard
//! as a record scratching on every double kick an edit pulled apart, and
//! the overheads' high end gone wherever a tail was slowed.
//!
//! A stretched take is rendered instead, once, through its markers as a
//! stretch map ([`stretch::map`]): between each pair, a stretch played at
//! speed is copied from the source as it is and one played slower or
//! faster goes through the phase vocoder — its pitch kept, its timing the
//! map's, started afresh at each marker so every mic of a kit, rendered
//! one take at a time through the same markers, stays in phase with the
//! others — each joined to the next across a short crossfade. (WSOLA did
//! this before: at a quarter of the speed it repeated its grains — a
//! buzz on every tail an edit drew out — and it chose its splices from
//! each mic's own signal, so the kit's mics combed against each other.)
//! The attacks are the markers' to keep: an edit puts a marker after each
//! hit's attack and plays that at speed, so the render copies none of its
//! own. The mixer then plays the render as an unstretched take. Renders
//! are kept by take, offset and markers, so a replan after an edit
//! renders only what the edit changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use daw_proto::StretchMarker;

use crate::audio_engine::decoder::DecodedAudio;
use crate::audio_engine::source::AudioSource;

/// The joint between two stretches of a render, seconds: a crossfade, so
/// the seam between a copy and a stretch does not click.
const JOINT: f64 = 0.002;

/// A stretch slower or faster than this is no stretch: copied as it is.
const AT_SPEED: f64 = 1e-4;

/// The most renders kept before the cache starts over.
const KEPT: usize = 8192;

/// A render's key: its source (by identity), and its markers and length
/// as bits.
type Key = (usize, u64, Vec<(u64, u64)>);

fn cache() -> &'static Mutex<HashMap<Key, Arc<AudioSource>>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Arc<AudioSource>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `audio` as the take with `markers` plays it over `length` seconds of
/// its playback time, rendered at its own pitch; `None` when there is
/// nothing to render — fewer than two markers, every stretch at speed,
/// or a source read a window at a time (the browser's), which the mixer
/// plays through the map as before.
pub(crate) fn rendered(
    audio: &Arc<AudioSource>,
    markers: &[StretchMarker],
    length: f64,
) -> Option<Arc<AudioSource>> {
    if markers.len() < 2 || length <= 0.0 || matches!(**audio, AudioSource::Streamed(_)) {
        return None;
    }
    let stretched = markers.windows(2).any(|pair| {
        let (a, b) = (&pair[0], &pair[1]);
        let played = b.position - a.position;
        played > 0.0 && ((b.source_position - a.source_position) / played - 1.0).abs() > AT_SPEED
    });
    if !stretched {
        return None;
    }
    let key: Key = (
        Arc::as_ptr(audio) as usize,
        length.to_bits(),
        markers
            .iter()
            .map(|m| (m.position.to_bits(), m.source_position.to_bits()))
            .collect(),
    );
    if let Some(kept) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Some(kept);
    }
    let render = Arc::new(render(audio, markers, length));
    if let Ok(mut c) = cache().lock() {
        if c.len() >= KEPT {
            c.clear();
        }
        c.insert(key, Arc::clone(&render));
    }
    Some(render)
}

/// The render itself: the take's markers as a stretch map over the part
/// of its source it plays ([`stretch::map::render`]).
fn render(audio: &AudioSource, markers: &[StretchMarker], length: f64) -> AudioSource {
    let sr = f64::from(audio.sample_rate().max(1));
    let channels = usize::from(audio.channels().max(1));
    let frames = (length * sr).ceil() as usize;
    let source_frames = audio.frame_count();
    let map = |t: f64| StretchMarker::source_position_at(markers, t).unwrap_or(t);
    // The playback time from 0 to `length`, cut at every marker inside it.
    let mut cuts: Vec<f64> = vec![0.0];
    cuts.extend(
        markers
            .iter()
            .map(|m| m.position)
            .filter(|p| *p > 0.0 && *p < length),
    );
    cuts.push(length);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // Only the source the take plays, and a frame's reach either side —
    // a slice of a long take reads its own few hundred milliseconds.
    let config = stretch::map::MapConfig {
        attack: 0,
        ..stretch::map::MapConfig::for_sample_rate(sr)
    };
    let reach = config.vocoder.frame as f64;
    let (lo, hi) = cuts
        .iter()
        .map(|&t| map(t) * sr)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), s| {
            (lo.min(s), hi.max(s))
        });
    let from = (lo - reach).floor().max(0.0) as usize;
    let to = ((hi + reach).ceil().max(0.0) as usize).min(source_frames);
    let planar: Vec<Vec<f32>> = (0..channels)
        .map(|ch| {
            (from..to.max(from))
                .map(|i| audio.channel_interp(i, i, 0.0, ch))
                .collect()
        })
        .collect();
    let anchors: Vec<stretch::map::Anchor> = cuts
        .iter()
        .map(|&t| stretch::map::Anchor {
            out: t * sr,
            src: map(t).max(0.0) * sr - from as f64,
        })
        .collect();
    let input: Vec<&[f32]> = planar.iter().map(Vec::as_slice).collect();
    let out = stretch::map::render(&input, &anchors, frames, &config);
    let mut interleaved = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        for ch in &out {
            interleaved.push(ch.get(i).copied().unwrap_or(0.0));
        }
    }
    AudioSource::Memory(DecodedAudio::new(
        interleaved,
        u16::try_from(channels).unwrap_or(2),
        audio.sample_rate(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(position: f64, source_position: f64) -> StretchMarker {
        StretchMarker {
            position,
            source_position,
            slope: 0.0,
        }
    }

    /// A 440 Hz tone, a second of it.
    fn tone() -> Arc<AudioSource> {
        let sr = 48_000_u32;
        let samples = (0..sr)
            .map(|i| (std::f64::consts::TAU * 440.0 * f64::from(i) / f64::from(sr)).sin() as f32)
            .collect();
        Arc::new(AudioSource::Memory(DecodedAudio::new(samples, 1, sr)))
    }

    #[test]
    fn a_drawn_out_tail_keeps_its_pitch() {
        // Its first 30 ms as played, the next 70 ms of source over 280 ms.
        let markers = [marker(0.0, 0.2), marker(0.03, 0.23), marker(0.31, 0.30)];
        let out = rendered(&tone(), &markers, 0.31).expect("rendered");
        let n = out.frame_count();
        assert_eq!(n, (0.31_f64 * 48_000.0).ceil() as usize);
        // In the slowed stretch, still 440 Hz: 880 crossings a second,
        // not the 220 a varispeed at a quarter would give.
        let (a, b) = (4_800, 14_400);
        let x: Vec<f32> = (a..b).map(|i| out.channel_interp(i, i, 0.0, 0)).collect();
        let crossings = x
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count() as f64
            / ((b - a) as f64 / 48_000.0);
        assert!((crossings - 880.0).abs() < 880.0 * 0.05, "{crossings:.0}");
        // And where it plays at speed, the source sample for sample.
        let src = tone();
        for i in [10, 500, 1_000] {
            let want = src.channel_interp(9_600 + i, 9_600 + i, 0.0, 0);
            assert!((out.channel_interp(i, i, 0.0, 0) - want).abs() < 1e-5);
        }
    }

    /// A noisy tail — a cymbal's wash, a room — drawn out to a quarter of
    /// its speed is not its own grains played over and over: no echo of
    /// itself a grain later (the buzz WSOLA made of Unbreakable's bar 56),
    /// and its level where the source's is.
    #[test]
    fn a_tail_drawn_out_to_a_quarter_does_not_buzz() {
        let sr = 48_000_u32;
        // Noise, steady: xorshift, so the test is the same every run.
        let mut state = 0x9e37_79b9_u32;
        let samples: Vec<f32> = (0..sr)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                0.3 * (2.0 * (state as f32 / u32::MAX as f32) - 1.0)
            })
            .collect();
        let noise = Arc::new(AudioSource::Memory(DecodedAudio::new(samples, 1, sr)));
        // 30 ms as played, then 100 ms of source over 400 ms.
        let markers = [marker(0.0, 0.2), marker(0.03, 0.23), marker(0.43, 0.33)];
        let out = rendered(&noise, &markers, 0.43).expect("rendered");
        let x: Vec<f64> = (4_800..19_200)
            .map(|i| f64::from(out.channel_interp(i, i, 0.0, 0)))
            .collect();
        let energy: f64 = x.iter().map(|v| v * v).sum();
        let rms = (energy / x.len() as f64).sqrt();
        let source_rms = 0.3 / 3f64.sqrt();
        assert!(
            (rms / source_rms - 1.0).abs() < 0.3,
            "the tail's level kept: {rms:.3} against {source_rms:.3}"
        );
        // Its likeness to itself at any lag a grain could repeat at.
        let worst = (200..2_000)
            .step_by(8)
            .map(|lag| {
                let dot: f64 = x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum();
                dot / energy
            })
            .fold(0.0_f64, f64::max);
        assert!(worst < 0.25, "no grain repeated: {worst:.3}");
    }

    #[test]
    fn a_take_played_at_speed_is_not_rendered() {
        let markers = [marker(0.0, 0.2), marker(0.1, 0.3)];
        assert!(rendered(&tone(), &markers, 0.1).is_none());
    }
}
