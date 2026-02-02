//! DAW REAPER Implementation
//!
//! This crate provides REAPER-specific implementations of the DAW Protocol.
//! It is designed to be used as an in-process library within the reaper-extension,
//! not as a standalone cell binary.
//!
//! # Main Thread Safety
//!
//! REAPER APIs can only be called from the main thread. This crate uses callbacks
//! to dispatch commands to the main thread. The extension must set up these callbacks
//! during initialization:
//!
//! ```ignore
//! use daw_reaper::{set_transport_callback, set_get_current_project_callback};
//!
//! // Set up transport callback
//! set_transport_callback(|cmd| {
//!     // Queue command for main thread execution
//!     main_thread_dispatcher.queue(cmd);
//! });
//!
//! // Set up project callback
//! set_get_current_project_callback(|| {
//!     // Queue command and return receiver for result
//!     main_thread_dispatcher.queue_get_current_project()
//! });
//! ```

pub mod project;
pub mod transport;

pub use project::{ReaperProject, set_get_current_project_callback};
pub use transport::{ReaperTransport, TransportCommand, get_play_state, set_transport_callback};
