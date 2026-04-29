//! [`DockHostService`] implementation backed by the REAPER docker + Dioxus
//! dock module.
//!
//! Wraps the imperative `dock::*` helpers behind the platform-portable
//! `DockHostService` trait so callers (tests, RPC clients) can drive the
//! dock host without knowing it's REAPER underneath.
//!
//! Lifetime model: panels are still registered through
//! `dock::register_panel_from_service` (called from the host extension's
//! startup, e.g. fts-extensions). `DockHostService::register_dock` mints
//! a handle for an already-registered id, since the existing dock module
//! owns panel construction (component fn pointer, GPU surface, dock
//! window). For pure-mock testing without REAPER, use `MockDockHost`
//! (see daw-reaper crate).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use daw_proto::dock_host::{DockEvent, DockHandle, DockHostService, DockKind, PanelPixels};
use tokio::sync::broadcast;
use tracing::{debug, info};
use vox::Tx;

use crate::dock;

/// Capacity of the in-process broadcast channel that fans dock events out
/// to vox stream subscribers. Slow subscribers see Lagged errors past this.
const DOCK_EVENT_BUFFER: usize = 64;

static DOCK_BROADCASTER: OnceLock<broadcast::Sender<DockEvent>> = OnceLock::new();
static DOCK_STATE: OnceLock<Mutex<HostState>> = OnceLock::new();

fn dock_broadcaster() -> &'static broadcast::Sender<DockEvent> {
    DOCK_BROADCASTER.get_or_init(|| {
        let (tx, _rx) = broadcast::channel::<DockEvent>(DOCK_EVENT_BUFFER);
        tx
    })
}

fn host_state() -> &'static Mutex<HostState> {
    DOCK_STATE.get_or_init(|| Mutex::new(HostState::default()))
}

/// In-process subscriber for dock events. Useful for `LocalCaller` flows
/// that want to bypass the vox streaming round-trip.
pub fn subscribe_dock_broadcasts() -> broadcast::Receiver<DockEvent> {
    dock_broadcaster().subscribe()
}

/// REAPER-backed [`DockHostService`] adapter.
///
/// Stateless wrapper — all dock state lives in a process-wide
/// `OnceLock<Mutex<HostState>>` (matches the static-state pattern used by
/// `ReaperActionRegistry` and the rest of the daw-reaper crate). Cloning
/// is cheap and preserves the shared state.
#[derive(Default, Clone, Copy)]
pub struct ReaperDockHost;

#[derive(Default)]
struct HostState {
    /// `DockHandle.0 -> stable string id` (interned as `&'static str`
    /// because the dock module pins `PanelId = &'static str`).
    handles: HashMap<u64, &'static str>,
    /// Reverse lookup so re-registering the same id returns the same handle.
    by_id: HashMap<&'static str, DockHandle>,
    next_id: u64,
}

impl ReaperDockHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a handle back to the dock module's `&'static str` id.
    fn lookup(&self, handle: DockHandle) -> Option<&'static str> {
        host_state().lock().unwrap().handles.get(&handle.0).copied()
    }

    fn emit(event: DockEvent) {
        let _ = dock_broadcaster().send(event);
    }
}

impl DockHostService for ReaperDockHost {
    async fn register_dock(&self, id: String, _title: String, _kind: DockKind) -> DockHandle {
        let mut st = host_state().lock().unwrap();
        // Intern id as &'static str — the dock module requires it.
        let static_id: &'static str = Box::leak(id.into_boxed_str());
        if let Some(&existing) = st.by_id.get(static_id) {
            return existing;
        }
        st.next_id += 1;
        let handle = DockHandle(st.next_id);
        st.handles.insert(handle.0, static_id);
        st.by_id.insert(static_id, handle);
        handle
    }

    async fn unregister_dock(&self, handle: DockHandle) -> bool {
        let mut st = host_state().lock().unwrap();
        if let Some(id) = st.handles.remove(&handle.0) {
            st.by_id.remove(id);
            true
        } else {
            false
        }
    }

    async fn show(&self, handle: DockHandle) {
        if let Some(id) = self.lookup(handle) {
            dock::show_panel(id);
            Self::emit(DockEvent::Shown(handle));
        }
    }

    async fn hide(&self, handle: DockHandle) {
        if let Some(id) = self.lookup(handle) {
            dock::hide_panel(id);
            Self::emit(DockEvent::Hidden(handle));
        }
    }

    async fn toggle(&self, handle: DockHandle) -> bool {
        let Some(id) = self.lookup(handle) else {
            return false;
        };
        dock::toggle_panel(id);
        let visible = dock::is_panel_visible(id);
        Self::emit(if visible {
            DockEvent::Shown(handle)
        } else {
            DockEvent::Hidden(handle)
        });
        visible
    }

    async fn is_visible(&self, handle: DockHandle) -> bool {
        self.lookup(handle).is_some_and(dock::is_panel_visible)
    }

    async fn save_layout(&self) -> Vec<u8> {
        // Layout currently lives in REAPER's ExtState — `dock::save_dock_state`
        // is the actual side effect. Returning an empty marker blob keeps
        // the trait shape uniform across adapters; consumers that need a
        // portable blob should wrap this adapter.
        dock::save_dock_state();
        Vec::new()
    }

    async fn restore_layout(&self, _blob: Vec<u8>) -> bool {
        dock::restore_dock_state();
        Self::emit(DockEvent::LayoutChanged);
        true
    }

    async fn capture_panel_pixels(&self, handle: DockHandle) -> Option<PanelPixels> {
        let id = self.lookup(handle)?;
        let (width, height, bgra) = dock::capture_panel_pixels(id)?;
        Some(PanelPixels {
            width,
            height,
            bgra,
        })
    }

    async fn subscribe_dock_events(&self, tx: Tx<DockEvent>) {
        let mut rx = dock_broadcaster().subscribe();
        info!(
            "Dock event subscriber added (receivers: {})",
            dock_broadcaster().receiver_count()
        );
        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            debug!("Dock event subscriber disconnected");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!("Dock event subscriber lagged by {count} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Dock broadcast channel closed");
                        break;
                    }
                }
            }
        });
    }
}
