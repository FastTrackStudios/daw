//! In-memory [`DockHostService`] adapter for unit tests.
//!
//! Records every call into a `Vec<DockOp>` so tests can assert exact
//! sequences ("show was called once for this handle", "register_dock for
//! id X happened exactly once", etc.) without spinning up REAPER, GPU
//! state, or any real window.
//!
//! Pulled in via `cargo features = ["test-utils"]` so production builds
//! never link this code.

use crate::dock_host::{DockEvent, DockHandle, DockHostService, DockKind};
use std::collections::HashMap;
use std::sync::Mutex;
use vox::Tx;

/// Record of a single operation invoked against [`MockDockHost`].
///
/// Tests inspect the recorded sequence with [`MockDockHost::ops`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockOp {
    Register {
        handle: DockHandle,
        id: String,
        title: String,
        kind: DockKind,
    },
    Unregister(DockHandle),
    Show(DockHandle),
    Hide(DockHandle),
    Toggle(DockHandle),
    SaveLayout,
    RestoreLayout,
    Subscribe,
}

/// Snapshot of one mock dock's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockDock {
    pub id: String,
    pub title: String,
    pub kind: DockKind,
    pub visible: bool,
}

/// Pure in-memory [`DockHostService`].
///
/// Stores docks in a `HashMap<DockHandle, MockDock>` and tracks every
/// operation in a `Vec<DockOp>` for assertion. Subscribers receive
/// [`DockEvent`]s synchronously through their `Tx<DockEvent>` (the
/// channel's own send loop runs the event home).
#[derive(Default)]
pub struct MockDockHost {
    inner: Mutex<MockState>,
}

#[derive(Default)]
struct MockState {
    docks: HashMap<DockHandle, MockDock>,
    by_id: HashMap<String, DockHandle>,
    next_id: u64,
    ops: Vec<DockOp>,
    subscribers: Vec<Tx<DockEvent>>,
    /// Layout blob set by `restore_layout`, returned by next `save_layout`.
    /// Not a faithful round-trip — just a stable shape so tests can assert
    /// "the blob the host gave back is the one we set."
    layout_blob: Vec<u8>,
}

impl MockDockHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of the recorded operation log. Cheap to call repeatedly.
    pub fn ops(&self) -> Vec<DockOp> {
        self.inner.lock().unwrap().ops.clone()
    }

    /// Reset the operation log without touching dock state. Useful in
    /// "given/when/then" tests where setup operations clutter the log.
    pub fn clear_ops(&self) {
        self.inner.lock().unwrap().ops.clear();
    }

    /// Snapshot a dock by handle, or `None` if not registered.
    pub fn dock(&self, handle: DockHandle) -> Option<MockDock> {
        self.inner.lock().unwrap().docks.get(&handle).cloned()
    }

    /// Look up a dock by string id (the form callers passed to
    /// `register_dock`). Returns the assigned handle.
    pub fn handle_for(&self, id: &str) -> Option<DockHandle> {
        self.inner.lock().unwrap().by_id.get(id).copied()
    }

    /// Number of currently-visible docks.
    pub fn visible_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .docks
            .values()
            .filter(|d| d.visible)
            .count()
    }

    fn record(state: &mut MockState, op: DockOp) {
        state.ops.push(op);
    }

    /// Mock does not actually deliver events — daw-proto stays
    /// `tokio`-free for WASM compatibility, so we don't bring in a
    /// broadcast channel here. Tests that need delivery should assert on
    /// [`MockDockHost::ops`] / [`MockDockHost::dock`] state, or wrap a
    /// real adapter (e.g. `ReaperDockHost`) instead.
    fn emit(_state: &MockState, _event: DockEvent) {}
}

impl DockHostService for MockDockHost {
    async fn register_dock(&self, id: String, title: String, kind: DockKind) -> DockHandle {
        let mut st = self.inner.lock().unwrap();
        if let Some(&existing) = st.by_id.get(&id) {
            Self::record(
                &mut st,
                DockOp::Register {
                    handle: existing,
                    id,
                    title,
                    kind,
                },
            );
            return existing;
        }
        st.next_id += 1;
        let handle = DockHandle(st.next_id);
        st.docks.insert(
            handle,
            MockDock {
                id: id.clone(),
                title: title.clone(),
                kind,
                visible: false,
            },
        );
        st.by_id.insert(id.clone(), handle);
        Self::record(
            &mut st,
            DockOp::Register {
                handle,
                id,
                title,
                kind,
            },
        );
        handle
    }

    async fn unregister_dock(&self, handle: DockHandle) -> bool {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::Unregister(handle));
        if let Some(dock) = st.docks.remove(&handle) {
            st.by_id.remove(&dock.id);
            true
        } else {
            false
        }
    }

    async fn show(&self, handle: DockHandle) {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::Show(handle));
        if let Some(dock) = st.docks.get_mut(&handle) {
            dock.visible = true;
            Self::emit(&st, DockEvent::Shown(handle));
        }
    }

    async fn hide(&self, handle: DockHandle) {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::Hide(handle));
        if let Some(dock) = st.docks.get_mut(&handle) {
            dock.visible = false;
            Self::emit(&st, DockEvent::Hidden(handle));
        }
    }

    async fn toggle(&self, handle: DockHandle) -> bool {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::Toggle(handle));
        let visible = if let Some(dock) = st.docks.get_mut(&handle) {
            dock.visible = !dock.visible;
            dock.visible
        } else {
            return false;
        };
        Self::emit(
            &st,
            if visible {
                DockEvent::Shown(handle)
            } else {
                DockEvent::Hidden(handle)
            },
        );
        visible
    }

    async fn is_visible(&self, handle: DockHandle) -> bool {
        self.inner
            .lock()
            .unwrap()
            .docks
            .get(&handle)
            .is_some_and(|d| d.visible)
    }

    async fn save_layout(&self) -> Vec<u8> {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::SaveLayout);
        st.layout_blob.clone()
    }

    async fn restore_layout(&self, blob: Vec<u8>) -> bool {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::RestoreLayout);
        st.layout_blob = blob;
        Self::emit(&st, DockEvent::LayoutChanged);
        true
    }

    async fn subscribe_dock_events(&self, tx: Tx<DockEvent>) {
        let mut st = self.inner.lock().unwrap();
        Self::record(&mut st, DockOp::Subscribe);
        st.subscribers.push(tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock_host::DockKind;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        // Tiny single-thread runtime so we don't need to pull tokio in.
        // Future is polled until ready; the trait methods are all
        // synchronous-equivalent (no real awaits past the lock).
        let mut f = Box::pin(f);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(&waker);
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    #[test]
    fn register_returns_stable_handle_for_repeat_id() {
        let host = MockDockHost::new();
        let h1 = block_on(host.register_dock("a".into(), "A".into(), DockKind::Tabbed));
        let h2 = block_on(host.register_dock("a".into(), "A2".into(), DockKind::Floating));
        assert_eq!(h1, h2, "same id must yield same handle");
        // Two register calls recorded (idempotent at the data layer, not
        // at the op-log layer — log captures intent).
        assert_eq!(
            host.ops()
                .iter()
                .filter(|o| matches!(o, DockOp::Register { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn show_hide_toggle_track_visibility() {
        let host = MockDockHost::new();
        let h = block_on(host.register_dock("p".into(), "P".into(), DockKind::Tabbed));
        assert!(!block_on(host.is_visible(h)));

        block_on(host.show(h));
        assert!(block_on(host.is_visible(h)));

        block_on(host.hide(h));
        assert!(!block_on(host.is_visible(h)));

        let after = block_on(host.toggle(h));
        assert!(after);
        assert!(block_on(host.is_visible(h)));
    }

    #[test]
    fn unregister_clears_state_and_returns_false_for_unknown() {
        let host = MockDockHost::new();
        let h = block_on(host.register_dock("p".into(), "P".into(), DockKind::Tabbed));
        assert!(block_on(host.unregister_dock(h)));
        assert!(host.dock(h).is_none());
        assert!(!block_on(host.unregister_dock(h)));
    }

    #[test]
    fn save_layout_round_trips_blob_set_via_restore() {
        let host = MockDockHost::new();
        let blob = vec![1, 2, 3, 4];
        block_on(host.restore_layout(blob.clone()));
        assert_eq!(block_on(host.save_layout()), blob);
    }
}
