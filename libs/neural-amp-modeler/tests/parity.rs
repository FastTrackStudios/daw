//! Parity tests: run the same .nam model + input through the C++ core (via
//! the FFI `NamModel`) and the pure-Rust engine (`pure::PureNamModel`), and
//! require the outputs to agree within a small tolerance.
//!
//! Both engines compute in f32 but with different accumulation orders
//! (Eigen GEMM vs naive loops), so exact bit equality is not expected.

#![cfg(not(target_arch = "wasm32"))]

use neural_amp_modeler::{pure::PureNamModel, NamModel};
use std::path::PathBuf;

const SAMPLE_RATE: f64 = 48000.0;
const BUFFER_SIZE: usize = 512;
const NUM_SAMPLES: usize = 8192;

/// Deterministic test signal: a few sine partials with an amplitude ramp and
/// a touch of deterministic "noise" so the nonlinearity is well exercised.
fn test_signal(n: usize) -> Vec<f64> {
    let mut state: u64 = 0x243F6A8885A308D3;
    (0..n)
        .map(|i| {
            let t = i as f64 / SAMPLE_RATE;
            // xorshift for deterministic noise
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state as f64 / u64::MAX as f64) * 2.0 - 1.0;
            let ramp = (i as f64 / n as f64).min(1.0);
            ramp * (0.4 * (2.0 * std::f64::consts::PI * 110.0 * t).sin()
                + 0.2 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()
                + 0.05 * noise)
        })
        .collect()
}

fn process_native(path: &PathBuf, input: &[f64]) -> Vec<f64> {
    let mut m = NamModel::load(path).unwrap_or_else(|e| panic!("native load {path:?}: {e}"));
    m.reset(SAMPLE_RATE, BUFFER_SIZE);
    let mut out = vec![0.0f64; input.len()];
    for (ic, oc) in input.chunks(BUFFER_SIZE).zip(out.chunks_mut(BUFFER_SIZE)) {
        m.process(ic, oc);
    }
    out
}

fn process_pure(path: &PathBuf, input: &[f64]) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap();
    let mut m =
        PureNamModel::from_bytes(&bytes).unwrap_or_else(|e| panic!("pure load {path:?}: {e}"));
    m.reset(SAMPLE_RATE, BUFFER_SIZE);
    let mut out = vec![0.0f64; input.len()];
    m.process(input, &mut out);
    out
}

fn compare(name: &str, a: &[f64], b: &[f64]) {
    let rms = (a.iter().map(|v| v * v).sum::<f64>() / a.len() as f64).sqrt();
    let max_abs = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f64, f64::max);
    let rel = if rms > 0.0 { max_abs / rms } else { max_abs };
    println!("{name}: output rms {rms:.6}, max abs diff {max_abs:.3e}, rel {rel:.3e}");
    assert!(
        max_abs < 5e-4 && rel < 2e-3,
        "{name}: parity failure — max abs diff {max_abs:.3e} (rel {rel:.3e}) exceeds tolerance"
    );
    // Sanity: the model actually did something.
    assert!(rms > 1e-6, "{name}: output is silent");
}

fn parity_for_dir(dir: PathBuf) {
    let mut checked = 0;
    let input = test_signal(NUM_SAMPLES);
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "nam"))
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // Skip models the pure engine explicitly does not support.
        let bytes = std::fs::read(&path).unwrap();
        let pure = match PureNamModel::from_bytes(&bytes) {
            Ok(_) => process_pure(&path, &input),
            Err(e) => {
                println!("{name}: skipped (unsupported by pure engine: {e})");
                continue;
            }
        };
        let native = process_native(&path, &input);
        compare(&name, &native, &pure);
        checked += 1;
    }
    assert!(checked > 0, "no models were parity-checked in {dir:?}");
}

/// The 11 real rig models (WaveNet 0.5.4 + SlimmableContainer/WaveNet 0.7.0).
#[test]
fn parity_rig_models() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../features/rigs/guitar/default-config/models");
    if !dir.exists() {
        eprintln!("rig models dir not found; skipping");
        return;
    }
    parity_for_dir(dir);
}

/// The NeuralAmpModelerCore example models (tiny WaveNet + LSTM).
#[test]
fn parity_example_models() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("NeuralAmpModelerCore")
        .join("example_models");
    parity_for_dir(dir);
}

/// Native from_bytes must match native load-from-path exactly.
#[test]
fn native_from_bytes_matches_load() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("NeuralAmpModelerCore/example_models/wavenet.nam");
    let input = test_signal(4096);

    let from_path = process_native(&path, &input);

    let bytes = std::fs::read(&path).unwrap();
    let mut m = NamModel::from_bytes(&bytes).unwrap();
    m.reset(SAMPLE_RATE, BUFFER_SIZE);
    let mut from_bytes = vec![0.0f64; input.len()];
    for (ic, oc) in input
        .chunks(BUFFER_SIZE)
        .zip(from_bytes.chunks_mut(BUFFER_SIZE))
    {
        m.process(ic, oc);
    }

    assert_eq!(from_path, from_bytes, "from_bytes must be bit-identical to load()");
}
