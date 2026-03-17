//! Standalone region implementation
//!
//! In-memory region storage with mock data for testing.
//! Each project has its own set of regions.

use crate::project::project_guids;
use daw_proto::{
    ProjectContext, TimeRange,
    region::{AddRegionInLaneRequest, Region, RegionEvent, RegionService},
};
use roam::Tx;
use crate::platform::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Section type colors (matching session-proto SectionType colors)
mod colors {
    pub const INTRO: u32 = 0x9333EA; // Purple
    pub const VERSE: u32 = 0x3B82F6; // Blue
    pub const CHORUS: u32 = 0xEF4444; // Red
    pub const BRIDGE: u32 = 0xF97316; // Orange
    pub const OUTRO: u32 = 0x6366F1; // Indigo
    pub const SOLO: u32 = 0xEC4899; // Pink
}

/// Internal region state per project
#[derive(Clone)]
struct RegionState {
    /// Regions by project GUID
    regions_by_project: HashMap<String, Vec<Region>>,
    next_id: u32,
    /// Current playhead position (for navigation)
    position: f64,
}

impl Default for RegionState {
    fn default() -> Self {
        Self {
            regions_by_project: create_mock_regions_by_project(),
            next_id: 100, // Start after mock region IDs
            position: 0.0,
        }
    }
}

/// Create mock regions for each project
///
/// Sections end at SONGEND. The END section (from SONGEND to =END) is
/// automatically added by the SongBuilder.
fn create_mock_regions_by_project() -> HashMap<String, Vec<Region>> {
    let mut regions = HashMap::new();

    // Song 1: "How Great is Our God" (SONGSTART=0s, SONGEND=240s)
    regions.insert(
        project_guids::SONG1.to_string(),
        vec![
            Region::new_full(
                Some(1),
                TimeRange::from_seconds(0.0, 30.0),
                "Intro".to_string(),
                Some(colors::INTRO),
                Some("region-s1-intro".to_string()),
            ),
            Region::new_full(
                Some(2),
                TimeRange::from_seconds(30.0, 60.0),
                "Verse 1".to_string(),
                Some(colors::VERSE),
                Some("region-s1-verse1".to_string()),
            ),
            Region::new_full(
                Some(3),
                TimeRange::from_seconds(60.0, 105.0),
                "Chorus".to_string(),
                Some(colors::CHORUS),
                Some("region-s1-chorus1".to_string()),
            ),
            Region::new_full(
                Some(4),
                TimeRange::from_seconds(105.0, 135.0),
                "Verse 2".to_string(),
                Some(colors::VERSE),
                Some("region-s1-verse2".to_string()),
            ),
            Region::new_full(
                Some(5),
                TimeRange::from_seconds(135.0, 165.0),
                "Bridge".to_string(),
                Some(colors::BRIDGE),
                Some("region-s1-bridge".to_string()),
            ),
            Region::new_full(
                Some(6),
                TimeRange::from_seconds(165.0, 210.0),
                "Chorus".to_string(),
                Some(colors::CHORUS),
                Some("region-s1-chorus2".to_string()),
            ),
            Region::new_full(
                Some(7),
                TimeRange::from_seconds(210.0, 240.0), // Ends at SONGEND
                "Outro".to_string(),
                Some(colors::OUTRO),
                Some("region-s1-outro".to_string()),
            ),
        ],
    );

    // Song 2: "Holy, Holy, Holy" (SONGSTART=0s, SONGEND=180s)
    regions.insert(
        project_guids::SONG2.to_string(),
        vec![
            Region::new_full(
                Some(11),
                TimeRange::from_seconds(0.0, 45.0),
                "Verse 1".to_string(),
                Some(colors::VERSE),
                Some("region-s2-verse1".to_string()),
            ),
            Region::new_full(
                Some(12),
                TimeRange::from_seconds(45.0, 90.0),
                "Chorus".to_string(),
                Some(colors::CHORUS),
                Some("region-s2-chorus1".to_string()),
            ),
            Region::new_full(
                Some(13),
                TimeRange::from_seconds(90.0, 135.0),
                "Verse 2".to_string(),
                Some(colors::VERSE),
                Some("region-s2-verse2".to_string()),
            ),
            Region::new_full(
                Some(14),
                TimeRange::from_seconds(135.0, 180.0), // Ends at SONGEND
                "Chorus".to_string(),
                Some(colors::CHORUS),
                Some("region-s2-chorus2".to_string()),
            ),
        ],
    );

    // Song 3: "Amazing Grace" (SONGSTART=0s, SONGEND=200s)
    regions.insert(
        project_guids::SONG3.to_string(),
        vec![
            Region::new_full(
                Some(21),
                TimeRange::from_seconds(0.0, 20.0),
                "Intro".to_string(),
                Some(colors::INTRO),
                Some("region-s3-intro".to_string()),
            ),
            Region::new_full(
                Some(22),
                TimeRange::from_seconds(20.0, 60.0),
                "Verse 1".to_string(),
                Some(colors::VERSE),
                Some("region-s3-verse1".to_string()),
            ),
            Region::new_full(
                Some(23),
                TimeRange::from_seconds(60.0, 100.0),
                "Verse 2".to_string(),
                Some(colors::VERSE),
                Some("region-s3-verse2".to_string()),
            ),
            Region::new_full(
                Some(24),
                TimeRange::from_seconds(100.0, 140.0),
                "Verse 3".to_string(),
                Some(colors::VERSE),
                Some("region-s3-verse3".to_string()),
            ),
            Region::new_full(
                Some(25),
                TimeRange::from_seconds(140.0, 170.0),
                "Solo".to_string(),
                Some(colors::SOLO),
                Some("region-s3-solo".to_string()),
            ),
            Region::new_full(
                Some(26),
                TimeRange::from_seconds(170.0, 200.0), // Ends at SONGEND
                "Outro".to_string(),
                Some(colors::OUTRO),
                Some("region-s3-outro".to_string()),
            ),
        ],
    );

    regions
}

/// Standalone region implementation with mock data
#[derive(Clone)]
pub struct StandaloneRegion {
    state: Arc<RwLock<RegionState>>,
}

impl Default for StandaloneRegion {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneRegion {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new("standalone-region-state", RegionState::default())),
        }
    }

    /// Set the current playhead position (used for navigation)
    pub async fn set_position(&self, seconds: f64) {
        self.state.write().await.position = seconds;
    }
}

/// Extract project ID from ProjectContext, returning None for Current
fn project_id(ctx: &ProjectContext) -> Option<&str> {
    match ctx {
        ProjectContext::Current => None,
        ProjectContext::Project(id) => Some(id.as_str()),
    }
}

impl StandaloneRegion {
    /// Helper to get regions for a project
    fn get_project_regions<'a>(state: &'a RegionState, project: &ProjectContext) -> &'a [Region] {
        if let Some(id) = project_id(project) {
            state
                .regions_by_project
                .get(id)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
        } else {
            // For Current, return regions from the first project (default behavior)
            state
                .regions_by_project
                .values()
                .next()
                .map(|v| v.as_slice())
                .unwrap_or(&[])
        }
    }
}

impl RegionService for StandaloneRegion {
    async fn get_regions(&self, project: ProjectContext) -> Vec<Region> {
        let state = self.state.read().await;
        Self::get_project_regions(&state, &project).to_vec()
    }

    async fn get_region(&self, project: ProjectContext, id: u32) -> Option<Region> {
        let state = self.state.read().await;
        Self::get_project_regions(&state, &project)
            .iter()
            .find(|r| r.id == Some(id))
            .cloned()
    }

    async fn get_regions_in_range(
        &self,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Region> {
        let state = self.state.read().await;
        Self::get_project_regions(&state, &project)
            .iter()
            .filter(|r| r.intersects_range(start, end))
            .cloned()
            .collect()
    }

    async fn get_region_at(
        &self,
        project: ProjectContext,
        position: f64,
    ) -> Option<Region> {
        let state = self.state.read().await;
        Self::get_project_regions(&state, &project)
            .iter()
            .find(|r| r.contains_position(position))
            .cloned()
    }

    async fn region_count(&self, project: ProjectContext) -> usize {
        let state = self.state.read().await;
        Self::get_project_regions(&state, &project).len()
    }

    async fn add_region(
        &self,
        project: ProjectContext,
        start: f64,
        end: f64,
        name: String,
    ) -> u32 {
        let mut state = self.state.write().await;
        let id = state.next_id;
        state.next_id += 1;

        let region = Region::new_full(
            Some(id),
            TimeRange::from_seconds(start, end),
            name.clone(),
            None,
            None,
        );

        let proj_id = project_id(&project)
            .map(String::from)
            .unwrap_or_else(|| project_guids::SONG1.to_string());
        let regions = state
            .regions_by_project
            .entry(proj_id.clone())
            .or_insert_with(Vec::new);
        regions.push(region);
        regions.sort_by(|a, b| a.start_seconds().partial_cmp(&b.start_seconds()).unwrap());

        debug!(
            "Added region {} '{}' at {}-{} in project {}",
            id, name, start, end, proj_id
        );
        id
    }

    async fn remove_region(&self, project: ProjectContext, id: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get_mut(proj_id)
        {
            regions.retain(|r| r.id != Some(id));
            debug!("Removed region {} from project {}", id, proj_id);
        }
    }

    async fn set_region_bounds(
        &self,
        project: ProjectContext,
        id: u32,
        start: f64,
        end: f64,
    ) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get_mut(proj_id)
        {
            if let Some(region) = regions.iter_mut().find(|r| r.id == Some(id)) {
                region.time_range = TimeRange::from_seconds(start, end);
                debug!(
                    "Set region {} bounds to {}-{} in project {}",
                    id, start, end, proj_id
                );
            }
            regions.sort_by(|a, b| a.start_seconds().partial_cmp(&b.start_seconds()).unwrap());
        }
    }

    async fn rename_region(&self, project: ProjectContext, id: u32, name: String) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get_mut(proj_id)
            && let Some(region) = regions.iter_mut().find(|r| r.id == Some(id))
        {
            region.name = name.clone();
            debug!("Renamed region {} to '{}' in project {}", id, name, proj_id);
        }
    }

    async fn set_region_color(&self, project: ProjectContext, id: u32, color: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get_mut(proj_id)
            && let Some(region) = regions.iter_mut().find(|r| r.id == Some(id))
        {
            region.color = if color == 0 { None } else { Some(color) };
            debug!(
                "Set region {} color to {:06x} in project {}",
                id, color, proj_id
            );
        }
    }

    async fn add_region_in_lane(
        &self,
        _project: ProjectContext,
        _request: AddRegionInLaneRequest,
    ) -> u32 {
        0
    }

    async fn set_region_lane(&self, project: ProjectContext, id: u32, lane: Option<u32>) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get_mut(proj_id)
            && let Some(region) = regions.iter_mut().find(|r| r.id == Some(id))
        {
            region.lane = lane;
            debug!("Set region {} lane to {:?} in project {}", id, lane, proj_id);
        }
    }

    async fn get_regions_in_lane(&self, project: ProjectContext, lane: u32) -> Vec<Region> {
        let state = self.state.read().await;
        Self::get_project_regions(&state, &project)
            .iter()
            .filter(|r| r.lane == Some(lane))
            .cloned()
            .collect()
    }

    async fn goto_region_start(&self, project: ProjectContext, id: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get(proj_id)
            && let Some(region) = regions.iter().find(|r| r.id == Some(id))
        {
            state.position = region.start_seconds();
            debug!(
                "Navigated to region {} start at {} in project {}",
                id, state.position, proj_id
            );
        }
    }

    async fn goto_region_end(&self, project: ProjectContext, id: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(regions) = state.regions_by_project.get(proj_id)
            && let Some(region) = regions.iter().find(|r| r.id == Some(id))
        {
            state.position = region.end_seconds();
            debug!(
                "Navigated to region {} end at {} in project {}",
                id, state.position, proj_id
            );
        }
    }

    async fn subscribe(&self, _project: ProjectContext, _tx: Tx<RegionEvent>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
        let project = _project;
        let tx = _tx;
        info!("RegionService::subscribe() - starting region stream");

        // Clone state for the spawned task
        let state = self.state.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        moire::task::spawn(async move {
            // Send initial state: all regions for this project
            let regions = {
                let state = state.read().await;
                StandaloneRegion::get_project_regions(&state, &project).to_vec()
            };

            if tx
                .send(RegionEvent::RegionsChanged(regions.clone()))
                .await
                .is_err()
            {
                debug!("RegionService::subscribe() - client disconnected during initial send");
                return;
            }

            // Poll for changes at 2Hz (regions rarely change, no need for 60Hz)
            let mut last_regions = regions;

            loop {
                crate::platform::sleep(Duration::from_millis(500)).await;

                // Check for region changes
                let current_regions = {
                    let state = state.read().await;
                    StandaloneRegion::get_project_regions(&state, &project).to_vec()
                };

                if current_regions != last_regions {
                    // Send full region list on change
                    // (A more sophisticated implementation would send granular events)
                    if tx
                        .send(RegionEvent::RegionsChanged(current_regions.clone()))
                        .await
                        .is_err()
                    {
                        debug!("RegionService::subscribe() - client disconnected");
                        break;
                    }
                    last_regions = current_regions;
                }
            }

            info!("RegionService::subscribe() - stream ended");
        });
        } // cfg(not(wasm32))
    }
}
