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

use daw_proto::dock_host::{
    DockEvent, DockHandle, DockHostService, DockKind, PanelPixels, UiEventDto,
};
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

/// Build a panel-local pointer event at `(x, y)` from the wire DTO.
fn pointer_event_at(x: f32, y: f32, button: u8) -> blitz_traits::events::BlitzPointerEvent {
    let blitz_button = match button {
        1 => blitz_traits::events::MouseEventButton::Auxiliary,
        2 => blitz_traits::events::MouseEventButton::Secondary,
        _ => blitz_traits::events::MouseEventButton::Main,
    };
    let buttons = match blitz_button {
        blitz_traits::events::MouseEventButton::Main => {
            blitz_traits::events::MouseEventButtons::Primary
        }
        blitz_traits::events::MouseEventButton::Secondary => {
            blitz_traits::events::MouseEventButtons::Secondary
        }
        _ => blitz_traits::events::MouseEventButtons::empty(),
    };
    blitz_traits::events::BlitzPointerEvent {
        id: blitz_traits::events::BlitzPointerId::Mouse,
        is_primary: true,
        coords: blitz_traits::events::PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: blitz_button,
        buttons,
        mods: keyboard_types::Modifiers::empty(),
        details: blitz_traits::events::PointerDetails::default(),
    }
}

fn build_key_event(key: String) -> blitz_traits::events::BlitzKeyEvent {
    let parsed = key
        .parse::<keyboard_types::Key>()
        .ok()
        .unwrap_or(keyboard_types::Key::Unidentified);
    blitz_traits::events::BlitzKeyEvent {
        key: parsed,
        code: keyboard_types::Code::Unidentified,
        modifiers: keyboard_types::Modifiers::empty(),
        location: keyboard_types::Location::Standard,
        is_composing: false,
        state: blitz_traits::events::KeyState::Pressed,
        is_auto_repeating: false,
        text: None,
    }
}

fn dto_to_blitz_event(dto: UiEventDto) -> Option<blitz_traits::events::UiEvent> {
    use blitz_traits::events::{BlitzWheelDelta, BlitzWheelEvent, UiEvent};
    Some(match dto {
        UiEventDto::PointerMove { x, y } => UiEvent::PointerMove(pointer_event_at(x, y, 0)),
        UiEventDto::PointerDown { x, y, button } => {
            UiEvent::PointerDown(pointer_event_at(x, y, button))
        }
        UiEventDto::PointerUp { x, y, button } => {
            UiEvent::PointerUp(pointer_event_at(x, y, button))
        }
        UiEventDto::Wheel {
            x,
            y,
            delta_x,
            delta_y,
        } => UiEvent::Wheel(BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(delta_x, delta_y),
            coords: blitz_traits::events::PointerCoords {
                page_x: x,
                page_y: y,
                screen_x: x,
                screen_y: y,
                client_x: x,
                client_y: y,
            },
            buttons: blitz_traits::events::MouseEventButtons::empty(),
            mods: keyboard_types::Modifiers::empty(),
        }),
        UiEventDto::KeyDown { key } => UiEvent::KeyDown(build_key_event(key)),
        UiEventDto::KeyUp { key } => UiEvent::KeyUp(build_key_event(key)),
    })
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

    async fn inject_ui_event(&self, handle: DockHandle, event: UiEventDto) -> bool {
        let Some(id) = self.lookup(handle) else {
            return false;
        };
        let Some(blitz_event) = dto_to_blitz_event(event) else {
            return false;
        };
        dock::dispatch_event_to_panel(id, blitz_event)
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
