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
pub mod mixer_control_panel;
pub mod model;
pub mod track_control_panel;
pub mod workspace;

pub use arrange_view::ArrangeView;
pub use mixer_control_panel::MixerControlPanel;
pub use model::{ClipView, TrackView};
pub use track_control_panel::TrackControlPanel;
pub use workspace::DawWorkspace;
