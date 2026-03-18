//! Regions handle and operations

use std::sync::Arc;

use crate::DawClients;
use crate::Result;
use daw_proto::{ProjectContext, Region, RegionEvent};
use roam::Rx;

/// Regions handle for a specific project
///
/// This handle provides access to region operations (query, add, remove, navigate)
/// for a specific project. Like reaper-rs, it's lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::ErasedCaller) -> daw_control::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let regions = project.regions();
///
/// // Query regions
/// let all = regions.all().await?;
/// let count = regions.count().await?;
///
/// // Find region at current position
/// if let Some(region) = regions.at_position(10.5).await? {
///     println!("Currently in: {}", region.name);
/// }
///
/// // Add and manipulate regions
/// let id = regions.add(0.0, 30.0, "Intro").await?;
/// regions.rename(id, "Extended Intro").await?;
/// regions.set_bounds(id, 0.0, 45.0).await?;
///
/// // Navigation
/// regions.goto_start(id).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Regions {
    project_id: String,
    clients: Arc<DawClients>,
}

impl Regions {
    /// Create a new regions handle for a project
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all regions in the project
    pub async fn all(&self) -> Result<Vec<Region>> {
        let regions = self.clients.region.get_regions(self.context()).await?;
        Ok(regions)
    }

    /// Get a specific region by ID
    pub async fn get(&self, id: u32) -> Result<Option<Region>> {
        let region = self.clients.region.get_region(self.context(), id).await?;
        Ok(region)
    }

    /// Get all regions that intersect with a time range
    pub async fn in_range(&self, start: f64, end: f64) -> Result<Vec<Region>> {
        let regions = self
            .clients
            .region
            .get_regions_in_range(self.context(), start, end)
            .await?;
        Ok(regions)
    }

    /// Get the region containing a specific position (if any)
    pub async fn at_position(&self, position: f64) -> Result<Option<Region>> {
        let region = self
            .clients
            .region
            .get_region_at(self.context(), position)
            .await?;
        Ok(region)
    }

    /// Get the total number of regions
    pub async fn count(&self) -> Result<usize> {
        let count = self.clients.region.region_count(self.context()).await?;
        Ok(count)
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Add a new region with the given bounds
    ///
    /// Returns the ID of the newly created region.
    pub async fn add(&self, start: f64, end: f64, name: &str) -> Result<u32> {
        let id = self
            .clients
            .region
            .add_region(self.context(), start, end, name.to_string())
            .await?;
        Ok(id)
    }

    /// Add a new region in a specific ruler lane.
    ///
    /// Returns the ID of the newly created region.
    pub async fn add_in_lane(&self, start: f64, end: f64, name: &str, lane: u32) -> Result<u32> {
        let id = self
            .clients
            .region
            .add_region_in_lane(
                self.context(),
                daw_proto::AddRegionInLaneRequest {
                    start,
                    end,
                    name: name.to_string(),
                    lane,
                },
            )
            .await?;
        Ok(id)
    }

    /// Set the ruler lane for a region. Pass None to move to the default lane.
    pub async fn set_lane(&self, id: u32, lane: Option<u32>) -> Result<()> {
        self.clients
            .region
            .set_region_lane(self.context(), id, lane)
            .await?;
        Ok(())
    }

    /// Remove a region by ID
    pub async fn remove(&self, id: u32) -> Result<()> {
        self.clients
            .region
            .remove_region(self.context(), id)
            .await?;
        Ok(())
    }

    /// Set region bounds (start and end position)
    pub async fn set_bounds(&self, id: u32, start: f64, end: f64) -> Result<()> {
        self.clients
            .region
            .set_region_bounds(self.context(), id, start, end)
            .await?;
        Ok(())
    }

    /// Rename a region
    pub async fn rename(&self, id: u32, name: &str) -> Result<()> {
        self.clients
            .region
            .rename_region(self.context(), id, name.to_string())
            .await?;
        Ok(())
    }

    /// Set the color of a region (0 for default color)
    pub async fn set_color(&self, id: u32, color: u32) -> Result<()> {
        self.clients
            .region
            .set_region_color(self.context(), id, color)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    /// Navigate to the start of a region
    pub async fn goto_start(&self, id: u32) -> Result<()> {
        self.clients
            .region
            .goto_region_start(self.context(), id)
            .await?;
        Ok(())
    }

    /// Navigate to the end of a region
    pub async fn goto_end(&self, id: u32) -> Result<()> {
        self.clients
            .region
            .goto_region_end(self.context(), id)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Subscriptions
    // =========================================================================

    /// Subscribe to region change events for this project.
    pub async fn subscribe(&self) -> Result<Rx<RegionEvent>> {
        let (tx, rx) = roam::channel::<RegionEvent>();
        self.clients.region.subscribe(self.context(), tx).await?;
        Ok(rx)
    }
}

impl std::fmt::Debug for Regions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Regions")
            .field("project_id", &self.project_id)
            .finish()
    }
}
