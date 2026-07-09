//! Round-trip the `round_trip` example binary over every fixture.
//!
//! This is goal-item #2: `cargo test --workspace` must include a test that
//! exercises the binary on every fixture file.

use std::path::PathBuf;
use std::process::Command;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn round_trip_binary() -> PathBuf {
    // The example is a sibling of this test binary under the workspace target
    // dir: the test runs from `<target>/debug/deps/<name>-<hash>`, and the
    // example lands at `<target>/debug/examples/round_trip`. Deriving it from
    // `current_exe()` is independent of where this crate sits in the source
    // tree (it moved under features/backends/protools/), unlike walking a fixed
    // number of parents up from CARGO_MANIFEST_DIR.
    let exe = std::env::current_exe().expect("current_exe");
    let debug = exe
        .parent() // .../debug/deps
        .and_then(|p| p.parent()) // .../debug
        .expect("test binary path");
    debug.join("examples/round_trip")
}

#[test]
fn round_trip_every_fixture() {
    // Build the example once so every iteration uses the same binary.
    let build = Command::new(env!("CARGO"))
        .args(["build", "-p", "dawfile-protools", "--example", "round_trip"])
        .output()
        .expect("cargo build failed");
    assert!(
        build.status.success(),
        "failed to build round_trip example:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let bin = round_trip_binary();
    assert!(bin.exists(), "expected built example at {}", bin.display());

    let entries = std::fs::read_dir(fixtures_dir()).expect("read fixtures dir");

    let mut count = 0usize;
    let mut failures = Vec::new();
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(e) => e,
            None => continue,
        };
        if !matches!(ext, "ptx" | "ptf" | "pts") {
            continue;
        }
        count += 1;
        let out = Command::new(&bin)
            .arg(&path)
            .output()
            .expect("run round_trip example");
        if !out.status.success() {
            failures.push(format!(
                "{}:\n  stdout: {}\n  stderr: {}",
                path.display(),
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim(),
            ));
        }
    }

    assert!(count > 0, "no fixtures discovered");
    assert!(
        failures.is_empty(),
        "{} of {count} fixtures failed round-trip:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
