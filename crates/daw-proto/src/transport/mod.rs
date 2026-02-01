//! Transport module
//!
//! This module provides transport state management.

pub mod error;
pub mod transport;

pub use error::TransportError;
pub use transport::{PlayState, RecordMode, Transport, TransportService};
pub use transport::TransportServiceClient;
