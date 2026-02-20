//! Marker module
//!
//! This module provides marker types and the MarkerService trait
//! for managing named reference points in a DAW timeline.

mod error;
mod event;
#[allow(clippy::module_inception)]
mod marker;
mod service;

pub use error::MarkerError;
pub use event::MarkerEvent;
pub use marker::Marker;
pub use service::{MarkerService, MarkerServiceClient, MarkerServiceDispatcher};
