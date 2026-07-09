//! Parse → write → re-parse round-trip integrity check.
//!
//! Reads a Pro Tools session, re-encrypts the byte buffer via
//! `RawSession::encrypt()`, parses the result back to a `ProToolsSession`,
//! and asserts every public field is equal.
//!
//! Usage:
//!   cargo run -p dawfile-protools --example round_trip <path>

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: {} <path-to-session>", args[0]);
        return ExitCode::from(2);
    }
    let path = PathBuf::from(&args[1]);

    match run(&path) {
        Ok(()) => {
            println!("round-trip OK: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("round-trip FAIL: {}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn run(path: &PathBuf) -> Result<(), String> {
    // 1. Original parse
    let session_a = dawfile_protools::read_session(path, 0)
        .map_err(|e| format!("initial parse failed: {e}"))?;

    // 2. Read raw, re-encrypt, write to in-memory buffer
    let original = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    let raw = dawfile_protools::parse_raw(original.clone())
        .map_err(|e| format!("parse_raw failed: {e}"))?;
    let re_encrypted = raw.encrypt();

    // Byte-level identity (must hold for unknown-block passthrough)
    if re_encrypted.len() != original.len() {
        return Err(format!(
            "byte length changed: orig={} encrypted={}",
            original.len(),
            re_encrypted.len()
        ));
    }
    for (i, (a, b)) in original.iter().zip(re_encrypted.iter()).enumerate() {
        if a != b {
            return Err(format!(
                "byte mismatch at 0x{i:04x}: orig=0x{a:02x} re-encrypted=0x{b:02x}"
            ));
        }
    }

    // 3. Re-parse the re-encrypted bytes
    let mut buf = re_encrypted;
    let session_b = dawfile_protools::parse::parse_session(&mut buf, 0)
        .map_err(|e| format!("re-parse failed: {e}"))?;

    // 4. Field-by-field equality.
    compare_sessions(&session_a, &session_b)?;

    Ok(())
}

fn compare_sessions(
    a: &dawfile_protools::ProToolsSession,
    b: &dawfile_protools::ProToolsSession,
) -> Result<(), String> {
    macro_rules! check {
        ($field:ident) => {
            if a.$field != b.$field {
                return Err(format!(
                    "field `{}` mismatch:\n  a = {:?}\n  b = {:?}",
                    stringify!($field),
                    a.$field,
                    b.$field
                ));
            }
        };
    }
    check!(version);
    check!(session_sample_rate);
    check!(bpm);
    check!(tempo_events);
    check!(meter_events);
    check!(markers);
    check!(audio_files);
    check!(audio_regions);
    check!(audio_tracks);
    check!(midi_regions);
    check!(midi_tracks);
    check!(plugins);
    check!(io_channels);
    Ok(())
}
