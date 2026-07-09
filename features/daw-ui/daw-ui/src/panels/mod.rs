//! Top-level DAW panels — reusable, Reaper-style composition components.
//!
//! Built from the crate's widgets and driven by a shared [`TrackView`] model
//! (no host/`daw-proto` coupling, so any project can mount them):
//!
//! - [`TrackControlPanel`] — the left control sidebar (per-track rows).
//! - [`MixerControlPanel`] — the bottom-third mixer console.
//! - [`ArrangeView`] — TCP sidebar + scrollable timeline lanes.
//! - [`DawWorkspace`] — arrange-over-mixer composition of all three.

pub mod arrange_view;
pub mod envcp_row;
pub mod mcp_strip;
pub mod mixer_control_panel;
pub mod model;
pub mod track_control_panel;
pub mod transport_bar;
pub mod workspace;

pub use arrange_view::{ArrangeEdit, ArrangeView};
pub use envcp_row::EnvcpRow;
pub use mcp_strip::McpStrip;
pub use mixer_control_panel::MixerControlPanel;
pub use model::{
    ClipView, EnvelopeView, LaneDisplay, MarkerView, RegionView, TempoMarkerView, TrackView,
};
pub use track_control_panel::TrackControlPanel;
pub use transport_bar::TransportBar;
pub use workspace::DawWorkspace;
