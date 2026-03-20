//! Watches `UserPlugins/fts-extensions/` for guest executables and manages them.
//!
//! On startup, scans the directory and launches every executable found.
//! Then watches the directory for changes — when a binary is added, modified,
//! or replaced (e.g. via symlink update or cargo build), the old process is
//! killed and the new binary is launched automatically.
//!
//! For symlinked binaries, we also watch the symlink target so that rebuilds
//! (which modify the target, not the symlink) trigger a hot-reload.
//!
//! This enables the hot-reload workflow:
//! 1. Symlink your guest binary into `fts-extensions/`
//! 2. Rebuild it (`cargo build -p my-guest`)
//! 3. The watcher detects the change, kills the old process, spawns the new one
//! 4. The new guest connects to REAPER via SHM — no restart needed

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;

use notify::{EventKind, RecursiveMode, Watcher};
use reaper_high::Reaper;
use tracing::{info, warn};

#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;

/// Read the `.fts-ignore` file from the extensions directory.
///
/// Each non-empty, non-comment line is treated as an exact filename to skip.
/// Returns an empty set if the file doesn't exist.
fn read_ignore_list(extensions_dir: &PathBuf) -> HashSet<String> {
    let ignore_path = extensions_dir.join(".fts-ignore");
    let content = match std::fs::read_to_string(&ignore_path) {
        Ok(c) => c,
        Err(_) => return HashSet::new(),
    };

    let names: HashSet<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();

    if !names.is_empty() {
        info!(
            "Loaded .fts-ignore ({} entries): {:?}",
            names.len(),
            names
        );
    }

    names
}

/// Tracked guest processes, keyed by filename.
struct GuestRegistry {
    children: HashMap<String, Child>,
    bootstrap_sock: String,
    extensions_dir: PathBuf,
    ignore_list: HashSet<String>,
    /// Maps resolved symlink target path → guest name in fts-extensions/.
    /// Used to map inotify events on target files back to the guest name.
    symlink_targets: HashMap<PathBuf, String>,
    /// Maps resolved symlink target **directory** → guest name.
    /// Used when watching directories (inotify events come from the dir).
    symlink_target_dirs: HashMap<PathBuf, String>,
    /// Consecutive restart count per guest (reset on successful file-watcher reload).
    restart_count: HashMap<String, u32>,
}

/// Max consecutive auto-restarts before giving up (prevents crash loops).
const MAX_AUTO_RESTARTS: u32 = 3;

impl GuestRegistry {
    fn new(bootstrap_sock: String, extensions_dir: PathBuf) -> Self {
        let ignore_list = read_ignore_list(&extensions_dir);
        Self {
            children: HashMap::new(),
            bootstrap_sock,
            extensions_dir,
            ignore_list,
            symlink_targets: HashMap::new(),
            symlink_target_dirs: HashMap::new(),
            restart_count: HashMap::new(),
        }
    }

    /// Check if a guest name is blacklisted via `.fts-ignore`.
    fn is_ignored(&self, name: &str) -> bool {
        self.ignore_list.contains(name)
    }

    /// Reload the ignore list from disk (e.g. when `.fts-ignore` itself changes).
    fn reload_ignore_list(&mut self) {
        self.ignore_list = read_ignore_list(&self.extensions_dir);
    }

    /// Resolve a path to the guest name it belongs to.
    /// Checks exact symlink target match first, then checks if the path is
    /// inside a watched symlink target directory.
    fn resolve_target_to_name(&self, path: &std::path::Path) -> Option<String> {
        // Direct file match
        if let Some(name) = self.symlink_targets.get(path) {
            return Some(name.clone());
        }
        // Check if the event path is inside a watched directory
        if let Some(parent) = path.parent() {
            if let Some(name) = self.symlink_target_dirs.get(parent) {
                // Only match if the filename matches the target binary name
                if let Some(target_path) = self.symlink_targets.iter().find_map(|(target, n)| {
                    if n == name { Some(target) } else { None }
                }) {
                    if path.file_name() == target_path.file_name() {
                        return Some(name.clone());
                    }
                }
            }
        }
        None
    }

    /// Launch (or relaunch) a guest by filename.
    fn launch(&mut self, name: &str) {
        if self.is_ignored(name) {
            info!("Skipping ignored guest extension: {}", name);
            return;
        }

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

        let mut cmd = Command::new(&path);
        cmd.env("FTS_SHM_BOOTSTRAP_SOCK", &self.bootstrap_sock);

        // On Linux, ask the kernel to send SIGTERM to the child when the parent
        // (REAPER) dies. This prevents orphaned extension processes when REAPER
        // is killed or crashes without running cleanup.
        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }

        match cmd.spawn() {
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
                    info!("Guest '{}' already exited", name);
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

    /// Check all tracked children and relaunch any that have exited unexpectedly.
    /// Respects a per-guest restart limit to prevent infinite crash loops.
    fn reap_and_restart(&mut self) {
        // Collect names of exited children first to avoid borrow issues
        let exited: Vec<String> = self
            .children
            .iter_mut()
            .filter_map(|(name, child)| match child.try_wait() {
                Ok(Some(status)) => {
                    info!(
                        "Guest '{}' exited ({})",
                        name, status
                    );
                    Some(name.clone())
                }
                Ok(None) => None, // still running
                Err(e) => {
                    warn!("Failed to check guest '{}': {}", name, e);
                    None
                }
            })
            .collect();

        for name in exited {
            self.children.remove(&name);

            let count = self.restart_count.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count > MAX_AUTO_RESTARTS {
                warn!(
                    "Guest '{}' has crashed {} times — not restarting (use hot-reload to reset)",
                    name, count
                );
                continue;
            }
            info!("Auto-restarting guest '{}' (attempt {}/{})", name, count, MAX_AUTO_RESTARTS);
            self.launch(&name);
        }
    }

    /// Reset the restart counter for a guest (called on intentional hot-reload).
    fn reset_restart_count(&mut self, name: &str) {
        self.restart_count.remove(name);
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

    /// Collect symlink targets for all entries in the extensions directory.
    /// Returns the **directories** containing the resolved targets — we watch
    /// directories rather than individual files because cargo build uses atomic
    /// rename, which replaces the inode and breaks inotify file watches.
    fn collect_symlink_targets(&mut self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let mut seen_dirs = HashSet::new();
        let entries = match std::fs::read_dir(&self.extensions_dir) {
            Ok(e) => e,
            Err(_) => return dirs,
        };

        for entry in entries.flatten() {
            let link_path = entry.path();
            if !link_path.is_symlink() {
                continue;
            }
            let name = match entry.file_name().to_str() {
                Some(n) => n.to_string(),
                None => continue,
            };
            if self.is_ignored(&name) {
                continue;
            }
            match std::fs::canonicalize(&link_path) {
                Ok(target) => {
                    // Record exact file → name mapping (for resolve_target_to_name)
                    self.symlink_targets.insert(target.clone(), name.clone());

                    // Watch the parent directory instead of the file itself
                    if let Some(target_dir) = target.parent() {
                        let target_dir = target_dir.to_path_buf();
                        info!(
                            "Watching symlink target dir for '{}': {} (target: {})",
                            name,
                            target_dir.display(),
                            target.display()
                        );
                        self.symlink_target_dirs.insert(target_dir.clone(), name);
                        if seen_dirs.insert(target_dir.clone()) {
                            dirs.push(target_dir);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to resolve symlink for '{}': {}",
                        name, e
                    );
                }
            }
        }

        dirs
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
/// For symlinks, the resolved target is also watched so that rebuilding
/// the target binary triggers a hot-reload.
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
    let symlink_targets;
    {
        let mut reg = registry.lock().unwrap();
        reg.scan_and_launch_all();
        symlink_targets = reg.collect_symlink_targets();
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
                // Check if this is a symlink target change (directory or file)
                if let Some(guest_name) = reg.resolve_target_to_name(path) {
                    if matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_)
                    ) {
                        info!(
                            "Detected symlink target change for '{}' ({}) — hot-reloading",
                            guest_name,
                            path.display()
                        );
                        // Intentional hot-reload: reset crash counter
                        reg.reset_restart_count(&guest_name);
                        reg.launch(&guest_name);
                    }
                    continue;
                }

                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                // Reload ignore list when .fts-ignore itself changes
                if name == ".fts-ignore" {
                    info!("Detected .fts-ignore change — reloading ignore list");
                    reg.reload_ignore_list();
                    continue;
                }

                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        if reg.is_ignored(&name) {
                            info!(
                                "Detected change in fts-extensions: {} — skipped (ignored)",
                                name
                            );
                        } else {
                            info!(
                                "Detected change in fts-extensions: {} — (re)launching",
                                name
                            );
                            // Intentional hot-reload: reset crash counter
                            reg.reset_restart_count(&name);
                            reg.launch(&name);
                        }
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

    // Watch the extensions directory itself (for new files, non-symlink changes)
    if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
        warn!(
            "Failed to watch fts-extensions dir {}: {}",
            watch_dir.display(),
            e
        );
        return;
    }

    // Watch each symlink target directory so rebuilds trigger hot-reload.
    // We watch directories (not files) because cargo build uses atomic rename,
    // which replaces the inode and would break an inotify watch on the file itself.
    for target_dir in &symlink_targets {
        if let Err(e) = watcher.watch(target_dir, RecursiveMode::NonRecursive) {
            warn!(
                "Failed to watch symlink target dir {}: {}",
                target_dir.display(),
                e
            );
        }
    }

    info!(
        "Watching fts-extensions directory for changes: {} ({} symlink target dirs)",
        watch_dir.display(),
        symlink_targets.len()
    );

    // Spawn a monitoring thread that periodically checks for crashed/killed
    // guest processes and auto-restarts them.
    let monitor_registry = std::sync::Arc::clone(&registry);
    std::thread::Builder::new()
        .name("fts-guest-monitor".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let mut reg = monitor_registry.lock().unwrap();
                reg.reap_and_restart();
            }
        })
        .expect("failed to spawn guest monitor thread");

    // Keep the watcher alive by leaking it — it runs for the lifetime of REAPER.
    // The watcher's background thread will be cleaned up when the process exits.
    std::mem::forget(watcher);
    // Keep registry alive too (watcher callback and monitor thread hold Arcs to it)
    std::mem::forget(registry);
}
