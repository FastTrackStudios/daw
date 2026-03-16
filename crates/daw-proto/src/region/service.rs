//! Region service trait
//!
//! Defines the RPC interface for region operations.

use super::{Region, RegionEvent};
use crate::ProjectContext;
use roam::{Tx, service};

/// Service for managing regions in a DAW project
///
/// Regions are named time spans that can be used for organizing sections,
/// defining loop areas, or marking sections for export.
#[service]
pub trait RegionService {
    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all regions in the project
    async fn get_regions(&self, project: ProjectContext) -> Vec<Region>;

    /// Get a specific region by ID
    async fn get_region(&self, project: ProjectContext, id: u32) -> Option<Region>;

    /// Get all regions that intersect with a time range
    async fn get_regions_in_range(
        &self,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Region>;

    /// Get the region containing a specific position (if any)
    async fn get_region_at(&self, project: ProjectContext, position: f64) -> Option<Region>;

    /// Get the total number of regions
    async fn region_count(&self, project: ProjectContext) -> usize;

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Add a new region (returns region ID)
    async fn add_region(&self, project: ProjectContext, start: f64, end: f64, name: String) -> u32;

    /// Remove a region by ID
    async fn remove_region(&self, project: ProjectContext, id: u32);

    /// Set region bounds (start and end position)
    async fn set_region_bounds(&self, project: ProjectContext, id: u32, start: f64, end: f64);

    /// Rename a region
    async fn rename_region(&self, project: ProjectContext, id: u32, name: String);

    /// Set the color of a region (0 for default color)
    async fn set_region_color(&self, project: ProjectContext, id: u32, color: u32);

    // =========================================================================
    // Lane Methods (v7.62+)
    // =========================================================================

    /// Set the ruler lane for a region (None to move to default lane)
    async fn set_region_lane(&self, project: ProjectContext, id: u32, lane: Option<u32>);

    /// Get all regions in a specific ruler lane
    async fn get_regions_in_lane(&self, project: ProjectContext, lane: u32) -> Vec<Region>;

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    /// Navigate to the start of a region
    async fn goto_region_start(&self, project: ProjectContext, id: u32);

    /// Navigate to the end of a region
    async fn goto_region_end(&self, project: ProjectContext, id: u32);

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to region changes for a project
    ///
    /// Streams events when:
    /// - A region is added, removed, or modified
    /// - Project regions are bulk-updated (e.g., project reload)
    ///
    /// Initially sends a `RegionsChanged` event with all current regions.
    async fn subscribe(&self, project: ProjectContext, tx: Tx<RegionEvent>);
}
