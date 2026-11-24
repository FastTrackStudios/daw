//! Points of interest as a window draws them: the list, the one stepped
//! to, and the actions a window's keys press. Reading and writing them on
//! the song is the engine's (`session_engine::poi`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(feature = "native")]
use architect::action::ActionMeta;
use facet::Facet;

/// One place to check.
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct Poi {
    pub id: String,
    /// Project seconds.
    pub at: f64,
    /// What is there to check.
    pub note: String,
    /// Checked and right.
    pub done: bool,
}

/// The list as the window draws it, and which one was stepped to.
static SHOWN: Mutex<Option<Arc<Vec<Poi>>>> = Mutex::new(None);
static CURRENT: Mutex<Option<String>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);
static ASKED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "native")]
pub fn publish(points: Vec<Poi>) {
    if let Ok(mut shown) = SHOWN.lock() {
        *shown = Some(Arc::new(points));
    }
    VERSION.fetch_add(1, Ordering::Relaxed);
    crate::wake::wake();
    // Windows draw them from the marks the engine sends.
    crate::changes::announce(crate::changes::Change::Marks);
}

/// The points and the one stepped to, as the engine sent them with the
/// song's marks: what a window draws.
pub fn adopt(points: Vec<Poi>, stepped: Option<String>) {
    ASKED.store(true, Ordering::Release);
    if let Ok(mut shown) = SHOWN.lock() {
        *shown = Some(Arc::new(points));
    }
    if let Ok(mut c) = CURRENT.lock() {
        *c = stepped;
    }
    VERSION.fetch_add(1, Ordering::Relaxed);
    crate::wake::wake();
}

/// Goes up on every change: a window compares it with the one it drew.
#[must_use]
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// The list, once read; read off the window's thread the first time.
pub fn shown() -> Option<Arc<Vec<Poi>>> {
    let shown = SHOWN.lock().ok().and_then(|s| s.clone());
    if shown.is_none() && !ASKED.swap(true, Ordering::AcqRel) {
        refresh();
    }
    shown
}

pub fn current() -> Option<String> {
    CURRENT.lock().ok().and_then(|c| c.clone())
}

#[cfg(feature = "native")]
pub fn set_current(id: Option<String>) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = id;
    }
    VERSION.fetch_add(1, Ordering::Relaxed);
    crate::wake::wake();
    crate::changes::announce(crate::changes::Change::Marks);
}

/// Step to the next or the one before (`}` / `{`).
#[cfg(feature = "native")]
pub static STEP: ActionMeta = crate::actions::meta(
    "POI_STEP",
    "poi_step",
    "Step to a point of interest",
    "Go to the next point of interest after the edit cursor (`by` 1, `}`) or the one before (`by` -1, `{`) — `open` true skips the done ones — its page of bars framed (SESSION_STEP) and the cursor on it. Says which.",
    "Session",
    "",
);
/// Tick the one stepped to off, or open it again (`|`).
#[cfg(feature = "native")]
pub static TOGGLE: ActionMeta = crate::actions::meta(
    "POI_TOGGLE",
    "poi_toggle",
    "Tick a point of interest off",
    "The point of interest stepped to last (or `id`) done, or open again if it was (`|`). One undo step. Says which, and what it is now.",
    "Session",
    "",
);

/// Read the list again, off the window's thread (a song opened, an undo):
/// the engine's, when it runs in this process; a linked window gets the
/// list with the song's marks.
pub fn refresh() {
    crate::body::body().poi_refresh();
}
