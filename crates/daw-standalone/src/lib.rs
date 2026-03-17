//! Standalone DAW Implementation
//!
//! This is a minimal DAW implementation that runs standalone without any external DAW.
//! It serves as both the reference implementation and the mock for testing.
//!
//! The implementations in this module (`StandaloneTransport`, `StandaloneProject`) can be
//! used directly in tests without spawning a separate cell process.
//!
//! ## Mock Data
//!
//! The standalone implementation includes mock data for testing:
//! - **3 songs** with markers (SONGSTART/SONGEND)
//! - **Sections** as regions (Intro, Verse, Chorus, Bridge, Outro, Solo)
//! - **Tempo/time signature changes** throughout the timeline
//!
//! This allows testing the full fts-control-web experience without a real DAW.

#![deny(unsafe_code)]

mod automation;
mod ext_state;
mod fx;
mod item;
mod live_midi;
mod marker;
mod midi;
mod midi_analysis;
mod position_conversion;
mod project;
#[cfg(feature = "audio")]
pub mod audio_engine;
pub(crate) mod platform;
mod region;
mod resource;
mod routing;
mod tempo_map;
mod track;
mod transport;
mod ui;

pub use automation::StandaloneAutomation;
pub use ext_state::StandaloneExtState;
pub use fx::StandaloneFx;
pub use item::{StandaloneItem, StandaloneTake};
pub use live_midi::StandaloneLiveMidi;
pub use marker::StandaloneMarker;
pub use midi::StandaloneMidi;
pub use midi_analysis::StandaloneMidiAnalysis;
pub use position_conversion::StandalonePositionConversion;
pub use project::{StandaloneProject, project_guids};
pub use region::StandaloneRegion;
pub use resource::StandaloneResource;
pub use routing::StandaloneRouting;
pub use tempo_map::StandaloneTempoMap;
pub use track::StandaloneTrack;
pub use transport::{SharedProjectState, StandaloneTransport};
pub use ui::StandaloneUi;
