//! Theme artwork as Dioxus components.
//!
//! REAPER's chrome ships as ~1000 PNGs. Recolouring them (see
//! `fts_themer::restyle`) moves them onto a palette but leaves the *shapes*
//! inherited — and gives the web GUI nothing, since it would have to load
//! pictures of a mixer strip rather than draw one.
//!
//! So the artwork is authored once, as Dioxus components emitting SVG, and
//! consumed twice:
//!
//! ```text
//!            #[component] fn Button(..) -> Element   (rsx! → <svg>)
//!                          │
//!            ┌─────────────┴──────────────┐
//!            ▼                            ▼
//!   web GUI: rendered live        REAPER: rasterised to PNG
//!   (vector, any DPI, animatable)  at 100 / 150 / 200 %
//! ```
//!
//! The same function draws the button you click in the browser and the
//! button REAPER blits into the mixer.
//!
//! # Scope
//!
//! 571 of REAPER's 1021 images are toolbar icons, which `fts-icons` already
//! generates from SVG at all three scales — that half needs a *spec*, not a
//! renderer. What is left is ~450 pieces of chrome, and most of it is a few
//! shapes in many states, so this is a few dozen parameterised components
//! rather than 450 drawings.
//!
//! # Why rasterise at all
//!
//! REAPER cannot load SVG. The PNGs are build output from here on, which is
//! the point: change the palette or a component and every scale regenerates.

pub mod compare;
pub mod components;
pub mod derive;
pub mod primitives;
pub mod strip;

#[cfg(feature = "render")]
pub mod render;

pub use compare::{Fidelity, compare};
pub use derive::DerivedSpec;

pub use primitives::{Button, ControlState, Groove, Meter, Panel, Thumb};
#[cfg(feature = "render")]
pub use render::{RenderError, render_for, render_sized, render_svg};
pub use strip::{Mixer, MixerStrip};
