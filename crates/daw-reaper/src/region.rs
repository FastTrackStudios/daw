//! REAPER Region Implementation
//!
//! Implements RegionService by dispatching REAPER API calls to the main thread
//! using TaskSupport from reaper-high.

use crate::transport::task_support;
use daw_proto::{ProjectContext, Region, RegionEvent, RegionService, TimeRange};
use reaper_medium::{
    MarkerOrRegionPosition, PositionInSeconds, ProjectContext as ReaperProjectContext,
};
use roam::{Context, Tx};
use std::ffi::CString;
use std::time::Duration;
use tracing::{debug, info};

/// REAPER region implementation.
///
/// All methods dispatch to the main thread via TaskSupport.
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

    async fn get_regions(&self, _cx: &Context, _project: ProjectContext) -> Vec<Region> {
        if let Some(ts) = task_support() {
            ts.main_thread_future(|| {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                let mut regions = Vec::new();

                // Get total count of markers and regions
                let count_result =
                    medium.count_project_markers(ReaperProjectContext::CurrentProject);
                let total_count = count_result.total_count;

                // Enumerate all markers/regions
                for idx in 0..total_count {
                    medium.enum_project_markers_3(
                        ReaperProjectContext::CurrentProject,
                        idx,
                        |result| {
                            if let Some(info) = result {
                                // region_end_position is Some for regions, None for markers
                                if let Some(end_pos) = info.region_end_position {
                                    regions.push(Region {
                                        id: Some(info.id.get() as u32),
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
                        },
                    );
                }

                // Sort by start position
                regions.sort_by(|a, b| {
                    a.start_seconds()
                        .partial_cmp(&b.start_seconds())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                regions
            })
            .await
            .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    async fn get_region(&self, cx: &Context, project: ProjectContext, id: u32) -> Option<Region> {
        let regions = self.get_regions(cx, project).await;
        regions.into_iter().find(|r| r.id == Some(id))
    }

    async fn get_regions_in_range(
        &self,
        cx: &Context,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Region> {
        let regions = self.get_regions(cx, project).await;
        regions
            .into_iter()
            .filter(|r| r.intersects_range(start, end))
            .collect()
    }

    async fn get_region_at(
        &self,
        cx: &Context,
        project: ProjectContext,
        position: f64,
    ) -> Option<Region> {
        let regions = self.get_regions(cx, project).await;
        regions.into_iter().find(|r| r.contains_position(position))
    }

    async fn region_count(&self, cx: &Context, project: ProjectContext) -> usize {
        self.get_regions(cx, project).await.len()
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    async fn add_region(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        start: f64,
        end: f64,
        name: String,
    ) -> u32 {
        debug!(
            "ReaperRegion: add_region '{}' from {} to {}",
            name, start, end
        );
        if let Some(ts) = task_support() {
            ts.main_thread_future(move || {
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
        } else {
            0
        }
    }

    async fn remove_region(&self, _cx: &Context, _project: ProjectContext, id: u32) {
        debug!("ReaperRegion: remove_region {}", id);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                unsafe {
                    medium.low().DeleteProjectMarker(
                        ReaperProjectContext::CurrentProject.to_raw(),
                        id as i32,
                        true, // is a region
                    );
                }
            });
        }
    }

    async fn set_region_bounds(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        id: u32,
        start: f64,
        end: f64,
    ) {
        debug!(
            "ReaperRegion: set_region_bounds {} to {} - {}",
            id, start, end
        );
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                unsafe {
                    medium.low().SetProjectMarker(
                        id as i32,
                        true, // is a region
                        start,
                        end,
                        std::ptr::null(),
                    );
                }
            });
        }
    }

    async fn rename_region(&self, _cx: &Context, _project: ProjectContext, id: u32, name: String) {
        debug!("ReaperRegion: rename_region {} to '{}'", id, name);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                if let Ok(cname) = CString::new(name) {
                    unsafe {
                        medium
                            .low()
                            .SetProjectMarker(id as i32, true, -1.0, -1.0, cname.as_ptr());
                    }
                }
            });
        }
    }

    async fn set_region_color(&self, _cx: &Context, _project: ProjectContext, id: u32, color: u32) {
        debug!("ReaperRegion: set_region_color {} to {}", id, color);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                unsafe {
                    medium.low().SetProjectMarkerByIndex2(
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
                }
            });
        }
    }

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    async fn goto_region_start(&self, _cx: &Context, _project: ProjectContext, id: u32) {
        debug!("ReaperRegion: goto_region_start {}", id);
        if let Some(ts) = task_support() {
            let _ = ts.do_later_in_main_thread_asap(move || {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                // Go to region by ID (uses same API as markers)
                medium.go_to_marker(
                    ReaperProjectContext::CurrentProject,
                    reaper_medium::BookmarkRef::Id(reaper_medium::BookmarkId::new(id as _)),
                );
            });
        }
    }

    async fn goto_region_end(&self, cx: &Context, project: ProjectContext, id: u32) {
        debug!("ReaperRegion: goto_region_end {}", id);
        // Get the region's end position and set cursor there
        if let Some(region) = self.get_region(cx, project, id).await {
            let end_pos = region.end_seconds();
            if let Some(ts) = task_support() {
                let _ = ts.do_later_in_main_thread_asap(move || {
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
    }

    async fn subscribe(&self, cx: &Context, project: ProjectContext, tx: Tx<RegionEvent>) {
        info!("ReaperRegion::subscribe() - starting region stream");

        // Clone self for the spawned task
        let this = self.clone();
        let cx = cx.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        tokio::spawn(async move {
            // Send initial state
            let regions = this.get_regions(&cx, project.clone()).await;
            if tx
                .send(&RegionEvent::RegionsChanged(regions.clone()))
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

                let current_regions = this.get_regions(&cx, project.clone()).await;
                if current_regions != last_regions {
                    if tx
                        .send(&RegionEvent::RegionsChanged(current_regions.clone()))
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
