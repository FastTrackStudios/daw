//! Region module
//!
//! This module provides region types and the RegionService trait
//! for managing named time spans in a DAW timeline.

mod error;
mod event;
#[allow(clippy::module_inception)]
mod region;
mod service;

pub use error::RegionError;
pub use event::RegionEvent;
pub use region::Region;
pub use service::{
    AddRegionInLaneRequest, RegionService, RegionServiceClient, RegionServiceDispatcher,
    region_service_service_descriptor,
};
