//! The seam between the painted roll and the renderer.
//!
//! This is the only native-only module in the crate, and it is
//! deliberately thin: [`crate::paint`] decides what the roll looks like,
//! and this puts the result on screen.
//!
//! ## Why the scene is built outside `paint`
//!
//! [`Widget::paint`] is called by the renderer, not by dioxus. Reading a
//! `Signal` from there would be reaching into dioxus's world from
//! outside it — the same shape as the `get_client_rect().await` that
//! panicked with "RefCell already borrowed" on the first click under
//! Blitz (#167).
//!
//! So the component builds the scene during render, where reading
//! signals is ordinary and safe, and leaves it in [`SceneSlot`]. This
//! widget only clones what it finds. That also happens to be the right
//! performance model: the recording is rebuilt when the *state* changes,
//! and replayed by the renderer every frame regardless — so a frame
//! costs a replay, not a rebuild.
//!
//! ## Why the events are not here either
//!
//! `Widget::handle_event` exists, but its coordinates are page-relative
//! and it knows nothing about where its element sits. The `<object>`
//! element is an ordinary DOM node, so the existing dioxus handlers keep
//! working on it unchanged and keep giving `element_coordinates()` — and
//! with the scale now exactly 1, those *are* document coordinates. The
//! whole of `interaction.rs` therefore ports over untouched.

use std::cell::RefCell;
use std::rc::Rc;

use anyrender::Scene;

/// Where the component leaves the scene for the widget to find.
///
/// An `Rc<RefCell<…>>` rather than a signal because the reader is the
/// renderer, which is outside dioxus's reactive world entirely. Cloned
/// into the widget at mount and kept by the component.
#[derive(Clone, Default)]
pub struct SceneSlot(Rc<RefCell<Option<Scene>>>);

impl SceneSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Leave a freshly built scene for the next frame.
    pub fn put(&self, scene: Scene) {
        *self.0.borrow_mut() = Some(scene);
    }

    /// What the last render left, if anything.
    ///
    /// `try_borrow` rather than `borrow`: the cost of losing one frame
    /// to a contended slot is a stale frame, and the cost of panicking
    /// in a paint callback is the window.
    pub fn take_scene(&self) -> Option<Scene> {
        self.0.try_borrow().ok()?.clone()
    }
}

/// Painted frames, counted where they actually happen.
///
/// The editor's meter used to time its own re-renders, which is the rate
/// dioxus rebuilds a component at — not the rate anything reaches the
/// screen. `Widget::paint` is called by the renderer once per frame, so
/// this is the first place in the surface that can honestly say "fps".
///
/// A ring of recent intervals rather than a running average: what you
/// want to see while dragging is whether frames are *arriving evenly*,
/// and a lifetime mean hides exactly that.
#[derive(Clone, Default)]
pub struct Frames(Rc<RefCell<FrameLog>>);

#[derive(Default)]
pub struct FrameLog {
    last: Option<std::time::Instant>,
    intervals: Vec<f64>,
}

impl Frames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a frame was just painted.
    fn tick(&self) {
        let Ok(mut log) = self.0.try_borrow_mut() else {
            return;
        };
        let now = std::time::Instant::now();
        if let Some(previous) = log.last.replace(now) {
            let ms = now.duration_since(previous).as_secs_f64() * 1000.0;
            // Drop the idle gaps: the first frame after a second of
            // nothing is not a 1 fps frame, it is the start of a burst.
            if ms < 500.0 {
                log.intervals.push(ms);
                if log.intervals.len() > 60 {
                    log.intervals.remove(0);
                }
            }
        }
    }

    /// Frames per second over the recent window, once there are enough
    /// frames to divide by.
    pub fn fps(&self) -> Option<f64> {
        let log = self.0.try_borrow().ok()?;
        if log.intervals.is_empty() {
            return None;
        }
        let mean = log.intervals.iter().sum::<f64>() / log.intervals.len() as f64;
        (mean > 0.0).then(|| 1000.0 / mean)
    }
}

/// The roll's custom widget.
///
/// Holds nothing but the slot and the frame counter: everything it draws
/// was decided by the component that filled it.
pub struct RollWidget {
    slot: SceneSlot,
    frames: Frames,
}

impl RollWidget {
    pub fn new(slot: SceneSlot, frames: Frames) -> Self {
        Self { slot, frames }
    }
}

impl blitz_dom::Widget for RollWidget {
    fn paint(
        &mut self,
        _ctx: &mut dyn anyrender::RenderContext,
        _styles: &blitz_dom::node::ComputedStyles,
        _width: u32,
        _height: u32,
        _scale: f64,
    ) -> Scene {
        // `width`/`height` are the element's box, in device pixels. They
        // are deliberately unused: the scene was built in CSS pixels
        // against the same box, which the editor computes exactly
        // (`crate::sizing`), and the renderer applies the device scale
        // for us. Reading them here and scaling the drawing would
        // reintroduce the very ratio that made the svg wrong.
        self.frames.tick();
        self.slot.take_scene().unwrap_or_default()
    }
}
