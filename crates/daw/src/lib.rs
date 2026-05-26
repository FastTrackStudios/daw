//! Unified facade for DAW interaction.
//!
//! This is the single public API surface for the `daw` domain. External consumers
//! should depend only on this crate — never on internal crates directly.
//!
//! # Namespaces
//!
//! - **`rpc`** → `daw::rpc` — Async Vox control API (`Daw`, `Project`, `Transport`, `TrackHandle`,
//!   `FxChain`, `BatchBuilder`, etc.). Use from UI, remote clients, background tasks.
//! - **`service`** → `daw::service` — Raw protocol types and service clients.
//!
//! # Feature-gated modules
//!
//! - **`standalone`** → `daw::standalone` — Reference implementation for testing.
//! - **`file`** → `daw::file` — RPP file format parser.

// ── RPC: async Vox control API ──────────────────────────────────────────────
//
// All async client types (`Daw`, `Project`, `Transport`, `TrackHandle`, etc.)
// live under `daw::rpc`. Use from UI, remote clients, background tasks, tests.
//
// In-process extension code that needs native sync access should use the
// service traits from `daw::service` and receive a backend from its host.
pub mod rpc {
    pub use daw_control::*;
}

// Service composition primitives — re-exported from architect so apps
// depending only on `daw` can compose service bundles via `Services`,
// `Layer::merge`, and the `layers!` macro without a direct architect
// dep. Backend crates use these traits/macros to expose their service bundles.
pub use architect::{Descriptors, Layer, LayerRouter, Mounted, Services, layers};

/// Backend-agnostic main-thread affinity. Service code calls
/// `daw::main_thread::query` / `run`; REAPER installs a `TaskSupport`-backed
/// executor, standalone installs none (the work runs inline). See
/// [`daw_proto::main_thread`].
pub use daw_proto::main_thread;

// Internal alias for the bootstrap singleton.
use rpc::Daw;

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
    if let Some(host_ptr) = raw_host_context
        && let Some((daw, runtime)) = daw_reaper::bootstrap::create_plugin_daw(host_ptr)
    {
        // Register internal timer that fires user callbacks
        daw_reaper::bootstrap::register_internal_timer(|| {
            _fire_timer_callbacks();
        });

        let _ = DAW_INSTANCE.set(DawInstance {
            daw,
            runtime,
            _timer_callbacks: std::sync::Mutex::new(Vec::new()),
        });
        return true;
    }

    let _ = raw_host_context;
    false
}

/// Initialize the facade from an already constructed DAW handle.
///
/// Extension hosts that build a custom in-process DAW service graph can use this
/// to make `daw::get()` and `daw::block_on()` available to reusable modules.
pub fn init_from_parts(daw: rpc::Daw, runtime: std::sync::Arc<tokio::runtime::Runtime>) -> bool {
    if DAW_INSTANCE.get().is_some() {
        return true;
    }

    DAW_INSTANCE
        .set(DawInstance {
            daw,
            runtime,
            _timer_callbacks: std::sync::Mutex::new(Vec::new()),
        })
        .is_ok()
}

/// Get the global `Daw` handle.
///
/// Returns `None` if [`init`] hasn't been called or failed.
/// Works the same whether in a CLAP plugin, extension, or standalone.
pub fn get() -> Option<&'static rpc::Daw> {
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
    if let Some(instance) = DAW_INSTANCE.get()
        && let Ok(mut cbs) = instance._timer_callbacks.lock()
    {
        cbs.push(callback);
    }
}

/// Called internally by the timer system to fire user callbacks.
#[doc(hidden)]
pub fn _fire_timer_callbacks() {
    if let Some(instance) = DAW_INSTANCE.get()
        && let Ok(cbs) = instance._timer_callbacks.lock()
    {
        for cb in cbs.iter() {
            cb();
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
pub fn main_thread_daw() -> Option<daw_reaper::DawMainThread> {
    daw_reaper::DawMainThread::try_new()
}

// ── Service: raw protocol types & service clients ───────────────────────────
/// Raw protocol types and service clients.
pub mod service {
    pub use daw_proto::*;
}

// ── Standalone: reference/mock implementation ───────────────────────────────
#[cfg(feature = "standalone")]
/// Standalone reference implementation for testing (mock data included).
pub mod standalone {
    pub use daw_standalone::*;
}

// ── REAPER backend re-export ────────────────────────────────────────────────
// `daw-bridge`, `daw-perf-test`, and the in-process sync engine reach
// through the facade to REAPER-side helpers (event_hub, safe_wrappers,
// register_*) without taking direct `daw-reaper` deps everywhere. No
// cycle: `daw-reaper` is a feature-gated dependency of this facade.
#[cfg(feature = "reaper")]
pub mod reaper {
    pub use daw_reaper::*;
}

// ── File: RPP file format parser ────────────────────────────────────────────
#[cfg(feature = "file")]
/// High-performance RPP (REAPER Project) file format parser, plus
/// cross-format project conversion ([`file::convert`], with the `convert`
/// feature).
pub mod file {
    pub use dawfile_reaper::*;

    /// Error from [`convert`].
    #[cfg(feature = "reaper")]
    #[derive(Debug)]
    pub enum ConvertError {
        /// The output extension isn't a supported conversion target.
        UnsupportedOutput(String),
        /// The conversion itself failed (unsupported input, parse/build error).
        Convert(String),
        /// Reading the input or writing the output failed.
        Io(std::io::Error),
    }

    #[cfg(feature = "reaper")]
    impl std::fmt::Display for ConvertError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ConvertError::UnsupportedOutput(ext) => {
                    write!(
                        f,
                        "unsupported output format '.{ext}' (supported target: .rpp)"
                    )
                }
                ConvertError::Convert(e) => write!(f, "conversion failed: {e}"),
                ConvertError::Io(e) => write!(f, "io error: {e}"),
            }
        }
    }

    #[cfg(feature = "reaper")]
    impl std::error::Error for ConvertError {}

    /// Convert a supported project file to another supported format, dispatching
    /// on file extensions.
    ///
    /// Supported today: any of `.ptx` / `.ptf` / `.pts` (Pro Tools), `.als`
    /// (Ableton), `.aaf`, `.dawproject` **→ `.rpp` (REAPER)**. Other output
    /// formats return [`ConvertError::UnsupportedOutput`]; unsupported inputs
    /// surface as [`ConvertError::Convert`]. Requires the `reaper` feature
    /// (enable both via the `convert` feature).
    ///
    /// ```no_run
    /// daw::file::convert("session.ptx", "session.rpp").unwrap();
    /// ```
    #[cfg(feature = "reaper")]
    pub fn convert(
        input: impl AsRef<std::path::Path>,
        output: impl AsRef<std::path::Path>,
    ) -> Result<(), ConvertError> {
        let input = input.as_ref();
        let output = output.as_ref();
        let out_ext = output
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match out_ext.as_str() {
            "rpp" => {
                let rpp = daw_reaper::project_import::convert_to_rpp(&input.to_string_lossy())
                    .map_err(|e| ConvertError::Convert(e.to_string()))?;
                std::fs::write(output, rpp).map_err(ConvertError::Io)
            }
            other => Err(ConvertError::UnsupportedOutput(other.to_string())),
        }
    }
}

// ── Extension runtime: in-process REAPER extension hosting ──────────────────
//
// `daw-extension-runtime` lives as a public sibling crate, not under the
// `daw` facade. The fold can't be done without a cycle:
//   daw → daw-extension-runtime → daw-reaper → keyflow → daw
// (daw-reaper uses keyflow for chord detection; keyflow uses daw::module).
// Extension authors depend on `daw` + `daw-extension-runtime` together.

// ── Sync stack: engine + network + Link ─────────────────────────────────────
//
// `daw-synchronization`, `daw-network`, and `daw-link` are also public
// sibling crates, not folded under the `daw` facade. Each depends on
// `daw` itself (synchronization on the streaming surface, network on
// `SyncEvent` from synchronization, link is standalone), so a facade
// re-export would cycle. Consumers (other domain crates that want to
// reuse the peer-mesh transport for non-sync use cases, integration
// tests, the daw CLI's `sync` subcommand) depend on these directly.
//
// - `daw-synchronization` — backend-agnostic sync engine + drift corrector
//   + heartbeat. Wraps the streaming surface from `daw::service` in
//   `SyncEvent` envelopes.
// - `daw-network`       — TCP peer mesh + handshake + clock calibration.
//   Concrete on `SyncEvent` today; library-shaped so other domains can
//   reuse the transport.
// - `daw-link`          — Ableton Link adapter (host-agnostic via the
//   `LinkCallbacks` trait).
//
// `daw-bridge` wires all three together inside its `sync` cargo feature
// (default-on), gated at runtime by `FTS_SYNC_ENABLED=1`. See
// `daw-bridge/src/sync_runtime.rs` and `docs/sync-stack-consolidation.md`.

// ── Integration-test harness ────────────────────────────────────────────────
//
// Enabled via `features = ["test-harness"]`. Folds the former `reaper-test`
// crate in as a feature-gated module that drives `daw-reaper` over the daw
// RPC surface. Generalized over the `DawBackend` trait (single concrete impl:
// `ReaperBackend`). Re-exports the `#[daw_test]` / `#[reaper_test]` macros.
#[cfg(feature = "test-harness")]
pub mod test;

// ── Streaming ergonomics ────────────────────────────────────────────────────
pub use stream::RxExt;
mod stream;

// ── Module system: standard interface for extension modules ─────────────────
pub use daw_module as module;
pub use daw_module::{
    ActionDef, DawModule, DockPosition, ModuleContext, PanelComponent, PanelDef, PanelRenderer,
};

// ── UI host (REAPER + Dioxus) ──────────────────────────────────────────────
//
// Enabled via `features = ["reaper-ui"]`. Re-exports the curated public
// surface of `daw-reaper-dioxus` so apps depend only on the daw facade.
// Audio-graph / WASM / embedded targets get nothing pulled in — the GPU
// / blitz / dioxus-native stack only enters the dependency graph when the
// feature is on.
#[cfg(feature = "reaper-ui")]
pub mod ui {
    /// Dock host implementation backed by REAPER's docker + Dioxus.
    pub mod dock {
        pub use daw_reaper_dioxus::dock::{
            DockablePanelConfig, hide_panel, init as init_dock, is_panel_visible, prewarm_panel,
            register_panel, register_panel_from_service, restore_dock_state, save_dock_state,
            show_panel, toggle_panel, unregister_all_panels, update_panels,
        };
        pub use daw_reaper_dioxus::service::{init as init_service, register_panel_from_def};
    }

    /// Embedded Dioxus view inside an existing REAPER HWND.
    pub use daw_reaper_dioxus::EmbeddedView;
    /// Transparent floating overlay (HUDs, popups).
    pub use daw_reaper_dioxus::{DioxusOverlay, DioxusOverlayBuilder, OverlayConfig};

    /// Pixel snapshot helpers — render a Dioxus root component to an
    /// off-screen GPU surface and assert against a golden byte file.
    /// Useful for component-level visual regression tests in
    /// fts-extensions or any other consumer.
    pub mod snapshot {
        pub use daw_reaper_dioxus::snapshot::{
            SnapshotError, compare_to_golden, render_panel_offscreen,
        };
    }

    /// Dioxus prelude — re-exported for component authors so they don't have
    /// to depend on `dioxus-native` directly.
    pub use daw_reaper_dioxus::prelude;
}
