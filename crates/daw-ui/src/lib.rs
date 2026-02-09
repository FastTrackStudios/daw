//! DAW UI — Dioxus components for DAW integration.
//!
//! Provides panels for interacting with a connected DAW:
//! - MixerPanel — horizontal track strips with volume/pan/mute/solo
//! - TrackControlPanel (TCP) — vertical track list with folder hierarchy
//! - ArrangementView — timeline placeholder with track lanes
//! - FxParameterBrowser — live FX parameter browser with bidirectional control
//! - FxBrowserDockPanel — dock-ready wrapper for the FX browser

pub mod components;
pub mod hooks;
pub mod layouts;
pub mod prelude;
pub mod signals;

// Re-exports for desktop app
pub use components::arrangement_view::ArrangementView;
pub use components::fx_chain_tree::FxChainTree;
pub use components::fx_parameter_browser::FxParameterBrowser;
pub use components::mixer::MixerPanel;
pub use components::track_control_panel::TrackControlPanel;
pub use layouts::daw_panels::FxBrowserDockPanel;
