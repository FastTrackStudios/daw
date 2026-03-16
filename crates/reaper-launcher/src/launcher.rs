//! REAPER instance launcher.
//!
//! Reads `launch.json` from the wrapper bundle's `Contents/` directory,
//! patches `reaper.ini`, sets environment variables, and execs REAPER.
//! If `restore_ini_after_launch` is set, forks first so the parent can
//! restore the original INI values after REAPER has started.

use crate::config::LaunchConfig;
use crate::ini::ReaperIni;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Launch REAPER based on the config found relative to the current executable.
///
/// This is the main entry point for wrapper `.app` binaries.
/// It reads `launch.json` from `../` relative to the binary (i.e.,
/// `Contents/launch.json` when the binary is at `Contents/MacOS/REAPER`).
pub fn launch() -> ! {
    let exe = std::env::current_exe().expect("Failed to get current exe path");
    // exe is at .../Contents/MacOS/REAPER
    // launch.json is at .../Contents/launch.json
    let contents_dir = exe
        .parent() // MacOS/
        .and_then(|p| p.parent()) // Contents/
        .expect("Invalid bundle structure: expected Contents/MacOS/REAPER");

    let config_path = contents_dir.join("launch.json");
    let config = LaunchConfig::load(&config_path)
        .unwrap_or_else(|e| panic!("Failed to load launch config: {e}"));

    launch_with_config(&config);
}

/// Launch REAPER with the given config. Does not return.
pub fn launch_with_config(config: &LaunchConfig) -> ! {
    let ini = ReaperIni::new(&config.ini_path);

    // Build the list of ini patches
    let patches = config.ini_overrides.as_patches();
    let patch_refs: Vec<(&str, &str)> = patches.iter().map(|(k, v)| (*k, v.as_str())).collect();

    if config.restore_ini_after_launch && !patch_refs.is_empty() {
        // Fork: child launches REAPER, parent restores ini after a delay
        launch_with_restore(&ini, &patch_refs, config);
    } else {
        // Simple path: patch and exec (no restore needed)
        if !patch_refs.is_empty() {
            if let Err(e) = ini.patch(&patch_refs) {
                eprintln!("Warning: failed to patch reaper.ini: {e}");
            }
        }
        exec_reaper(config);
    }
}

/// Fork, patch ini, exec REAPER in child, restore ini in parent.
fn launch_with_restore(
    ini: &ReaperIni,
    patches: &[(&str, &str)],
    config: &LaunchConfig,
) -> ! {
    // Save originals before patching
    let originals = ini.patch(patches).unwrap_or_else(|e| {
        eprintln!("Warning: failed to patch reaper.ini: {e}");
        std::collections::HashMap::new()
    });

    // Safety: we're about to fork. No threads should be running.
    let pid = unsafe { libc::fork() };

    match pid {
        -1 => {
            // Fork failed — just exec without restore
            eprintln!("Warning: fork failed, launching without ini restore");
            exec_reaper(config);
        }
        0 => {
            // Child: exec REAPER
            exec_reaper(config);
        }
        _parent => {
            // Parent: wait for REAPER to read its ini, then restore
            // 3 seconds is plenty for REAPER to parse reaper.ini on startup
            std::thread::sleep(std::time::Duration::from_secs(3));

            if let Err(e) = ini.restore(&originals) {
                eprintln!("Warning: failed to restore reaper.ini: {e}");
            }

            std::process::exit(0);
        }
    }
}

/// Set env vars and exec the real REAPER binary. Does not return.
fn exec_reaper(config: &LaunchConfig) -> ! {
    // Safety: we are single-threaded at this point (pre-exec), no other
    // threads are reading env vars concurrently.
    unsafe {
        std::env::set_var("FTS_DAW_ROLE", &config.role);
        if let Some(ref rig_type) = config.rig_type {
            std::env::set_var("FTS_RIG_TYPE", rig_type);
        }
    }

    std::env::set_current_dir(&config.resources_dir)
        .unwrap_or_else(|e| eprintln!("Warning: failed to chdir: {e}"));

    let args: Vec<&str> = vec!["-newinst", "-nosplash", "-ignoreerrors"];

    // Collect any extra args passed to this binary
    let extra_args: Vec<String> = std::env::args().skip(1).collect();

    let err = Command::new(&config.reaper_executable)
        .args(&args)
        .args(&extra_args)
        .exec();

    // exec() only returns on error
    panic!("Failed to exec REAPER: {err}");
}
