//! The arrangement's selection, as the window publishes it every frame
//! (an edit's scope). Editing what is selected is the engine's
//! (`session_engine::edit_actions`).

use daw_edit::scope::Scope;

/// The arrangement's selection as an edit's scope, as last drawn.
static CURRENT: std::sync::Mutex<Option<Scope>> = std::sync::Mutex::new(None);

/// Publish the arrangement's selection (every frame it draws).
pub fn publish(scope: Scope) {
    if let Ok(mut slot) = CURRENT.lock() {
        *slot = Some(scope);
    }
}

/// The arrangement's selection as last drawn, if it has drawn.
#[must_use]
pub fn current() -> Option<Scope> {
    CURRENT.lock().ok().and_then(|slot| slot.clone())
}
