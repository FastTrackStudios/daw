//! Plugin Loader Service
//!
//! Allows SHM guests to request eager loading of REAPER plugins (.clap, .vst3)
//! by calling `ReaperPluginEntry` on dynamically loaded libraries. This follows
//! the Helgobox pattern where an extension loads plugins into REAPER's address
//! space at startup.

use facet::Facet;
use roam::service;

/// Result of a plugin load operation.
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum PluginLoadResult {
    /// Plugin loaded and initialized successfully.
    Ok,
    /// Plugin loading failed.
    Error(String),
    /// Plugin was already loaded (idempotent).
    AlreadyLoaded,
}

/// Information about a loaded plugin.
#[derive(Debug, Clone, Facet)]
pub struct LoadedPluginInfo {
    /// Filesystem path the plugin was loaded from.
    pub path: String,
    /// Human-readable name derived from the path.
    pub name: String,
}

/// Service for eagerly loading REAPER plugins into the host process.
///
/// Plugins are loaded via `dlopen` + `ReaperPluginEntry`, giving them their own
/// REAPER context (separate statics, TaskSupport, etc.). Loaded libraries are
/// kept alive for the process lifetime.
#[service]
pub trait PluginLoaderService {
    /// Load a plugin from the given filesystem path.
    ///
    /// The path should point to the actual binary inside the plugin bundle
    /// (e.g., `.clap/Contents/MacOS/plugin-name` or `.clap/Contents/x86_64-linux/plugin-name.so`).
    async fn load_plugin(&self, plugin_path: String) -> PluginLoadResult;

    /// List all currently loaded plugins.
    async fn list_loaded(&self) -> Vec<LoadedPluginInfo>;

    /// Check if a plugin at the given path is already loaded.
    async fn is_loaded(&self, plugin_path: String) -> bool;
}
