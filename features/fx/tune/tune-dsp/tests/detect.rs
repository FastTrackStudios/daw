//! YIN + note + correction sanity checks on synthesized pitched tones.

use tune_dsp::{
    analyze, correct_notes, hz_to_midi, AnalyzeConfig, CorrectConfig, Scale, YinDetector, YinConfig,
};

const SR: f64 = 48_000.0;

fn sine(freq: f64, secs: f64, start: usize) -> Vec<f64> {
    let n = (SR * secs) as usize;
    (0..n)
        .map(|i| 0.4 * (core::f64::consts::TAU * freq * (start + i) as f64 / SR).sin())
        .collect()
}

#[test]
fn yin_detects_a440() {
    let mut yin = YinDetector::new(SR, YinConfig::default());
    let buf = sine(440.0, 0.1, 0);
    let f = yin.detect(&buf);
    let hz = f.f0_hz.expect("A440 should be voiced");
    assert!(
        (hz - 440.0).abs() < 5.0,
        "expected ~440 Hz, got {hz} (aperiodicity {})",
        f.aperiodicity
    );
}

#[test]
fn detuned_note_snaps_to_scale() {
    // 445 Hz ≈ A4 + ~19.5 cents sharp. Chromatic snap should pull toward A4.
    let buf = sine(445.0, 0.3, 0);
    let a = analyze(&buf, SR, AnalyzeConfig::default());
    assert!(!a.notes.is_empty(), "expected at least one note");

    let corr = correct_notes(&a.notes, Scale::CHROMATIC, CorrectConfig::default());
    let c = corr[0];
    let a4 = hz_to_midi(440.0);
    assert!(
        (c.target_midi - a4).abs() < 0.05,
        "445 Hz should snap to A4 ({a4}), got {}",
        c.target_midi
    );
    // Correction is downward (sharp → down) and non-trivial.
    assert!(
        c.applied_cents < -10.0,
        "expected downward correction, got {} cents",
        c.applied_cents
    );
    assert!(c.ratio < 1.0, "downward shift ratio should be < 1");
}

#[test]
fn in_tune_note_is_left_alone() {
    let buf = sine(440.0, 0.3, 0);
    let a = analyze(&buf, SR, AnalyzeConfig::default());
    let corr = correct_notes(&a.notes, Scale::CHROMATIC, CorrectConfig::default());
    assert!(!corr.is_empty());
    // Within the deadband → no shift.
    assert_eq!(corr[0].ratio, 1.0);
    assert_eq!(corr[0].applied_cents, 0.0);
}
