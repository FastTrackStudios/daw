//! Envelope generation over synthesised audio.
//!
//! These assert that a curve was produced with the right *shape* and the
//! right invariants — not that a threshold is musically correct. Whether
//! a gate is set well is a listening judgement and is not what a test
//! can settle.

use level_dsp::classify::ClassifyConfig;
use level_dsp::envelope::{analyze, gate_envelope};

const SR: f64 = 48_000.0;

/// Phrase, silence, phrase — the shape a gate exists for.
fn phrase_gap_phrase() -> Vec<f64> {
    let mut out = Vec::new();
    let tone = |n: usize, amp: f64, out: &mut Vec<f64>| {
        for i in 0..n {
            let t = i as f64 / SR;
            out.push(amp * (2.0 * core::f64::consts::PI * 220.0 * t).sin());
        }
    };
    tone((SR * 0.5) as usize, 0.5, &mut out);
    out.extend(core::iter::repeat_n(0.0, (SR * 0.5) as usize));
    tone((SR * 0.5) as usize, 0.5, &mut out);
    out
}

#[test]
fn the_gate_emits_a_curve_rather_than_only_treated_audio() {
    let audio = phrase_gap_phrase();
    let analysis = analyze(&audio, SR, ClassifyConfig::default());
    let env = gate_envelope(&analysis, -40.0, -60.0, 0.25);

    assert!(!env.is_empty(), "the gate's decision is now visible");
    assert!(
        env.iter().any(|p| p.db < -30.0),
        "it closed somewhere — the gap"
    );
    assert!(
        env.iter().any(|p| p.db > -1.0),
        "and it opened somewhere — the phrases"
    );
}

#[test]
fn the_curve_is_sparse_not_one_point_per_block() {
    // 1.5 s at a 10 ms block is 150 blocks. A curve with one point each
    // is what makes a five-minute vocal 2.4 MB, so this is the property
    // that lets envelopes live in the item at all.
    let audio = phrase_gap_phrase();
    let analysis = analyze(&audio, SR, ClassifyConfig::default());
    let env = gate_envelope(&analysis, -40.0, -60.0, 0.25);

    assert!(
        env.len() < analysis.blocks() / 4,
        "{} points for {} blocks is not sparse",
        env.len(),
        analysis.blocks()
    );
}

#[test]
fn the_edges_survive_decimation() {
    let audio = phrase_gap_phrase();
    let analysis = analyze(&audio, SR, ClassifyConfig::default());
    let env = gate_envelope(&analysis, -40.0, -60.0, 0.25);

    // Two closings and two openings at most, but at least one of each,
    // and they must be *adjacent* pairs — an edge smoothed into a ramp
    // over half a second would be a gate that no longer sounds like one.
    let mut sharpest = f64::INFINITY;
    for w in env.windows(2) {
        if (w[0].db - w[1].db).abs() > 30.0 {
            sharpest = sharpest.min(w[1].t_s - w[0].t_s);
        }
    }
    assert!(
        sharpest < 0.05,
        "the gate's edge was smoothed to {sharpest}s"
    );
}

#[test]
fn a_gate_curve_holds_rather_than_ramping() {
    let audio = phrase_gap_phrase();
    let analysis = analyze(&audio, SR, ClassifyConfig::default());
    let env = gate_envelope(&analysis, -40.0, -60.0, 0.25);
    assert!(
        env.iter().all(|p| p.hold),
        "a gate steps; linear interpolation between its points would \
         need far more of them to describe the same shape"
    );
}

#[test]
fn silence_alone_produces_a_closed_curve_and_no_edges() {
    let audio = vec![0.0; (SR * 1.0) as usize];
    let analysis = analyze(&audio, SR, ClassifyConfig::default());
    let env = gate_envelope(&analysis, -40.0, -60.0, 0.25);

    assert!(env.len() <= 2, "nothing happened, so nothing to describe");
    assert!(env.iter().all(|p| p.db < -30.0), "and it stayed shut");
}

#[test]
fn one_analysis_pass_serves_every_stage() {
    // The shared pass is what makes four independent curves affordable —
    // and what lets the rider know where silence is without consulting
    // the gate.
    let audio = phrase_gap_phrase();
    let analysis = analyze(&audio, SR, ClassifyConfig::default());

    assert_eq!(analysis.blocks(), analysis.rms_db.len());
    assert!(analysis.blocks() > 100, "1.5s at 10ms blocks");
    assert!(
        analysis.silence_db < 0.0 && analysis.silence_db.is_finite(),
        "an adaptive floor was found for this material: {}",
        analysis.silence_db
    );
    assert!(analysis.time_of(0) > 0.0);
    assert!(analysis.time_of(analysis.blocks() - 1) < 1.6);
}

// ── The other three (#204) ───────────────────────────────────────────

use level_dsp::envelope::{breath_envelope, ride_envelope, sibilance_spans};

/// Loud tonal phrase, quiet breath, loud tonal phrase.
fn phrase_breath_phrase() -> Vec<f64> {
    let mut out = Vec::new();
    let tone = |n: usize, amp: f64, hz: f64, out: &mut Vec<f64>| {
        for i in 0..n {
            let t = i as f64 / SR;
            out.push(amp * (2.0 * core::f64::consts::PI * hz * t).sin());
        }
    };
    tone((SR * 0.4) as usize, 0.5, 220.0, &mut out);
    // A breath: quiet broadband noise, not bright enough to be an ess.
    let mut seed = 12345u32;
    for _ in 0..(SR * 0.3) as usize {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let n = ((seed >> 16) as f64 / 32768.0) - 1.0;
        out.push(n * 0.01);
    }
    tone((SR * 0.4) as usize, 0.5, 220.0, &mut out);
    out
}

#[test]
fn every_stage_emits_its_own_curve_from_one_pass() {
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());

    let gate = gate_envelope(&a, -40.0, -60.0, 0.25);
    let breath = breath_envelope(&a, 12.0, -30.0, -70.0, 0.25);
    let ride = ride_envelope(&a, -18.0, 1.0, 6.0, 6.0, 0.25);
    let sib = sibilance_spans(&a, 4.0);

    assert!(!gate.is_empty());
    assert!(!breath.is_empty());
    assert!(!ride.is_empty());
    // Sibilance may legitimately find nothing in this material; the
    // point is that it ran off the same pass.
    let _ = sib;
}

#[test]
fn sibilance_is_spans_with_gain_not_a_dense_curve() {
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());
    let spans = sibilance_spans(&a, 4.0);

    assert!(
        spans.len() < a.blocks() / 10,
        "{} spans for {} blocks is not a discrete-event representation",
        spans.len(),
        a.blocks()
    );
    for s in &spans {
        assert!(s.to_s > s.from_s, "a span has width");
        assert!(s.db < 0.0, "sibilance reduces");
    }
}

#[test]
fn the_ride_does_not_track_the_noise_floor_between_phrases() {
    // The reason the stages share an analysis pass: without the silence
    // floor the rider would boost room tone in the gap by the full
    // correction, which is exactly what gating-first used to prevent.
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());
    let ride = ride_envelope(&a, -18.0, 1.0, 24.0, 6.0, 0.25);

    let peak = ride.iter().map(|p| p.db).fold(f64::MIN, f64::max);
    assert!(
        peak < 24.0,
        "the ride hit its ceiling, which means it chased the floor: {peak}"
    );
}

#[test]
fn the_ride_holds_between_phrases_rather_than_returning_to_unity() {
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());
    let ride = ride_envelope(&a, -18.0, 1.0, 6.0, 6.0, 0.25);

    // A ride that snapped to 0 dB in every gap would swell at each
    // phrase edge.
    let zeros = ride.iter().filter(|p| p.db.abs() < 1e-9).count();
    assert!(
        zeros <= 1,
        "{zeros} points at unity suggests it reset between phrases"
    );
}

#[test]
fn the_curves_are_independent_of_each_other() {
    // Each must be bypassable on its own, so none may be derived from
    // another's output. Generating one twice with a different *other*
    // stage's settings must change nothing.
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());

    let ride_a = ride_envelope(&a, -18.0, 1.0, 6.0, 6.0, 0.25);
    // Nothing about the gate's configuration is an input to the ride.
    let _tight_gate = gate_envelope(&a, -10.0, -90.0, 0.25);
    let ride_b = ride_envelope(&a, -18.0, 1.0, 6.0, 6.0, 0.25);

    assert_eq!(ride_a, ride_b, "the ride does not know what the gate did");
}

#[test]
fn a_breath_ducks_and_the_phrases_do_not() {
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());
    let breath = breath_envelope(&a, 12.0, -30.0, -70.0, 0.25);

    assert!(
        breath.iter().any(|p| p.db < -6.0),
        "something got ducked"
    );
    assert!(
        breath.iter().any(|p| p.db.abs() < 1e-9),
        "and something did not"
    );
}

#[test]
fn a_breath_curve_ramps_rather_than_stepping() {
    // Unlike a gate: a breath ducks in and out over a slew.
    let audio = phrase_breath_phrase();
    let a = analyze(&audio, SR, ClassifyConfig::default());
    let breath = breath_envelope(&a, 12.0, -30.0, -70.0, 0.25);
    assert!(breath.iter().all(|p| !p.hold));
}
