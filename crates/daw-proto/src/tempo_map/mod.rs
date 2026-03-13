//! Tempo map module
//!
//! Provides types and services for managing tempo and time signature changes
//! throughout a project's timeline.

mod error;
mod event;
mod service;
mod tempo_point;

pub use error::*;
pub use event::*;
pub use service::{
    TempoMapService, TempoMapServiceClient, TempoMapServiceDispatcher,
    tempo_map_service_service_descriptor,
};
pub use tempo_point::*;
