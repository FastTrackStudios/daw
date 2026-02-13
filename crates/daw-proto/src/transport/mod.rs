//! Transport module
//!
//! This module provides transport state management and the TransportService trait
//! for controlling DAW playback, recording, and navigation.

pub mod error;
pub mod actions;
pub mod transport;

pub use error::TransportError;
pub use actions::fts_transport_actions;
pub use transport::{
    AllProjectsTransport, LoopRegion, PlayState, ProjectTransportState, RecordMode, Transport,
    TransportService,
};
pub use transport::{TransportServiceClient, TransportServiceDispatcher};
