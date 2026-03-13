//! REAPER Region Implementation
//!
//! Implements RegionService by dispatching REAPER API calls to the main thread
//! using crate::main_thread.

use crate::main_thread;
use crate::project_context::resolve_project_context;
use crate::safe_wrappers::markers as sw;
use daw_proto::{ProjectContext, Region, RegionEvent, RegionService, TimeRange};
use reaper_medium::{
    MarkerOrRegionPosition, PositionInSeconds, ProjectContext as ReaperProjectContext,
};
use roam::Tx;
use std::ffi::CString;
use std::time::Duration;
use tracing::{debug, info};

// =============================================================================
// Public sync helper — callable directly from the main thread
// =============================================================================

/// Read all regions from the current project, sorted by start position.
///
/// Must be called from the main thread.
pub fn get_regions_on_main_thread() -> Vec<Region> {
    let reaper = reaper_high::Reaper::get();
    let medium = reaper.medium_reaper();
    let mut regions = Vec::new();

    let count_result = medium.count_project_markers(ReaperProjectContext::CurrentProject);
    let total_count = count_result.total_count;

    for idx in 0..total_count {
        medium.enum_project_markers_3(ReaperProjectContext::CurrentProject, idx, |result| {
            if let Some(info) = result {
                if let Some(end_pos) = info.region_end_position {
                    regions.push(Region {
                        id: Some(info.id.get()),
                        time_range: TimeRange::from_seconds(
                            info.position.get(),
                            end_pos.get(),
                        ),
                        name: info.name.to_string(),
                        color: {
                            let c = info.color.to_raw();
                            if c != 0 { Some(c as u32) } else { None }
                        },
                        guid: None,
                    });
                }
            }
        });
    }

    regions.sort_by(|a, b| {
        a.start_seconds()
            .partial_cmp(&b.start_seconds())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    regions
}

/// REAPER region implementation.
///
/// All methods dispatch to the main thread via main_thread.
#[derive(Clone)]
pub struct ReaperRegion;

impl ReaperRegion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperRegion {
    fn default() -> Self {
        Self::new()
    }
}

impl RegionService for ReaperRegion {
    // =========================================================================
    // Query Methods
    // =========================================================================

    async fn get_regions(&self, project: ProjectContext) -> Vec<Region> {
        main_thread::query(move || {
            // For non-current projects we still need to resolve the context
            let reaper_ctx = resolve_project_context(&project);
            if matches!(reaper_ctx, ReaperProjectContext::CurrentProject) {
                return get_regions_on_main_thread();
            }

            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            let mut regions = Vec::new();

            let count_result = medium.count_project_markers(reaper_ctx);
            let total_count = count_result.total_count;

            for idx in 0..total_count {
                medium.enum_project_markers_3(reaper_ctx, idx, |result| {
                    if let Some(info) = result {
                        if let Some(end_pos) = info.region_end_position {
                            regions.push(Region {
                                id: Some(info.id.get()),
                                time_range: TimeRange::from_seconds(
                                    info.position.get(),
                                    end_pos.get(),
                                ),
                                name: info.name.to_string(),
                                color: {
                                    let c = info.color.to_raw();
                                    if c != 0 { Some(c as u32) } else { None }
                                },
                                guid: None,
                            });
                        }
                    }
                });
            }

            regions.sort_by(|a, b| {
                a.start_seconds()
                    .partial_cmp(&b.start_seconds())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            regions
        })
        .await
        .unwrap_or_default()
    }

    async fn get_region(&self, project: ProjectContext, id: u32) -> Option<Region> {
        let regions = self.get_regions(project).await;
        regions.into_iter().find(|r| r.id == Some(id))
    }

    async fn get_regions_in_range(
        &self,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Region> {
        let regions = self.get_regions(project).await;
        regions
            .into_iter()
            .filter(|r| r.intersects_range(start, end))
            .collect()
    }

    async fn get_region_at(
        &self,
        project: ProjectContext,
        position: f64,
    ) -> Option<Region> {
        let regions = self.get_regions(project).await;
        regions.into_iter().find(|r| r.contains_position(position))
    }

    async fn region_count(&self, project: ProjectContext) -> usize {
        self.get_regions(project).await.len()
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    async fn add_region(
        &self,
        _project: ProjectContext,
        start: f64,
        end: f64,
        name: String,
    ) -> u32 {
        debug!(
            "ReaperRegion: add_region '{}' from {} to {}",
            name, start, end
        );
        main_thread::query(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            if let (Ok(start_pos), Ok(end_pos)) =
                (PositionInSeconds::new(start), PositionInSeconds::new(end))
            {
                medium
                    .add_project_marker_2(
                        ReaperProjectContext::CurrentProject,
                        MarkerOrRegionPosition::Region(start_pos, end_pos),
                        name.as_str(),
                        None,
                        None,
                    )
                    .unwrap_or(0)
            } else {
                0
            }
        })
        .await
        .unwrap_or(0)
    }

    async fn remove_region(&self, _project: ProjectContext, id: u32) {
        debug!("ReaperRegion: remove_region {}", id);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            sw::delete_project_marker(
                low,
                ReaperProjectContext::CurrentProject.to_raw(),
                id as i32,
                true, // is a region
            );
        });
    }

    async fn set_region_bounds(
        &self,
        _project: ProjectContext,
        id: u32,
        start: f64,
        end: f64,
    ) {
        debug!(
            "ReaperRegion: set_region_bounds {} to {} - {}",
            id, start, end
        );
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            sw::set_project_marker(
                low,
                id as i32,
                true, // is a region
                start,
                end,
                std::ptr::null(),
            );
        });
    }

    async fn rename_region(&self, _project: ProjectContext, id: u32, name: String) {
        debug!("ReaperRegion: rename_region {} to '{}'", id, name);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            if let Ok(cname) = CString::new(name) {
                sw::set_project_marker(low, id as i32, true, -1.0, -1.0, cname.as_ptr());
            }
        });
    }

    async fn set_region_color(&self, _project: ProjectContext, id: u32, color: u32) {
        debug!("ReaperRegion: set_region_color {} to {}", id, color);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            sw::set_project_marker_by_index2(
                low,
                ReaperProjectContext::CurrentProject.to_raw(),
                id as i32,
                true, // is a region
                -1.0,
                -1.0,
                -1,
                std::ptr::null(),
                color as i32,
                0,
            );
        });
    }

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    async fn goto_region_start(&self, _project: ProjectContext, id: u32) {
        debug!("ReaperRegion: goto_region_start {}", id);
        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            // Go to region by ID (uses same API as markers)
            medium.go_to_marker(
                ReaperProjectContext::CurrentProject,
                reaper_medium::BookmarkRef::Id(reaper_medium::BookmarkId::new(id as _)),
            );
        });
    }

    async fn goto_region_end(&self, project: ProjectContext, id: u32) {
        debug!("ReaperRegion: goto_region_end {}", id);
        // Get the region's end position and set cursor there
        if let Some(region) = self.get_region(project, id).await {
            let end_pos = region.end_seconds();
            main_thread::run(move || {
                let reaper = reaper_high::Reaper::get();
                if let Ok(pos) = PositionInSeconds::new(end_pos) {
                    reaper.current_project().set_edit_cursor_position(
                        pos,
                        reaper_medium::SetEditCurPosOptions {
                            move_view: false,
                            seek_play: true,
                        },
                    );
                }
            });
        }
    }

    async fn subscribe(&self, project: ProjectContext, tx: Tx<RegionEvent>) {
        info!("ReaperRegion::subscribe() - starting region stream");

        // Clone self for the spawned task
        let this = self.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        moire::task::spawn(async move {
            // Send initial state
            let regions = this.get_regions(project.clone()).await;
            if tx
                .send(RegionEvent::RegionsChanged(regions.clone()))
                .await
                .is_err()
            {
                debug!("ReaperRegion::subscribe() - client disconnected during initial send");
                return;
            }

            // Poll for changes at 60Hz
            let mut last_regions = regions;

            loop {
                tokio::time::sleep(Duration::from_micros(16667)).await;

                let current_regions = this.get_regions(project.clone()).await;
                if current_regions != last_regions {
                    if tx
                        .send(RegionEvent::RegionsChanged(current_regions.clone()))
                        .await
                        .is_err()
                    {
                        debug!("ReaperRegion::subscribe() - client disconnected");
                        break;
                    }
                    last_regions = current_regions;
                }
            }

            info!("ReaperRegion::subscribe() - stream ended");
        });
    }
}
