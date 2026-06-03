//! Theming system for audio controls.
//!
//! Provides customizable styling through the `StyleSheet` trait and preset themes.

pub mod context;
pub mod presets;
pub mod style;
pub mod svg_texture;
/// Token-based theme model (vector-first; Phase 1 of the themeable-UI plan).
pub mod theme;

pub use context::{ControlConfig, ThemeContext, ThemeProvider, use_theme};
pub use style::{ControlState, ControlVariant, KnobStyle, SliderStyle, StyleSheet, XYPadStyle};
pub use svg_texture::SvgTexture;
pub use theme::{
    Color, ElementRole, FaderStyle, MeterStyle, Metrics, PanelStyle, StripStyle, Theme, ThemeState,
    ToggleKind, ToggleStyle, Tokens,
};
