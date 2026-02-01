//! DAW Reaper Implementation
//!
//! This crate provides a REAPER-specific implementation of the DAW Protocol.

#![deny(unsafe_code)]

pub mod transport;

pub use transport::ReaperTransport;