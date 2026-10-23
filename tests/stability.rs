//! No setting of a delay or reverb may run away.
//!
//! Every reverb algorithm at full decay, the shimmer at its most extreme
//! (full amount, two coherent octave voices), every delay style at the top
//! of its feedback: a burst in, then silence, and the output must stay
//! finite and never grow past what the burst put in. A regenerating block
//! that rings long is fine; one that climbs is a speaker-destroying bug —
//! the shimmer's did, to infinity in six seconds.

use builtin::{NativeDelay, NativeReverb};
use daw::plugin::{PluginEvents, PluginInstance};

const SR: f64 = 48_000.0;
const BLOCK: usize = 512;

/// Peak of the first second (the burst and its onset) and the largest peak
/// of any later second, over `secs`.
fn peaks(fx: &mut dyn PluginInstance, secs: usize) -> (f32, f32) {
    fx.prepare(SR, BLOCK as u32).unwrap();
    let per_sec = SR as usize / BLOCK;
    let mut seed = 5u32;
    let (mut first, mut later) = (0.0f32, 0.0f32);
    for sec in 0..secs {
        for b in 0..per_sec {
            let x: Vec<f32> = (0..BLOCK)
                .map(|_| {
                    if sec == 0 && b < 10 {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.5
                    } else {
                        0.0
                    }
                })
                .collect();
            let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
            fx.process_block(&x, &x, &mut l, &mut r, &PluginEvents::EMPTY)
                .unwrap();
            let p = l.iter().chain(&r).fold(0.0f32, |m, s| {
                if s.is_finite() {
                    m.max(s.abs())
                } else {
                    f32::INFINITY
                }
            });
            if sec == 0 {
                first = first.max(p);
            } else {
                later = later.max(p);
            }
        }
    }
    (first, later)
}

fn assert_bounded(what: &str, fx: &mut dyn PluginInstance) {
    let (first, later) = peaks(fx, 12);
    assert!(first.is_finite() && later.is_finite(), "{what}: not finite");
    // Room to bloom (a swell, a shimmer building), none to run away.
    assert!(
        later <= first.max(0.05) * 4.0,
        "{what}: grew from {first} to {later}"
    );
}

/// One reverb algorithm, every variant it has, at full decay. (A variant
/// past an algorithm's own count falls back to its first, so asking every
/// algorithm for three rendered most of them three times over.)
fn reverb_at_full_decay_stays_bounded(algorithm: u8) {
    let variants = reverb::AlgorithmType::from_index(usize::from(algorithm)).variant_count();
    for variant in (0..variants).filter_map(|v| u8::try_from(v).ok()) {
        let mut r = NativeReverb::new(SR);
        for (n, v) in [
            ("algorithm", f64::from(algorithm)),
            ("variant", f64::from(variant)),
            ("decay", 0.99),
            ("size", 1.0),
            ("mix", 1.0),
            ("dry", 0.0),
            ("modulation", 1.0),
        ] {
            r.set_named(n, v);
        }
        assert_bounded(&format!("reverb {algorithm}/{variant}"), &mut r);
    }
}

/// Every reverb algorithm at full decay, one test each: twelve seconds of
/// audio per variant is real work, and fourteen algorithms in one test ran
/// past the per-test timeout on a loaded runner. Apart, they run in parallel
/// and a failure names its algorithm.
macro_rules! every_reverb_at_full_decay {
    ($($name:ident = $algorithm:literal),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                reverb_at_full_decay_stays_bounded($algorithm);
            }
        )*
    };
}

every_reverb_at_full_decay! {
    reverb_algorithm_0_at_full_decay_stays_bounded = 0,
    reverb_algorithm_1_at_full_decay_stays_bounded = 1,
    reverb_algorithm_2_at_full_decay_stays_bounded = 2,
    reverb_algorithm_3_at_full_decay_stays_bounded = 3,
    reverb_algorithm_4_at_full_decay_stays_bounded = 4,
    reverb_algorithm_5_at_full_decay_stays_bounded = 5,
    reverb_algorithm_6_at_full_decay_stays_bounded = 6,
    reverb_algorithm_7_at_full_decay_stays_bounded = 7,
    reverb_algorithm_8_at_full_decay_stays_bounded = 8,
    reverb_algorithm_9_at_full_decay_stays_bounded = 9,
    reverb_algorithm_10_at_full_decay_stays_bounded = 10,
    reverb_algorithm_11_at_full_decay_stays_bounded = 11,
    reverb_algorithm_12_at_full_decay_stays_bounded = 12,
    reverb_algorithm_13_at_full_decay_stays_bounded = 13,
}

#[test]
fn the_shimmer_at_its_most_extreme_stays_bounded() {
    for (amount, voice2, shift2) in [
        (1.0, 1.0, 12.0),
        (1.0, 0.0, 12.0),
        (0.5, 1.0, 12.0),
        (1.0, 1.0, 7.0),
    ] {
        let mut r = NativeReverb::new(SR);
        for (n, v) in [
            ("algorithm", 6.0),
            ("decay", 1.0),
            ("size", 1.0),
            ("mix", 1.0),
            ("dry", 0.0),
            ("shim_amount", amount),
            ("shim_shift1", 12.0),
            ("shim_shift2", shift2),
            ("shim_voice2", voice2),
        ] {
            r.set_named(n, v);
        }
        assert_bounded(
            &format!("shimmer amount {amount} voice2 {voice2} +{shift2}"),
            &mut r,
        );
    }
}

#[test]
fn every_delay_style_at_top_feedback_stays_bounded() {
    for style in 0..=12 {
        let mut d = NativeDelay::new(SR);
        for (n, v) in [
            ("style", f64::from(style)),
            ("time", 250.0),
            ("feedback", 0.95),
            ("mix", 1.0),
            ("dry", 0.0),
            ("interval", 27.0),
            ("blend", 1.0),
        ] {
            d.set_named(n, v);
        }
        assert_bounded(&format!("delay style {style}"), &mut d);
    }
}
