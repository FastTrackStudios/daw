//! Professional audio parameter control widgets for Dioxus.
//!
//! Provides a comprehensive set of UI widgets for controlling audio parameters
//! in control surfaces and rig editors. All widgets support:
//!
//! - SVG/CSS-based rendering (works across native, web, and desktop)
//! - Modulation range visualization
//! - Customizable theming with preset styles (Minimal, SSL, Vintage)
//! - Fine control with modifier keys
//! - Double-click to reset

pub mod core;
pub mod theming;
pub mod widgets;

// Internal prelude for conditional dioxus import
pub(crate) mod prelude;
