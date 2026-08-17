//! How big the roll is, and who decides.
//!
//! Its own module because it is the question this surface has got wrong
//! most often, and it was previously five loose items in the middle of a
//! two-thousand-line file.
//!
//! **The element box is the truth.** The roll is `position: absolute;
//! inset: 0` inside its cell, so its size comes from layout — responsive,
//! no pixel constants, and independent of its own content. It carries no
//! `viewBox`, so an svg user unit is a CSS pixel and a pointer position
//! *is* a document coordinate.
//!
//! [`Editor::viewport`] then has to follow that box. Where it does not,
//! the roll draws at the wrong size and the surplus is background or is
//! clipped — visibly wrong, which is the point: every earlier attempt
//! made a mismatch *invisible* by scaling, and a scaled pointer lands
//! somewhere the user did not click.
//!
//! ## Why the host reports rather than the editor measuring
//!
//! Measuring the element is the obvious answer and has failed four ways:
//!
//! - `onresize` never fires under dioxus-native — `convert_resize_data`
//!   is `unimplemented!()`, and that is the renderer the plugin, the
//!   REAPER panel and the desktop runner all use.
//! - `get_client_rect()` from a task re-enters dioxus's document while an
//!   event is being dispatched: "RefCell already borrowed", the #167
//!   panic that `tests/mpe_gestures.rs` guards.
//! - Measuring on a timer takes `doc_mut()` and forces a relayout every
//!   tick.
//! - Deriving the size from the content is what made the editor resize
//!   itself as the roll scrolled.
//!
//! So the host states its space — a desktop window from its winit resize
//! event, the REAPER panel from its dock callback — and the editor
//! subtracts its own chrome. That is a constant, and a constant is an
//! approximation; see [`CHROME_HEIGHT`].

use dioxus::prelude::*;
use expression_editor_core::Viewport;

use crate::canvas;
/// Chrome the editor draws around the roll — toolbar, track switcher,
/// chord row, lane strip and status bar, top and bottom combined.
///
/// A constant rather than a measurement, because measuring it is what
/// kept going wrong. `onresize` never fires under dioxus-native
/// (`convert_resize_data` is `unimplemented!()`), and reading the cell
/// with `get_client_rect` from a task re-enters dioxus's document during
/// event dispatch — "RefCell already borrowed", the #167 panic, which
/// `tests/mpe_gestures.rs` still guards.
///
/// Measured off a rendered shot; re-measure with `--example shot` if the
/// toolbar grows a row.
pub const CHROME_HEIGHT: f64 = 224.0;

/// The space the host has given the editor, in CSS pixels.
///
/// The editor cannot discover this for itself in any renderer it runs
/// in, so the host states it: a desktop window from its winit resize
/// event, the REAPER panel from its dock callback. `None` until someone
/// says, which leaves the viewport the document was built with.
pub static AVAILABLE: GlobalSignal<Option<(f64, f64)>> = Signal::global(|| None);

/// Tell the editor how much room it has. Idempotent; call it as often as
/// a resize drag fires.
pub fn available_space(width: f64, height: f64) {
    let next = Some((width, height));
    if *AVAILABLE.read() != next {
        *AVAILABLE.write() = next;
    }
}

/// The viewport for whatever space the host last reported.
///
/// What a *new* document should be built with. Constructing one against
/// a fixed size — which is what the runner's constant did — resets the
/// surface to that size every time a scene or a file is opened, so the
/// editor snapped back to its opening aspect no matter how wide the
/// window had been dragged.
///
/// `fallback` covers the first load, before any host has said anything.
pub fn current_viewport(fallback: Viewport) -> Viewport {
    AVAILABLE()
        .map(|(w, h)| viewport_in(w, h))
        .unwrap_or(fallback)
}

/// The roll's viewport inside `(width, height)` of host space.
pub fn viewport_in(width: f64, height: f64) -> Viewport {
    // The gutter comes off the width for the same reason the chrome
    // comes off the height: `vp` is the *roll*, and the svg drawn for it
    // is `vp.w + GUTTER_W` wide. Leaving the gutter in made the viewBox
    // 54 units wider than the element it was drawn into, so the svg
    // scaled and every pointer position was read as a viewBox unit while
    // being an element pixel — a selection that landed further off the
    // further right you clicked.
    Viewport::new(
        (width - canvas::GUTTER_W).max(1.0),
        (height - CHROME_HEIGHT).max(1.0),
    )
}
