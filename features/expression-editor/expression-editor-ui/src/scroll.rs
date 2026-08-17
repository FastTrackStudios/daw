//! Wheel gestures, resolved from the shared input configuration.
//!
//! The editor used to hardcode a `match` on modifiers, and it disagreed
//! with both the REAPER config and the arrange view about what the same
//! gesture meant — Shift did nothing here, Alt scrolled sideways instead
//! of zooming, and plain wheel zoomed instead of scrolling. Three
//! schemes for one set of gestures.
//!
//! So the mapping is not written here at all. It comes from
//! `input`'s keymap, under the [`SURFACE`] key, and REAPER binds the same
//! gesture names to its own command ids. Changing what Shift+wheel does
//! is a config edit, in one file, for every surface at once.
//!
//! The editor is its own *surface* rather than reusing the arrange
//! view's because it does not fully exist in the DAW: it has gestures the
//! arrange view has no meaning for (nudging notes off-grid). A surface
//! may add to the shared scheme; it must not redefine it, which is what
//! `input`'s own tests pin.

use input::{ActionContext, ActionId, KeymapConfig, ScrollBindingTable, ScrollEvent};

use expression_editor_core::tools::Mods;

/// The editor's key in the keymap's `scroll` table.
pub const SURFACE: &str = "editor";

/// The resolved bindings, built once.
///
/// A `OnceLock` rather than a `use_signal`, because the table is process
/// state, not component state: every canvas in every panel resolves the
/// same gestures, and rebuilding it per mount would parse the keymap on
/// every remount.
static TABLE: std::sync::OnceLock<ScrollBindingTable> = std::sync::OnceLock::new();

/// Load the shipped defaults, overlaid with the user's keymap where
/// there is one.
///
/// A broken user keymap falls back to the defaults rather than leaving
/// the editor with no scroll at all — a config typo should cost you your
/// customisation, not the ability to scroll.
fn build() -> ScrollBindingTable {
    let mut config = match input::config::load_default_config() {
        Ok(c) => c,
        // The defaults are `include_str!`d and tested, so this is
        // unreachable short of a bad edit to the crate itself.
        Err(_) => KeymapConfig::default(),
    };

    // User overrides are a filesystem read, which a browser build has no
    // business attempting.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(Some(user)) = input::config::load_user_config() {
        config = KeymapConfig::merge(config, user);
    }

    input::table_for_surface(&config, SURFACE).unwrap_or_default()
}

fn table() -> &'static ScrollBindingTable {
    TABLE.get_or_init(build)
}

/// What the shared configuration says this gesture does here.
///
/// `None` when nothing is bound, which the caller should treat as "leave
/// the view alone" rather than falling back to a guess — an unbound
/// gesture that still did something is how the schemes drifted apart.
pub fn action_for(dx: f64, dy: f64, mods: Mods) -> Option<ActionId> {
    let event = ScrollEvent {
        delta_x: dx,
        delta_y: dy,
        modifiers: input::Modifiers {
            ctrl: mods.ctrl,
            alt: mods.alt,
            shift: mods.shift,
            ..Default::default()
        },
    };
    table().match_event(&event, &ActionContext::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(dx: f64, dy: f64, mods: Mods) -> Option<String> {
        action_for(dx, dy, mods).map(|a| a.0)
    }

    fn mods(ctrl: bool, alt: bool, shift: bool) -> Mods {
        Mods { ctrl, alt, shift }
    }

    /// The bug this whole module exists for: Shift+wheel scrolled
    /// nothing, because the editor's hardcoded match had no arm for it.
    #[test]
    fn shift_wheel_scrolls_horizontally() {
        assert_eq!(
            act(0.0, 10.0, mods(false, false, true)).as_deref(),
            Some("view.hscroll")
        );
    }

    #[test]
    fn the_editor_resolves_the_shared_scheme() {
        assert_eq!(
            act(0.0, 10.0, mods(false, false, false)).as_deref(),
            Some("view.vscroll")
        );
        assert_eq!(
            act(0.0, 10.0, mods(false, true, false)).as_deref(),
            Some("view.zoom_v")
        );
        assert_eq!(
            act(0.0, 10.0, mods(false, true, true)).as_deref(),
            Some("view.zoom_h")
        );
        assert_eq!(
            act(0.0, 10.0, mods(true, true, false)).as_deref(),
            Some("view.zoom_both")
        );
    }

    #[test]
    fn the_editor_keeps_its_own_off_grid_nudge() {
        assert_eq!(
            act(0.0, 10.0, mods(true, false, true)).as_deref(),
            Some("edit.nudge_time")
        );
    }

    #[test]
    fn an_unbound_gesture_resolves_to_nothing() {
        // Ctrl alone is deliberately unbound, matching REAPER. It must
        // report that rather than falling through to a default.
        assert_eq!(act(0.0, 10.0, mods(true, false, false)), None);
    }
}
