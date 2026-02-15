//! Transport module
//!
//! This module provides transport state management and the TransportService trait
//! for controlling DAW playback, recording, and navigation.

pub mod actions;
pub mod error;
pub mod transport;

pub use actions::fts_transport_actions;
pub use error::TransportError;
pub use transport::{
    AllProjectsTransport, LoopRegion, PlayState, ProjectTransportState, RecordMode, Transport,
    TransportService,
};
pub use transport::{TransportServiceClient, TransportServiceDispatcher};
