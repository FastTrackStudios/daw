//! DAW Protocol Definitions
//!
//! This crate defines the shared types and service interfaces for DAW cells.

#![deny(unsafe_code)]

pub mod action_registry;
pub mod actions;
pub mod audio_accessor;
pub mod audio_engine;
pub mod automation;
pub mod error;
pub mod ext_state;
pub mod fx;
pub mod health;
pub mod item;
pub mod live_midi;
pub mod marker;
pub mod markers_regions;
pub mod midi;
pub mod midi_analysis;
pub mod peak;
pub mod position_conversion;
pub mod primitives;
pub mod project;
pub mod region;
pub mod resource;
pub mod routing;
pub mod tempo_map;
pub mod track;
pub mod transport;
pub mod ui;
pub mod undo;

pub use action_registry::*;
pub use actions::*;
pub use audio_accessor::*;
pub use audio_engine::*;
pub use automation::*;
pub use error::*;
pub use ext_state::*;
pub use fx::*;
pub use health::*;
pub use item::*;
pub use live_midi::*;
pub use marker::*;
pub use markers_regions::*;
pub use midi::*;
pub use midi_analysis::*;
pub use peak::*;
pub use position_conversion::*;
pub use primitives::*;
pub use project::*;
pub use region::*;
pub use resource::*;
pub use routing::*;
pub use tempo_map::*;
pub use track::*;
pub use transport::*;
pub use ui::*;
pub use undo::*;
