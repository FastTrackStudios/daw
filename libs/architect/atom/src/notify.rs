//! [`Notifications`] — an app-wide queue for failures (and other notices)
//! that outlive the screen that caused them.
//!
//! The gap it closes: an optimistic delete/update usually **navigates away
//! immediately**. If the server then rejects the call, the store rolls the
//! row back — but the page that owned the error signal has unmounted, so
//! the message had nowhere to go. With a `Notifications` context provided
//! at the app root, [`Mutation::run`](crate::Mutation::run) reports
//! rollback failures here automatically; the shell renders the queue once
//! (a toast tray / banner strip) and every page benefits.
//!
//! ```ignore
//! // app root:
//! provide_notifications();
//! // shell chrome:
//! let notices = use_notifications();
//! for n in notices.list() { /* render + n.id for dismiss */ }
//! ```

use dioxus::prelude::*;

/// Severity of a notice. Errors come from failed mutations; `Info` is for
/// app use (e.g. "copied to clipboard").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticeLevel {
    Info,
    Error,
}

/// One queued notice.
#[derive(Clone, PartialEq, Debug)]
pub struct Notice {
    /// Stable id for dismissal.
    pub id: u64,
    pub level: NoticeLevel,
    pub message: String,
}

/// A `Copy` handle to the app-wide notice queue.
pub struct Notifications {
    items: Signal<Vec<Notice>>,
    seq: Signal<u64>,
}

impl Clone for Notifications {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for Notifications {}

impl Notifications {
    /// Queue an error notice. Returns the notice id (for programmatic
    /// dismissal).
    pub fn error(&self, message: impl Into<String>) -> u64 {
        self.push(NoticeLevel::Error, message)
    }

    /// Queue an info notice.
    pub fn info(&self, message: impl Into<String>) -> u64 {
        self.push(NoticeLevel::Info, message)
    }

    fn push(&self, level: NoticeLevel, message: impl Into<String>) -> u64 {
        let mut seq = self.seq;
        let id = *seq.peek() + 1;
        seq.set(id);
        let mut items = self.items;
        items.write().push(Notice {
            id,
            level,
            message: message.into(),
        });
        id
    }

    /// Remove one notice by id.
    pub fn dismiss(&self, id: u64) {
        let mut items = self.items;
        items.write().retain(|n| n.id != id);
    }

    /// Remove everything.
    pub fn clear(&self) {
        let mut items = self.items;
        items.write().clear();
    }

    /// Snapshot of the queue (reactive read — re-renders on change).
    pub fn list(&self) -> Vec<Notice> {
        self.items.read().clone()
    }
}

/// Provide the queue at the app root. Mutations created below this point
/// report their rollback failures into it automatically.
pub fn provide_notifications() -> Notifications {
    let items = use_signal(Vec::new);
    let seq = use_signal(|| 0u64);
    use_context_provider(|| Notifications { items, seq })
}

/// Pull the queue anywhere under the provider.
pub fn use_notifications() -> Notifications {
    use_context::<Notifications>()
}

/// Like [`use_notifications`] but tolerant of a missing provider —
/// [`crate::use_mutation`] (and the derive-emitted CRDT mutations) use
/// this so writes work (minus auto-report) in apps that haven't opted in.
pub fn try_use_notifications() -> Option<Notifications> {
    use_hook(try_consume_context::<Notifications>)
}
