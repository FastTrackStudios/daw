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
