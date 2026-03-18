//! Watches `UserPlugins/fts-extensions/` for guest executables and manages them.
//!
//! On startup, scans the directory and launches every executable found.
//! Then watches the directory for changes — when a binary is added, modified,
//! or replaced (e.g. via symlink update or cargo build), the old process is
//! killed and the new binary is launched automatically.
//!
//! This enables the hot-reload workflow:
//! 1. Symlink your guest binary into `fts-extensions/`
//! 2. Rebuild it (`cargo build -p my-guest`)
//! 3. The watcher detects the change, kills the old process, spawns the new one
//! 4. The new guest connects to REAPER via SHM — no restart needed

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;

use notify::{EventKind, RecursiveMode, Watcher};
use reaper_high::Reaper;
use tracing::{info, warn};

/// Tracked guest processes, keyed by filename.
struct GuestRegistry {
    children: HashMap<String, Child>,
    bootstrap_sock: String,
    extensions_dir: PathBuf,
}

impl GuestRegistry {
    fn new(bootstrap_sock: String, extensions_dir: PathBuf) -> Self {
        Self {
            children: HashMap::new(),
            bootstrap_sock,
            extensions_dir,
        }
    }

    /// Launch (or relaunch) a guest by filename.
    fn launch(&mut self, name: &str) {
        let path = self.extensions_dir.join(name);

        // Check it's a file and executable
        if !path.is_file() {
            return;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = path.metadata() {
                if meta.permissions().mode() & 0o111 == 0 {
                    return; // not executable
                }
            }
        }

        // Kill existing process for this name
        self.kill(name);

        info!("Launching guest extension: {}", name);

        match Command::new(&path)
            .env("FTS_SHM_BOOTSTRAP_SOCK", &self.bootstrap_sock)
            .spawn()
        {
            Ok(child) => {
                info!("Guest '{}' spawned (pid {})", name, child.id());
                self.children.insert(name.to_string(), child);
            }
            Err(e) => {
                warn!("Failed to launch guest '{}': {}", name, e);
            }
        }
    }

    /// Kill a tracked guest by filename.
    fn kill(&mut self, name: &str) {
        if let Some(mut child) = self.children.remove(name) {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Already exited
                }
                Ok(None) => {
                    info!("Killing guest '{}' (pid {})", name, child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(e) => {
                    warn!("Failed to check guest '{}': {}", name, e);
                }
            }
        }
    }

    /// Initial scan: launch all executables in the directory.
    fn scan_and_launch_all(&mut self) {
        let entries = match std::fs::read_dir(&self.extensions_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    "Failed to read fts-extensions dir {}: {}",
                    self.extensions_dir.display(),
                    e
                );
                return;
            }
        };

        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                self.launch(name);
            }
        }
    }
}

/// Discover the `fts-extensions/` directory inside REAPER's UserPlugins.
/// Creates it if it doesn't exist.
fn fts_extensions_dir() -> Option<PathBuf> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let resource_path = medium.get_resource_path(|p| p.to_path_buf());
    let dir = resource_path
        .into_std_path_buf()
        .join("UserPlugins")
        .join("fts-extensions");

    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!(
                "Failed to create fts-extensions directory at {}: {}",
                dir.display(),
                e
            );
            return None;
        }
        info!("Created fts-extensions directory: {}", dir.display());
    }

    Some(dir)
}

/// Launch all existing guests and start watching for changes.
///
/// When a file in `fts-extensions/` is created, modified, or replaced,
/// the corresponding guest process is (re)launched automatically.
pub fn launch_guests(bootstrap_sock: &str) {
    let dir = match fts_extensions_dir() {
        Some(d) => d,
        None => return,
    };

    let registry = std::sync::Arc::new(Mutex::new(GuestRegistry::new(
        bootstrap_sock.to_string(),
        dir.clone(),
    )));

    // Initial scan
    {
        let mut reg = registry.lock().unwrap();
        reg.scan_and_launch_all();
    }

    // Start file watcher
    let watch_registry = std::sync::Arc::clone(&registry);
    let watch_dir = dir.clone();

    // notify's RecommendedWatcher uses inotify on Linux, FSEvents on macOS
    let mut watcher =
        match notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    warn!("File watcher error: {}", e);
                    return;
                }
            };

            // We care about creates, modifications, and removes
            let dominated = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            );
            if !dominated {
                return;
            }

            let mut reg = watch_registry.lock().unwrap();

            for path in &event.paths {
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        info!(
                            "Detected change in fts-extensions: {} — (re)launching",
                            name
                        );
                        reg.launch(&name);
                    }
                    EventKind::Remove(_) => {
                        info!(
                            "Detected removal in fts-extensions: {} — killing guest",
                            name
                        );
                        reg.kill(&name);
                    }
                    _ => {}
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create file watcher: {}", e);
                return;
            }
        };

    if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
        warn!(
            "Failed to watch fts-extensions dir {}: {}",
            watch_dir.display(),
            e
        );
        return;
    }

    info!(
        "Watching fts-extensions directory for changes: {}",
        watch_dir.display()
    );

    // Keep the watcher alive by leaking it — it runs for the lifetime of REAPER.
    // The watcher's background thread will be cleaned up when the process exits.
    std::mem::forget(watcher);
    // Keep registry alive too (watcher callback holds an Arc to it)
    std::mem::forget(registry);
}
