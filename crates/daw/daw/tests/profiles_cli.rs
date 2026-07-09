//! CLI tests for DAW launch profile discovery.

#![cfg(feature = "cli")]

use serde_json::Value;
use std::process::Command;

fn daw_json(args: &[&str]) -> eyre::Result<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_daw"))
        .arg("--json")
        .args(args)
        .output()?;
    if !output.status.success() {
        eyre::bail!(
            "daw {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn profiles_include_reaper_dev_and_sandbox_configs() -> eyre::Result<()> {
    let value = daw_json(&["profiles"])?;
    let profiles = value
        .as_array()
        .ok_or_else(|| eyre::eyre!("profiles output should be an array: {value:#?}"))?;

    let by_id = |id: &str| {
        profiles
            .iter()
            .find(|profile| profile["id"].as_str() == Some(id))
    };

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    assert_eq!(
        by_id("fasttrackstudio")
            .and_then(|p| p["resources_dir"].as_str())
            .map(str::to_string),
        Some(format!("{home}/.fasttrackstudio/Reaper"))
    );

    let fts_dev = by_id("fts-dev").ok_or_else(|| eyre::eyre!("missing fts-dev profile"))?;
    assert_eq!(fts_dev["daw"], "reaper");
    assert_eq!(fts_dev["role"], "dev");
    assert_eq!(fts_dev["sandboxed"], false);

    let sandbox = by_id("sandbox").ok_or_else(|| eyre::eyre!("missing sandbox profile"))?;
    assert_eq!(sandbox["daw"], "reaper");
    assert_eq!(sandbox["role"], "sandbox");
    assert_eq!(sandbox["sandboxed"], true);

    Ok(())
}
