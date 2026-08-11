//! Take-to-take alignment, against pairs whose true offset is known.
//!
//! Every case here is a reference and a dub built from the same phrase
//! description, with one property deliberately changed — a shift, a
//! stretch, a different pitch. That is the only way to assert that an
//! alignment is *correct* rather than merely plausible.

use expression_editor_audio::{AlignConfig, Analysis, TakeConfig, align, analyze_take};

const SR: f64 = 44100.0;

fn midi_to_hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

/// One syllable: a pitched tone with an attack and release.
fn syllable(midi: f64, secs: f64) -> Vec<f64> {
    let n = (SR * secs) as usize;
    let mut phase = 0.0;
    (0..n)
        .map(|i| {
            let t = i as f64 / SR;
            phase += core::f64::consts::TAU * midi_to_hz(midi) / SR;
            let s = phase.sin() + 0.5 * (phase * 2.0).sin() + 0.3 * (phase * 3.0).sin();
            s * (t / 0.02).min(1.0) * ((secs - t) / 0.04).clamp(0.0, 1.0) * 0.3
        })
        .collect()
}

fn gap(secs: f64) -> Vec<f64> {
    vec![0.0; (SR * secs).max(0.0) as usize]
}

/// A phrase: `(midi, length, gap-after)` repeated.
fn phrase(parts: &[(f64, f64, f64)], transpose: f64, rate: f64) -> Vec<f64> {
    let mut out = Vec::new();
    for &(midi, len, after) in parts {
        out.extend(syllable(midi + transpose, len * rate));
        out.extend(gap(after * rate));
    }
    out
}

const LINE: &[(f64, f64, f64)] = &[
    (60.0, 0.30, 0.10),
    (64.0, 0.25, 0.08),
    (67.0, 0.35, 0.12),
    (65.0, 0.30, 0.10),
];

fn analyse(audio: &[f64]) -> Analysis {
    analyze_take(audio, SR, TakeConfig::default())
}

/// Seconds the map moves dub time `t` by.
fn shift_at(a: &expression_editor_audio::Alignment, secs: f64) -> f64 {
    let i = ((secs * a.frame_rate).round() as usize).min(a.map.len().saturating_sub(1));
    (a.map[i] - i as f64) / a.frame_rate
}

#[test]
fn an_identical_take_needs_no_correction() {
    let audio = phrase(LINE, 0.0, 1.0);
    let r = analyse(&audio);
    let d = analyse(&audio);
    let a = align(&r, &d, AlignConfig::default()).expect("aligned");

    assert_eq!(a.map.len(), d.frames.frames.len());
    assert!(
        a.max_shift_secs() < 0.02,
        "a take against itself should barely move: {} s",
        a.max_shift_secs()
    );
}

#[test]
fn a_late_dub_is_pulled_back_by_how_late_it_is() {
    let reference = phrase(LINE, 0.0, 1.0);
    // The same performance, starting 120 ms later.
    let mut dub = gap(0.12);
    dub.extend(phrase(LINE, 0.0, 1.0));

    let a = align(&analyse(&reference), &analyse(&dub), AlignConfig::default()).expect("aligned");

    // Mid-phrase, the map should point about 120 ms earlier.
    let s = shift_at(&a, 0.6);
    assert!(
        (s + 0.12).abs() < 0.06,
        "wanted about -0.12 s of correction, got {s}"
    );
}

#[test]
fn a_dub_sung_slower_is_compressed_progressively() {
    let reference = phrase(LINE, 0.0, 1.0);
    // Same line, 15% slower — the drift grows across the phrase, which
    // is what a constant offset cannot fix and alignment must.
    let dub = phrase(LINE, 0.0, 1.15);

    let a = align(&analyse(&reference), &analyse(&dub), AlignConfig::default()).expect("aligned");

    let early = shift_at(&a, 0.2);
    let late = shift_at(&a, 1.4);
    assert!(
        late < early - 0.05,
        "the correction grows as the takes drift apart: {early} then {late}"
    );
    // And the map never goes backwards, which would render as a stutter.
    for pair in a.map.windows(2) {
        assert!(pair[1] >= pair[0]);
    }
}

#[test]
fn a_harmony_at_a_different_pitch_still_aligns_on_timing() {
    // The case that decides whether energy or pitch is the primary
    // feature. A stacked third is deliberately at another pitch and
    // must still line up.
    let reference = phrase(LINE, 0.0, 1.0);
    let mut dub = gap(0.1);
    dub.extend(phrase(LINE, 4.0, 1.0));

    let a = align(&analyse(&reference), &analyse(&dub), AlignConfig::default()).expect("aligned");
    let s = shift_at(&a, 0.7);
    assert!(
        (s + 0.1).abs() < 0.07,
        "a harmony aligns on when, not what: got {s}"
    );
}

#[test]
fn strength_scales_the_correction_without_changing_its_shape() {
    let reference = phrase(LINE, 0.0, 1.0);
    let mut dub = gap(0.12);
    dub.extend(phrase(LINE, 0.0, 1.0));
    let (r, d) = (analyse(&reference), analyse(&dub));

    let full = align(&r, &d, AlignConfig::default()).unwrap();
    let half = align(
        &r,
        &d,
        AlignConfig {
            strength: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    let none = align(
        &r,
        &d,
        AlignConfig {
            strength: 0.0,
            ..Default::default()
        },
    )
    .unwrap();

    let (f, h) = (shift_at(&full, 0.6), shift_at(&half, 0.6));
    assert!((h - f * 0.5).abs() < 0.02, "half is half: {f} vs {h}");
    assert!(
        none.max_shift_secs() < 1e-9,
        "zero strength leaves the dub exactly as sung"
    );
}

#[test]
fn nothing_moves_further_than_the_configured_limit() {
    // A dub a second and a half late. Alignment *could* fix it — the
    // parts do correspond — but the user asked for corrections no
    // larger than 300 ms, and that is a promise about the result rather
    // than a hint to the search. Better a partly-corrected take than
    // one silently dragged five times further than allowed.
    let reference = phrase(LINE, 0.0, 1.0);
    let mut dub = gap(1.5);
    dub.extend(phrase(LINE, 0.0, 1.0));

    let cfg = AlignConfig {
        max_shift_secs: 0.3,
        ..Default::default()
    };
    let a = align(&analyse(&reference), &analyse(&dub), cfg).expect("aligned");
    assert!(
        a.max_shift_secs() <= 0.35,
        "nothing moved further than the band allows: {} s",
        a.max_shift_secs()
    );
}

#[test]
fn markers_are_thinned_but_keep_both_ends() {
    let reference = phrase(LINE, 0.0, 1.0);
    let mut dub = gap(0.1);
    dub.extend(phrase(LINE, 0.0, 1.0));
    let d = analyse(&dub);
    let a = align(&analyse(&reference), &d, AlignConfig::default()).expect("aligned");

    let hop = d.frames.hop;
    let dense = a.markers(hop, 1);
    let thin = a.markers(hop, 16);
    assert_eq!(dense.len(), a.map.len());
    assert!(thin.len() < dense.len() / 8);

    // Both ends survive thinning, or the map would be undefined at the
    // edges and the take would snap back there.
    assert_eq!(thin.first().unwrap().sample, dense.first().unwrap().sample);
    assert_eq!(thin.last().unwrap().sample, dense.last().unwrap().sample);
    // Anchored at dub time, sorted, and carrying a real correction.
    for pair in thin.windows(2) {
        assert!(pair[1].sample > pair[0].sample);
    }
    assert!(thin.iter().any(|m| m.d_time.abs() > 1.0));
}

#[test]
fn an_empty_take_declines_to_align() {
    let real = analyse(&phrase(LINE, 0.0, 1.0));
    let empty = analyse(&[]);
    assert!(align(&real, &empty, AlignConfig::default()).is_none());
    assert!(align(&empty, &real, AlignConfig::default()).is_none());
}

#[test]
fn the_map_covers_every_dub_frame_and_stays_in_the_reference() {
    let reference = phrase(LINE, 0.0, 1.0);
    let dub = phrase(LINE, 0.0, 1.2);
    let (r, d) = (analyse(&reference), analyse(&dub));
    let a = align(&r, &d, AlignConfig::default()).expect("aligned");

    assert_eq!(a.map.len(), d.frames.frames.len());
    let last_ref = (r.frames.frames.len() - 1) as f64;
    assert!(
        a.map.iter().all(|&v| v >= 0.0 && v <= last_ref),
        "the map never points outside the reference"
    );
}

#[cfg(feature = "render")]
#[test]
fn an_aligned_dub_renders_and_keeps_its_pitch() {
    // Alignment is a timing edit, so the rendered dub must still be the
    // dub: retimed, not retuned.
    let reference = phrase(LINE, 0.0, 1.0);
    let dub_audio = phrase(LINE, 0.0, 1.15);
    let r = analyse(&reference);
    let mut d = analyse(&dub_audio);

    let a = align(&r, &d, AlignConfig::default()).expect("aligned");
    d.pitch.markers = a.markers(d.frames.hop, 8);
    let out = d.render(&dub_audio);
    assert!(!out.is_empty());

    let again = analyze_take(&out, SR, TakeConfig::default());
    assert!(!again.doc.notes.is_empty(), "the rendered dub still sings");
    // First note of the line is C4 in both takes.
    let n = &again.doc.notes[0];
    assert!(
        (n.row - 60).abs() <= 1,
        "retimed, not retuned — got row {}",
        n.row
    );
}
