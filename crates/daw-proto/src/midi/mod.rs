//! MIDI editing module
//!
//! This module provides types and services for editing MIDI data within takes.
//! It supports CRUD operations on notes, CC events, pitch bends, and other
//! MIDI data, plus batch operations like quantize and transpose.
//!
//! For real-time MIDI I/O during playback, see the `live_midi` module.

mod cc;
mod error;
mod event;
mod note;
mod service;

pub use cc::*;
pub use error::*;
pub use event::*;
pub use note::*;
pub use service::*;
