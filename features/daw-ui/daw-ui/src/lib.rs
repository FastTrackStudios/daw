//! DAW UI — Dioxus components for DAW integration.
//!
//! Provides panels for interacting with a connected DAW:
//! - MixerPanel — horizontal track strips with volume/pan/mute/solo
//! - TrackControlPanel (TCP) — vertical track list with folder hierarchy
//! - ArrangementView — timeline placeholder with track lanes
//! - FxParameterBrowser — live FX parameter browser with bidirectional control
//! - FxBrowserDockPanel — dock-ready wrapper for the FX browser

// ── DAW-wired panels (poll a live `daw_control::Daw`) ──
pub mod components;
pub mod hooks;
pub mod layouts;
pub mod panel_registration;
pub mod prelude;
pub mod signals;

// ── Reusable, vector-themeable component library (merged from the former
// `audio-controls` crate). Low-level widgets + the token-based theming model +
// the reusable top-level panels (TrackControlPanel / MixerControlPanel /
// ArrangeView / DawWorkspace). Driven by props/Signals + `Theme` — no DAW
// runtime dependency, unlike the `components::*` panels above. ──
pub mod core;
/// Reusable top-level panels (require the `web`/dioxus feature).
#[cfg(feature = "web")]
pub mod panels;
pub mod theming;
pub mod widgets;

#[cfg(feature = "reaper-test-panels")]
pub mod test_panels;

// Re-exports for desktop app
pub use components::arrangement_view::ArrangementView;
pub use components::fx_chain_tree::FxChainTree;
pub use components::fx_parameter_browser::FxParameterBrowser;
pub use components::mixer::MixerPanel;
pub use components::track_control_panel::TrackControlPanel;
pub use layouts::daw_panels::{DawApplication, FxBrowserDockPanel};
pub use panel_registration::register_panels;
