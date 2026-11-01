//! Standard FTS directory paths.
//!
//! All paths are derived from a single root — `fts_home()` — which auto-discovers
//! the installation directory:
//!
//! 1. `$FTS_HOME` env var (explicit override, always wins)
//! 2. `~/Music/FastTrackStudio/` (production install, if it exists)
//! 3. `~/Music/Dev/FastTrackStudio/` (dev fallback)

use std::path::PathBuf;

/// The FTS installation root.
///
/// Resolution order:
/// 1. `$FTS_HOME` environment variable (explicit override)
/// 2. `~/Music/FastTrackStudio/` if it exists (production install)
/// 3. `~/Music/Dev/FastTrackStudio/` (dev fallback)
pub fn fts_home() -> PathBuf {
    if let Ok(p) = std::env::var("FTS_HOME") {
        return PathBuf::from(p);
    }
    if let Some(home) = home_dir() {
        let production = home.join("Music/FastTrackStudio");
        // Check for the Reaper dir with an actual .app bundle — an empty
        // skeleton created by a stray REAPER launch doesn't count.
        if production.join("Reaper/reaper.ini").exists() {
            return production;
        }
        return home.join("Music/Dev/FastTrackStudio");
    }
    PathBuf::from("/tmp/FastTrackStudio")
}

/// `<fts_home>/Library/` — signal database, presets, catalog, config files.
pub fn library_dir() -> PathBuf {
    fts_home().join("Library")
}

/// `<fts_home>/Library/signal.db`
pub fn signal_db() -> PathBuf {
    library_dir().join("signal.db")
}

/// `<fts_home>/Library/assets/`
pub fn assets_dir() -> PathBuf {
    library_dir().join("assets")
}

/// `<fts_home>/Reaper/` — portable REAPER resource directory.
pub fn reaper_dir() -> PathBuf {
    fts_home().join("Reaper")
}

/// `<fts_home>/Reaper/UserPlugins/`
pub fn reaper_user_plugins() -> PathBuf {
    reaper_dir().join("UserPlugins")
}

/// `<fts_home>/Reaper/FXChains/`
pub fn reaper_fxchains() -> PathBuf {
    reaper_dir().join("FXChains")
}

/// `<fts_home>/Reaper/TrackTemplates/`
pub fn reaper_track_templates() -> PathBuf {
    reaper_dir().join("TrackTemplates")
}

/// `<fts_home>/Plug-Ins/`
pub fn plugins_dir() -> PathBuf {
    fts_home().join("Plug-Ins")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The source checkout this process runs from, when it runs from one: the
/// first directory holding the workspace's `Cargo.lock` and `features/`
/// above `$CARGO_MANIFEST_DIR` (set by `cargo run` and `cargo test`), the
/// executable (a dev build lives in the checkout's `target/`) or the
/// working directory. `None` for an installed app.
///
/// Found at run time on purpose: an `env!("CARGO_MANIFEST_DIR")` would bake
/// one checkout's absolute path into the crate, so no other worktree could
/// share its build from the compiler cache (kache keys that variable by its
/// full path), and a build would point at the checkout that compiled it.
pub fn checkout_root() -> Option<PathBuf> {
    let starts = [
        std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from),
        std::env::current_exe().ok(),
        std::env::current_dir().ok(),
    ];
    starts
        .into_iter()
        .flatten()
        .find_map(|start| find_checkout_above(&start))
}

/// `rel` inside the [`checkout_root`] — or, outside a checkout, relative to
/// the working directory, which is where it would be looked for anyway.
pub fn checkout_path(rel: impl AsRef<std::path::Path>) -> PathBuf {
    checkout_root().unwrap_or_default().join(rel)
}

fn find_checkout_above(start: &std::path::Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").is_file() && dir.join("features").is_dir())
        .map(std::path::Path::to_path_buf)
}
