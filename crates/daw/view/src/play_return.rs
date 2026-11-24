//! Where Stop leaves the cursor: where it is, or back where play started.
//!
//! "Return to where play started" is REAPER's "stop moves the edit cursor
//! back to the last start position" — start playing at bar 5, stop, and
//! the cursor is at bar 5 again, so a passage can be played over and over
//! without going back to place the cursor each time. A locate made while
//! playing (a click on the ruler, a jump to a section) is where play
//! starts from now, so it is what Stop returns to.
//!
//! The choice is the window's, process-wide like [`crate::options`], and
//! kept on this device the way [`crate::song_modes`] keeps its modes.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

pub use daw_timeline::cursor::{Locate, StopMode as Stop, TransportStep as Step};

/// The window's last locate.
static LOCATE: Mutex<Locate> = Mutex::new(Locate::new());

/// Run `f` on the window's [`Locate`].
pub fn with_locate<R>(f: impl FnOnce(&mut Locate) -> R) -> Option<R> {
    LOCATE.lock().ok().map(|mut locate| f(&mut locate))
}

/// Return is the default: stopping goes back to where play started.
static CURRENT: AtomicU8 = AtomicU8::new(1);
static LOADED: std::sync::Once = std::sync::Once::new();

const fn code(stop: Stop) -> u8 {
    match stop {
        Stop::Stay => 0,
        Stop::Return => 1,
    }
}

/// What Stop does now.
#[must_use]
pub fn get() -> Stop {
    LOADED.call_once(|| {
        if let Some(saved) = load().as_deref().and_then(Stop::from_slug) {
            CURRENT.store(code(saved), Ordering::Relaxed);
        }
    });
    if CURRENT.load(Ordering::Relaxed) == code(Stop::Stay) {
        Stop::Stay
    } else {
        Stop::Return
    }
}

/// Choose what Stop does, and keep the choice.
pub fn set(stop: Stop) {
    // Loaded first, so a late first `get` does not overwrite this.
    LOADED.call_once(|| {});
    CURRENT.store(code(stop), Ordering::Relaxed);
    store(stop.slug());
}

#[cfg(feature = "native")]
fn path() -> Option<std::path::PathBuf> {
    Some(dirs::data_dir()?.join("Session").join("stop-behaviour.txt"))
}

#[cfg(feature = "native")]
fn load() -> Option<String> {
    std::fs::read_to_string(path()?).ok()
}

#[cfg(feature = "native")]
fn store(text: &str) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, text) {
        tracing::warn!(error = %e, "stop behaviour: could not be kept");
    }
}

#[cfg(all(feature = "web", not(feature = "native")))]
const KEY: &str = "fts-session-stop-behaviour";

#[cfg(all(feature = "web", not(feature = "native")))]
fn load() -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(KEY)
        .ok()?
}

#[cfg(all(feature = "web", not(feature = "native")))]
fn store(text: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(KEY, text);
    }
}

#[cfg(not(any(feature = "native", feature = "web")))]
fn load() -> Option<String> {
    None
}

#[cfg(not(any(feature = "native", feature = "web")))]
fn store(_text: &str) {}
