//! Routing module for track sends, receives, and hardware outputs
//!
//! This module provides types and services for managing audio routing between tracks
//! and to hardware outputs. Routes can be sends (from a track), receives (to a track),
//! or hardware outputs.

mod error;
mod event;
mod route;
mod service;

pub use error::*;
pub use event::*;
pub use route::*;
pub use service::*;
