//! Track module
//!
//! This module provides track types and the TrackService trait
//! for managing audio/MIDI tracks in a DAW mixer.

mod error;
mod event;
mod service;
mod track;

pub use error::TrackError;
pub use event::TrackEvent;
pub use service::{TrackService, TrackServiceClient, TrackServiceDispatcher};
pub use track::{Track, TrackRef};
