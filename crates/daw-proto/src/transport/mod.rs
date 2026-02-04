//! Transport module
//!
//! This module provides transport state management and the TransportService trait
//! for controlling DAW playback, recording, and navigation.

pub mod error;
pub mod transport;

pub use error::TransportError;
pub use transport::{
    AllProjectsTransport, PlayState, ProjectTransportState, RecordMode, Transport, TransportService,
};
pub use transport::{TransportServiceClient, TransportServiceDispatcher};
