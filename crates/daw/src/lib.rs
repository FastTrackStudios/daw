//! Unified facade for DAW interaction.
//!
//! This is the single public API surface for the `daw` domain. External consumers
//! should depend only on this crate — never on internal crates directly.
//!
//! # Core (always available, WASM-compatible)
//!
//! - **Root**: High-level control API — `Daw`, `Project`, `Track`, `FxChain`,
//!   `Transport`, etc.
//! - **`service`**: Raw protocol types and service clients.
//!
//! # Feature-gated modules
//!
//! - **`sync`** → `daw::sync` — Blocking wrapper for audio plugins (`DawSync`, `LocalCaller`).
//! - **`reaper`** → `daw::reaper` — REAPER-specific implementations.
//! - **`standalone`** → `daw::standalone` — Reference implementation for testing.
//! - **`file`** → `daw::file` — RPP file format parser.

// ── Core: high-level control API (WASM-compatible) ──────────────────────────
pub use daw_control::*;

// ── Plugin API: DAW-agnostic initialization ─────────────────────────────────

use std::sync::OnceLock;

static DAW_INSTANCE: OnceLock<DawInstance> = OnceLock::new();

struct DawInstance {
    daw: Daw,
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
    _timer_callbacks: std::sync::Mutex<Vec<fn()>>,
}

/// Initialize DAW access from a plugin context.
///
/// Detects the host environment (REAPER via CLAP, standalone, etc.)
/// and creates the appropriate `Daw` instance. Call once from
/// `Plugin::initialize()`.
///
/// ```rust,ignore
/// fn initialize(&mut self, ..., context: &mut impl InitContext<Self>) -> bool {
///     daw::init(context.raw_host_context());
///     true
/// }
/// ```
pub fn init(raw_host_context: Option<*const std::ffi::c_void>) -> bool {
    if DAW_INSTANCE.get().is_some() {
        return true;
    }

    #[cfg(feature = "reaper")]
    if let Some(host_ptr) = raw_host_context {
        if let Some((daw, runtime)) = reaper::bootstrap::create_plugin_daw(host_ptr) {
            // Register internal timer that fires user callbacks
            reaper::bootstrap::register_internal_timer(|| {
                _fire_timer_callbacks();
            });

            let _ = DAW_INSTANCE.set(DawInstance {
                daw,
                runtime,
                _timer_callbacks: std::sync::Mutex::new(Vec::new()),
            });
            return true;
        }
    }

    let _ = raw_host_context;
    false
}

/// Get the global `Daw` handle.
///
/// Returns `None` if [`init`] hasn't been called or failed.
/// Works the same whether in a CLAP plugin, extension, or standalone.
pub fn get() -> Option<&'static Daw> {
    DAW_INSTANCE.get().map(|i| &i.daw)
}

/// Run an async operation on the DAW runtime.
///
/// Use from sync contexts (timer callbacks, process()) to call
/// async `Daw` methods.
pub fn block_on<F: std::future::Future>(f: F) -> Option<F::Output> {
    DAW_INSTANCE.get().map(|i| i.runtime.block_on(f))
}

/// Register a callback that fires at ~30Hz on the DAW's main thread.
pub fn register_timer(callback: fn()) {
    if let Some(instance) = DAW_INSTANCE.get() {
        if let Ok(mut cbs) = instance._timer_callbacks.lock() {
            cbs.push(callback);
        }
    }
}

/// Called internally by the timer system to fire user callbacks.
#[doc(hidden)]
pub fn _fire_timer_callbacks() {
    if let Some(instance) = DAW_INSTANCE.get() {
        if let Ok(cbs) = instance._timer_callbacks.lock() {
            for cb in cbs.iter() {
                cb();
            }
        }
    }
}

// ── Main Thread Sync API ─────────────────────────────────────────────────────

/// Get a sync DAW handle for code running on REAPER's main thread.
///
/// Returns `None` if not on the main thread or not in REAPER.
/// This is the zero-overhead path for timer callbacks.
///
/// ```rust,ignore
/// fn my_timer() {
///     let daw = daw::main_thread_daw().unwrap();
///     let tracks = daw.track_list();
///     daw.fx_param_set("guid", 0, 2, 0.5);
/// }
/// ```
#[cfg(feature = "reaper")]
pub fn main_thread_daw() -> Option<reaper::DawMainThread> {
    reaper::DawMainThread::try_new()
}

// ── Service: raw protocol types & service clients ───────────────────────────
/// Raw protocol types and service clients.
pub mod service {
    pub use daw_proto::*;
}

// ── Sync: blocking wrapper for audio plugins ────────────────────────────────
#[cfg(feature = "sync")]
/// Synchronous (blocking) DAW control API for real-time audio contexts.
pub mod sync {
    pub use daw_control_sync::*;
}

// ── Reaper: REAPER-specific implementations ─────────────────────────────────
#[cfg(feature = "reaper")]
/// REAPER DAW implementation — in-process service dispatchers.
pub mod reaper {
    pub use daw_reaper::*;
}

// ── Standalone: reference/mock implementation ───────────────────────────────
#[cfg(feature = "standalone")]
/// Standalone reference implementation for testing (mock data included).
pub mod standalone {
    pub use daw_standalone::*;
}

// ── File: RPP file format parser ────────────────────────────────────────────
#[cfg(feature = "file")]
/// High-performance RPP (REAPER Project) file format parser.
pub mod file {
    pub use dawfile_reaper::*;
}

// ── Module system: standard interface for extension modules ─────────────────
pub use daw_module as module;
pub use daw_module::{ActionDef, DawModule, DockPosition, ModuleContext, PanelComponent, PanelDef};
