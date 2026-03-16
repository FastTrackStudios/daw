//! reaper-launcher: Typed management of REAPER ini config and instance launching.
//!
//! Provides a safe, structured interface for:
//! - Reading/writing `reaper.ini` config variables
//! - Temporarily patching settings for a launch, then restoring originals
//! - Spawning REAPER instances with the correct env vars and arguments
//!
//! Used by the wrapper `.app` launcher binaries and by `xtask setup-rigs`.

mod ini;
mod config;
mod launcher;

pub use config::{LaunchConfig, ReaperIniConfig};
pub use ini::ReaperIni;
pub use launcher::launch;
