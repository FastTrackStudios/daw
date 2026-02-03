//! Region event types
//!
//! Events are emitted when region state changes.

use super::Region;
use facet::Facet;

/// Events emitted when regions change
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum RegionEvent {
    /// A new region was added
    Added(Region),
    /// A region was removed (contains the ID)
    Removed(u32),
    /// A region was modified
    Changed(Region),
    /// Multiple regions changed (e.g., project reload)
    RegionsChanged(Vec<Region>),
}
