//! Automation module for envelope control
//!
//! This module provides types and services for managing automation envelopes.
//! Envelopes contain automation data for track parameters (volume, pan) and
//! FX parameters. Each envelope has points with time, value, and curve shape.

mod envelope;
mod error;
mod event;
mod service;

pub use envelope::*;
pub use error::*;
pub use event::*;
pub use service::*;
