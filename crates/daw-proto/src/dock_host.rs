//! Dock Host Service — platform-portable dock/window management.
//!
//! Abstracts over the OS-level container that hosts UI panels:
//!
//! - REAPER docker (`daw-reaper-dioxus`)
//! - Standalone native window (future `daw-standalone` adapter)
//! - Browser DOM panel (future WASM adapter)
//! - In-memory mock (`daw-reaper` test utilities)
//!
//! Component rendering (Dioxus, egui, etc.) is intentionally out of scope —
//! the host trait only describes window/dock lifecycle. Apps pick one
//! component framework per binary.

use facet::Facet;
use vox::{Tx, service};

/// Opaque handle returned by `register_dock`. Stable for the lifetime of the
/// registration; callers pass it back to `show`/`hide`/`is_visible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Facet)]
pub struct DockHandle(pub u64);

/// Events emitted by the dock host as the user manipulates docks.
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum DockEvent {
    /// User opened/showed a dock.
    Shown(DockHandle),
    /// User closed/hid a dock.
    Hidden(DockHandle),
    /// User moved or resized a dock; the host has persisted the new layout.
    LayoutChanged,
}

/// Hint about the kind of host window the dock should produce.
///
/// Adapters may ignore hints they cannot honor (e.g. a browser adapter
/// produces a DOM panel regardless of `Floating`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Facet)]
pub enum DockKind {
    /// Tabbed alongside the host's native panels (e.g. REAPER docker).
    #[default]
    Tabbed,
    /// Free-floating window.
    Floating,
    /// Embedded inside an existing host window (e.g. transport area).
    Embedded,
}

/// Service for managing platform docks/panels.
///
/// Implemented by a host adapter (REAPER, standalone, browser, mock).
/// Guests/extensions register docks and toggle visibility without knowing
/// which adapter is in use.
#[service]
pub trait DockHostService {
    /// Register a dock by stable string id. Returns a handle for subsequent
    /// calls. If `id` is already registered, returns the existing handle.
    async fn register_dock(&self, id: String, title: String, kind: DockKind) -> DockHandle;

    /// Unregister a dock. Idempotent — returns `false` if the handle was
    /// already gone.
    async fn unregister_dock(&self, handle: DockHandle) -> bool;

    /// Show the dock. Idempotent.
    async fn show(&self, handle: DockHandle);

    /// Hide the dock. Idempotent.
    async fn hide(&self, handle: DockHandle);

    /// Toggle visibility. Returns the new visibility state.
    async fn toggle(&self, handle: DockHandle) -> bool;

    /// Query current visibility.
    async fn is_visible(&self, handle: DockHandle) -> bool;

    /// Serialize the current layout (positions, visibility, dock-vs-floating)
    /// for persistence. The blob is opaque — only the same adapter version
    /// guarantees round-trip.
    async fn save_layout(&self) -> Vec<u8>;

    /// Restore a previously-saved layout blob. Returns `false` if the blob
    /// is unrecognized; the dock host must remain in a usable state.
    async fn restore_layout(&self, blob: Vec<u8>) -> bool;

    /// Subscribe to dock lifecycle events. Multiple subscribers supported.
    async fn subscribe_dock_events(&self, tx: Tx<DockEvent>);
}
