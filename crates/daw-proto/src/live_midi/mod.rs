//! Live MIDI module for real-time MIDI I/O
//!
//! This module provides types and services for handling real-time MIDI input
//! and output during playback. It manages MIDI device enumeration, connection,
//! and event streaming.
//!
//! For editing MIDI data within takes, see the `midi` module.

mod device;
mod error;
mod event;
mod message;
mod service;

pub use device::*;
pub use error::*;
pub use event::*;
pub use message::*;
pub use service::*;
