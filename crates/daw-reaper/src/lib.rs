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

pub mod audio_engine;
pub mod automation;
pub mod item;
pub mod live_midi;
pub mod marker;
pub mod midi;
pub mod midi_analysis;
pub mod project;
pub mod project_context;
pub mod region;
pub mod routing;
pub mod tempo_map;
pub mod transport;

// Re-export the main types
pub use audio_engine::ReaperAudioEngine;
pub use automation::ReaperAutomation;
pub use item::{ReaperItem, ReaperTake};
pub use live_midi::ReaperLiveMidi;
pub use marker::ReaperMarker;
pub use midi::ReaperMidi;
pub use midi_analysis::ReaperMidiAnalysis;
pub use project::ReaperProject;
pub use region::ReaperRegion;
pub use routing::ReaperRouting;
pub use tempo_map::ReaperTempoMap;
pub use transport::ReaperTransport;

// Re-export the TaskSupport setter and transport broadcaster functions
pub use transport::{init_transport_broadcaster, poll_and_broadcast, set_task_support};
