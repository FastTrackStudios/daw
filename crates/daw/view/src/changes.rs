//! What the engine changed, told to whatever shows it.
//!
//! The engine never calls into a window: an edit that moved what a window
//! draws from (a track's marks) [`announce`]s it, and a window that draws
//! it has [`listen`]ed for it — registered when the window attaches
//! (`crate::window::attach`). With no window attached nobody listens and
//! nothing is read back. The read-back of the song itself has its own
//! flags (`crate::song::request_resync`), which a window polls.

use std::sync::RwLock;

/// One kind of change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Tracks' own markers and regions (`crate::track_regions`) changed.
    Marks,
}

type Listener = fn(Change);

static LISTENERS: RwLock<Vec<Listener>> = RwLock::new(Vec::new());

/// Call `listener` with every change from now on. Listening twice with the
/// same function listens once.
pub fn listen(listener: Listener) {
    let mut listeners = LISTENERS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !listeners.iter().any(|l| std::ptr::fn_addr_eq(*l, listener)) {
        listeners.push(listener);
    }
}

/// Stop calling `listener`.
pub fn unlisten(listener: Listener) {
    LISTENERS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|l| !std::ptr::fn_addr_eq(*l, listener));
}

/// `change` happened: every listener is told, on this thread.
pub fn announce(change: Change) {
    let listeners = LISTENERS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    for listener in listeners {
        listener(change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static HEARD: AtomicU32 = AtomicU32::new(0);

    fn count(_: Change) {
        HEARD.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn a_listener_hears_each_change_once_and_stops_when_it_goes() {
        listen(count);
        listen(count);
        let before = HEARD.load(Ordering::Relaxed);
        announce(Change::Marks);
        assert_eq!(HEARD.load(Ordering::Relaxed) - before, 1);
        unlisten(count);
        announce(Change::Marks);
        assert_eq!(HEARD.load(Ordering::Relaxed) - before, 1);
    }
}
