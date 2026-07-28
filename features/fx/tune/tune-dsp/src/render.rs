//! Committed (offline) render: [`crate::model::PitchDoc`] edits →
//! WORLD resynthesis.
//!
//! WORLD analyzes once (`f0[]`, spectral envelope, aperiodicity); the
//! edit render REWRITES the f0 contour from the blob targets and
//! resynthesizes — formants stay untouched by construction, which is
//! why this is the quality tier (the realtime preview shifts with
//! PSOLA instead). WORLD's frame grid (~5 ms) differs from the
//! analysis hop grid; frames map through time.

use crate::detect::midi_to_hz;
use crate::model::PitchDoc;
use world_sys::WorldAnalysis;

/// Render the edited document over the original mono audio.
///
/// Frames whose time falls inside an edited blob take the blob's
/// target contour (+ marker bend); everything else keeps the WORLD
/// analysis f0 (unvoiced consonants and unedited regions pass
/// through untouched).
pub fn render_world(doc: &PitchDoc, audio: &[f64], sample_rate: u32) -> Vec<f64> {
    let analysis = WorldAnalysis::analyze(audio, sample_rate, 5.0);
    let mut f0 = analysis.f0.clone();
    for (i, slot) in f0.iter_mut().enumerate() {
        if *slot <= 0.0 {
            continue; // unvoiced — leave it alone
        }
        let t = analysis.temporal_positions[i];
        let sample = t * sample_rate as f64;
        let frame = (sample / doc.hop.max(1) as f64) as usize;
        for blob in &doc.blobs {
            if frame >= blob.start_frame && frame <= blob.end_frame {
                if let Some(target) = blob.target_midi(frame) {
                    *slot = midi_to_hz(target + doc.bend_at(sample));
                }
                break;
            }
        }
    }
    analysis.synthesize_with_f0(&f0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{YinConfig, YinDetector};
    use crate::model::PitchDoc;
    use crate::tracker::TrackedNote;

    const SR: u32 = 48000;

    fn tone(buf: &[f64], freq: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in buf.iter().enumerate() {
            let ph = core::f64::consts::TAU * freq * i as f64 / SR as f64;
            re += x * ph.cos();
            im += x * ph.sin();
        }
        (re * re + im * im).sqrt() / buf.len() as f64
    }

    #[test]
    fn transposed_blob_renders_at_the_new_pitch() {
        // A 220 Hz "voice" (pulse train + resonator), blob transposed
        // +3 st (→ ~261.6 Hz), rendered through WORLD.
        let n = 2 * SR as usize;
        let mut x = vec![0.0; n];
        let period = SR as f64 / 220.0;
        let mut next = 0.0f64;
        for (i, o) in x.iter_mut().enumerate() {
            if i as f64 >= next {
                *o = 1.0;
                next += period;
            }
        }
        let w = core::f64::consts::TAU * 900.0 / SR as f64;
        let (a1, a2) = (2.0 * 0.98 * w.cos(), -0.98f64 * 0.98);
        let (mut y1, mut y2) = (0.0f64, 0.0f64);
        for o in x.iter_mut() {
            let y = *o + a1 * y1 + a2 * y2;
            y2 = y1;
            y1 = y;
            *o = y * 0.05;
        }

        // Analyze with the house YIN just to build the doc (hop 512).
        let hop = 512usize;
        let mut det = YinDetector::new(SR as f64, YinConfig::default());
        let window = det.window();
        let mut f0 = Vec::new();
        let mut i = 0;
        while i + window <= n {
            f0.push(det.detect(&x[i..i + window]).f0_hz.unwrap_or(220.0));
            i += hop;
        }
        let note = TrackedNote {
            start_frame: 0,
            end_frame: f0.len() - 1,
            median_midi: 57.0, // A3 = 220 Hz
            f0,
        };
        let mut doc = PitchDoc::from_notes(&[note], hop, SR as f64);
        doc.blobs[0].transpose(3.0);
        // Robot mode so the render target is clean.
        doc.blobs[0].drift_amount = 0.0;
        doc.blobs[0].modulation_amount = 0.0;

        let y = render_world(&doc, &x, SR);
        let late = &y[SR as usize / 2..(3 * SR as usize) / 2];
        let e_new = tone(late, 261.6);
        let e_old = tone(late, 220.0);
        assert!(
            e_new > e_old * 2.0,
            "render lands on the transposed pitch: new={e_new:.5} old={e_old:.5}"
        );
        // The 900 Hz formant survives: nearest harmonic of 261.6 (3rd,
        // 785 Hz vs 4th, 1046) — envelope keeps ~900 region strong vs
        // where a resampler would move it (900·1.19 ≈ 1071).
        let e_h3 = tone(late, 261.6 * 3.0);
        let e_h5 = tone(late, 261.6 * 5.0);
        assert!(
            e_h3 > e_h5,
            "envelope still peaks near 900 Hz: h3={e_h3:.5} h5={e_h5:.5}"
        );
    }
}
