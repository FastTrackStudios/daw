//! DAW REAPER Implementation
//!
//! This crate provides REAPER-specific implementations of the DAW Protocol.
//! It is designed to be used as an in-process library within the reaper-extension,
//! not as a standalone cell binary.
//!
//! # Main Thread Safety
//!
//! REAPER APIs can only be called from the main thread. This crate uses `TaskSupport`
//! from `reaper-high` to dispatch operations to the main thread.
//!
//! # Usage
//!
//! During extension initialization, call `set_task_support()` with a reference to
//! the global TaskSupport instance:
//!
//! ```rust,ignore
//! // In extension initialization
//! daw_reaper::set_task_support(Global::task_support());
//! ```
//!
//! Then create the dispatchers:
//!
//! ```rust,ignore
//! let transport = daw_reaper::ReaperTransport::new();
//! let project = daw_reaper::ReaperProject::new();
//! let dispatcher = RoutedDispatcher::new(
//!     TransportServiceDispatcher::new(transport),
//!     ProjectServiceDispatcher::new(project),
//! );
//! ```

pub mod safe_wrappers;

pub mod action_registry;
pub mod audio_accessor;
pub mod audio_engine;
pub mod automation;
pub mod ext_state;
pub mod fx;
pub mod health;
pub mod input;
pub mod item;
pub mod live_midi;
pub mod main_thread;
pub mod marker;
pub mod midi;
pub mod midi_analysis;
pub mod peak;
pub mod position_conversion;
pub mod project;
pub mod project_context;
pub mod ptr_validation;
pub mod region;
pub mod resource;
pub mod routing;
pub mod tempo_map;
pub mod toolbar;
pub mod track;
pub mod transport;
pub mod ui;

// Re-export the main types
pub use action_registry::ReaperActionRegistry;
pub use audio_accessor::ReaperAudioAccessor;
pub use audio_engine::ReaperAudioEngine;
pub use automation::ReaperAutomation;
pub use ext_state::ReaperExtState;
pub use fx::ReaperFx;
pub use health::ReaperHealth;
pub use input::ReaperInput;
pub use item::{ReaperItem, ReaperTake};
pub use live_midi::ReaperLiveMidi;
pub use marker::ReaperMarker;
pub use midi::ReaperMidi;
pub use midi_analysis::ReaperMidiAnalysis;
pub use peak::ReaperPeak;
pub use position_conversion::ReaperPositionConversion;
pub use project::ReaperProject;
pub use region::ReaperRegion;
pub use routing::ReaperRouting;
pub use tempo_map::ReaperTempoMap;
pub use toolbar::ReaperToolbar;
pub use track::ReaperTrack;
pub use transport::ReaperTransport;

// Re-export the main thread bridge and transport broadcaster functions
pub use main_thread::set_task_support;
pub use transport::{init_transport_broadcaster, poll_and_broadcast};

// Re-export FX broadcaster functions
pub use fx::{init_fx_broadcaster, poll_and_broadcast_fx};

// Re-export track broadcaster functions
pub use track::{init_track_broadcaster, poll_and_broadcast_tracks};

// Re-export tempo map broadcaster functions
pub use tempo_map::{init_tempo_map_broadcaster, poll_and_broadcast_tempo_map};

// Re-export item broadcaster functions
pub use item::{init_item_broadcaster, poll_and_broadcast_items};

// Re-export routing broadcaster functions
pub use routing::{init_routing_broadcaster, poll_and_broadcast_routing};

// Re-export toolbar deferred ops processor
pub use toolbar::process_deferred_ops as process_toolbar_ops;
