//! Wrapper `.app` binary entry point.
//!
//! When macOS launches a wrapper app (e.g., FTS-GUITAR.app), this binary
//! reads `Contents/launch.json`, patches `reaper.ini` with the configured
//! overrides (theme, undo memory, etc.), sets env vars, and execs REAPER.
//!
//! The `launch.json` is editable without recompiling — just change the
//! config and relaunch.

fn main() {
    reaper_launcher::launch();
}
