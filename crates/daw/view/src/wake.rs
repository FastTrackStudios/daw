//! Asking the windows for a frame when the engine changed what they draw.
//!
//! What the engine sends a window (the hit markers, the marks, a song read
//! back, the waveform's detail) lands in this crate's replicas from the
//! link's tasks, with no event behind it. A Blitz window paints only after
//! an event or a redraw request, so without a nudge the change waits for
//! the pointer to move. A window registers how to ask for its next frame
//! ([`register`]: under Blitz, the winit window's `request_redraw`), and
//! every replica that changes what is drawn calls [`wake`].
//!
//! A page needs none of it: it paints every frame anyway.

use std::sync::{Arc, Mutex};

/// How to ask one window for a frame. Called from the link's tasks, so
/// `Send + Sync`; it must be cheap and must not block (winit's
/// `request_redraw` only flags the window).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Every registered window: its key and how to wake it.
static WINDOWS: Mutex<Vec<(u64, Waker)>> = Mutex::new(Vec::new());
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A registered window: woken until this is dropped.
#[must_use = "the window is woken only while this is held"]
pub struct Registration(u64);

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut windows) = WINDOWS.lock() {
            windows.retain(|(key, _)| *key != self.0);
        }
    }
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Registration").field(&self.0).finish()
    }
}

/// Wake `waker`'s window whenever the engine changes what it draws, until
/// the [`Registration`] is dropped.
pub fn register(waker: Waker) -> Registration {
    let key = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut windows) = WINDOWS.lock() {
        windows.push((key, waker));
    }
    Registration(key)
}

/// The engine changed something a window draws: ask every registered
/// window for its next frame.
pub fn wake() {
    // Cloned out so a waker runs with the lock released (one that
    // registered or dropped another would otherwise deadlock).
    let windows: Vec<Waker> = WINDOWS
        .lock()
        .map(|windows| windows.iter().map(|(_, w)| Arc::clone(w)).collect())
        .unwrap_or_default();
    for waker in windows {
        waker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The registry is the process's, and every test here wakes it: they
    /// run one at a time, so each counts only its own wakes.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A registered window is woken by every change, and no longer once
    /// its registration is dropped.
    #[test]
    fn a_window_is_woken_until_it_goes() {
        let _serial = serial();
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let registration = register(Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        }));
        wake();
        wake();
        assert_eq!(woken.load(Ordering::Relaxed), 2);
        drop(registration);
        wake();
        assert_eq!(woken.load(Ordering::Relaxed), 2);
    }

    /// Hit markers arriving from the engine ask for a frame at once: the
    /// edit is painted in the next frame, not at the next pointer move.
    #[test]
    fn published_markers_wake_the_window() {
        let _serial = serial();
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let _registration = register(Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        }));
        crate::hit_overlay::publish(crate::hit_overlay::Overlay::default());
        assert!(woken.load(Ordering::Relaxed) >= 1);
        let before = woken.load(Ordering::Relaxed);
        crate::wave_detail::engine_changed();
        assert!(woken.load(Ordering::Relaxed) > before);
    }

    /// A waker may register another window without deadlocking.
    #[test]
    fn a_waker_may_register() {
        let _serial = serial();
        let held = Arc::new(Mutex::new(Vec::new()));
        let inner = Arc::clone(&held);
        let _registration = register(Arc::new(move || {
            if let Ok(mut held) = inner.lock()
                && held.is_empty()
            {
                held.push(register(Arc::new(|| {})));
            }
        }));
        wake();
        assert_eq!(held.lock().map(|h| h.len()).unwrap_or(0), 1);
    }
}
