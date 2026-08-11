//! # These panels are not the app's mixer, and must not be deleted
//!
//! [`McpStrip`][mcp_strip::McpStrip] executes a **WALTER program**: the
//! theme's own layout, evaluated per frame at the strip's real pixel box by
//! a 938-line interpreter with preprocessor, macro expansion, prefix
//! arithmetic and layout scoping, corpus-tested against four third-party
//! themes. It does not imitate a REAPER layout — it runs one, and it blits
//! that theme's art because rendering somebody else's theme is precisely
//! what it is for.
//!
//! The app's own mixer is [`crate::components::mixer::MixerPanel`], which
//! draws the vector controls and blits nothing. #147 replaced the bitmap
//! art path *there*, behind that seam, and deliberately left this one
//! standing, because:
//!
//! - the theme editor's live preview depends on it, rebuilding a theme from
//!   palette and layout **text** with no filesystem, on the same frame as a
//!   colour edit;
//! - it is the verifier any future layout generator would need — the only
//!   way to check emitted output against real rendering;
//! - it is how this tree reads other people's themes at all, and that
//!   corpus is the only evidence available about what real WALTER programs
//!   look like in the wild.
//!
//! An earlier version of the wayfinding map said these would be deleted.
//! #130 corrected it. Building against that note would remove a shipping
//! feature.

//! Top-level DAW panels — reusable, Reaper-style composition components.
//!
//! Built from the crate's widgets and driven by a shared [`TrackView`] model
//! (no host/`daw-proto` coupling, so any project can mount them):
//!
//! - [`TrackControlPanel`] — the left control sidebar (per-track rows).
//! - [`MixerControlPanel`] — the bottom-third mixer console.
//! - [`ArrangeView`] — TCP sidebar + scrollable timeline lanes.
//! - [`DawWorkspace`] — arrange-over-mixer composition of all three.
//!
//! Those render a *REAPER theme*: WALTER layout, PNG atlas. The [`native`]
//! module draws the same controls from the vector components instead — no
//! WALTER, no bitmaps — starting with [`NativeTransportBar`].

pub mod arrange_view;
pub mod envcp_row;
pub mod mcp_strip;
pub mod mixer_control_panel;
pub mod native;
pub mod model;
pub mod track_control_panel;
pub mod transport_bar;
pub mod workspace;

pub use arrange_view::{ArrangeEdit, ArrangeView};
pub use envcp_row::EnvcpRow;
pub use mcp_strip::McpStrip;
pub use mixer_control_panel::MixerControlPanel;
pub use native::NativeTransportBar;
pub use model::{
    ClipView, EnvelopeView, LaneDisplay, MarkerView, RegionView, TempoMarkerView, TrackView,
};
pub use track_control_panel::TrackControlPanel;
pub use transport_bar::TransportBar;
pub use workspace::DawWorkspace;
