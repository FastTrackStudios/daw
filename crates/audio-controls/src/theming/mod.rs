//! Theming system for audio controls.
//!
//! Provides customizable styling through the `StyleSheet` trait and preset themes.

pub mod context;
pub mod presets;
pub mod style;
pub mod svg_texture;

pub use context::{use_theme, ControlConfig, ThemeContext, ThemeProvider};
pub use style::{ControlState, ControlVariant, KnobStyle, SliderStyle, StyleSheet, XYPadStyle};
pub use svg_texture::SvgTexture;
