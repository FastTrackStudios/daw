//! REAPER Marker Implementation
//!
//! Implements MarkerService by dispatching REAPER API calls to the main thread
//! using `crate::main_thread`.

use crate::main_thread;
use crate::project_context::resolve_project_context;
use crate::safe_wrappers::markers as sw;
use crate::safe_wrappers::ruler_lanes;
use daw_proto::{Marker, MarkerEvent, MarkerService, Position, ProjectContext, TimePosition};
use reaper_medium::{BookmarkRef, MarkerOrRegionPosition, ProjectContext as ReaperProjectContext};
use roam::Tx;
use std::ffi::CString;
use std::time::Duration;
use tracing::{debug, info};

/// REAPER marker implementation.
///
/// All methods dispatch to the main thread via `main_thread`.
#[derive(Clone)]
pub struct ReaperMarker;

impl ReaperMarker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperMarker {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkerService for ReaperMarker {
    // =========================================================================
    // Query Methods
    // =========================================================================

    async fn get_markers(&self, project: ProjectContext) -> Vec<Marker> {
        main_thread::query(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();
            let mut markers = Vec::new();

            // Resolve the project context to a REAPER project context
            let reaper_ctx = resolve_project_context(&project);

            // Get total count of markers and regions
            let count_result = medium.count_project_markers(reaper_ctx);
            let total_count = count_result.total_count;

            // Enumerate all markers/regions
            for idx in 0..total_count {
                medium.enum_project_markers_3(reaper_ctx, idx, |result| {
                    if let Some(info) = result {
                        // region_end_position is None for markers, Some for regions
                        if info.region_end_position.is_none() {
                            let lane = ruler_lanes::get_marker_lane(low, reaper_ctx, idx);
                            markers.push(Marker {
                                id: Some(info.id.get()),
                                position: Position::from_time(TimePosition::from_seconds(
                                    info.position.get(),
                                )),
                                name: info.name.to_string(),
                                color: {
                                    let c = info.color.to_raw();
                                    if c != 0 { Some(c as u32) } else { None }
                                },
                                guid: None,
                                lane,
                            });
                        }
                    }
                });
            }

            // Sort by position
            markers.sort_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            markers
        })
        .await
        .unwrap_or_default()
    }

    async fn get_marker(&self, project: ProjectContext, id: u32) -> Option<Marker> {
        let markers = self.get_markers(project).await;
        markers.into_iter().find(|m| m.id == Some(id))
    }

    async fn get_markers_in_range(
        &self,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Marker> {
        let markers = self.get_markers(project).await;
        markers
            .into_iter()
            .filter(|m| m.is_in_range(start, end))
            .collect()
    }

    async fn get_next_marker(&self, project: ProjectContext, after: f64) -> Option<Marker> {
        let markers = self.get_markers(project).await;
        markers
            .into_iter()
            .filter(|m| m.position_seconds() > after)
            .min_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    async fn get_previous_marker(&self, project: ProjectContext, before: f64) -> Option<Marker> {
        let markers = self.get_markers(project).await;
        markers
            .into_iter()
            .filter(|m| m.position_seconds() < before)
            .max_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    async fn marker_count(&self, project: ProjectContext) -> usize {
        self.get_markers(project).await.len()
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    async fn add_marker(&self, _project: ProjectContext, position: f64, name: String) -> u32 {
        debug!("ReaperMarker: add_marker '{}' at {}", name, position);
        main_thread::query(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            if let Ok(pos) = reaper_medium::PositionInSeconds::new(position) {
                medium
                    .add_project_marker_2(
                        ReaperProjectContext::CurrentProject,
                        MarkerOrRegionPosition::Marker(pos),
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

    async fn remove_marker(&self, _project: ProjectContext, id: u32) {
        debug!("ReaperMarker: remove_marker {}", id);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            sw::delete_project_marker(low, ReaperProjectContext::CurrentProject, id as i32, false);
        });
    }

    async fn move_marker(&self, _project: ProjectContext, id: u32, position: f64) {
        debug!("ReaperMarker: move_marker {} to {}", id, position);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            sw::set_project_marker(low, id as i32, false, position, 0.0, None);
        });
    }

    async fn rename_marker(&self, _project: ProjectContext, id: u32, name: String) {
        debug!("ReaperMarker: rename_marker {} to '{}'", id, name);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            if let Ok(cname) = CString::new(name) {
                sw::set_project_marker(low, id as i32, false, -1.0, 0.0, Some(&cname));
            }
        });
    }

    async fn set_marker_color(&self, _project: ProjectContext, id: u32, color: u32) {
        debug!("ReaperMarker: set_marker_color {} to {}", id, color);
        main_thread::run(move || {
            let low = reaper_high::Reaper::get().medium_reaper().low();
            sw::set_project_marker_by_index2(
                low,
                ReaperProjectContext::CurrentProject,
                id as i32,
                false,
                -1.0,
                0.0,
                -1,
                None,
                color as i32,
                0,
            );
        });
    }

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    async fn goto_next_marker(&self, _project: ProjectContext) {
        debug!("ReaperMarker: goto_next_marker");
        main_thread::run(|| {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            // 40173: Markers: Go to next marker/project end
            medium.main_on_command_ex(
                reaper_medium::CommandId::new(40173),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn goto_previous_marker(&self, _project: ProjectContext) {
        debug!("ReaperMarker: goto_previous_marker");
        main_thread::run(|| {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            // 40172: Markers: Go to previous marker/project start
            medium.main_on_command_ex(
                reaper_medium::CommandId::new(40172),
                0,
                ReaperProjectContext::CurrentProject,
            );
        });
    }

    async fn add_marker_in_lane(
        &self,
        project: ProjectContext,
        position: f64,
        name: String,
        lane: u32,
    ) -> u32 {
        debug!(
            "ReaperMarker: add_marker_in_lane '{}' at {} in lane {}",
            name, position, lane
        );
        let id = self.add_marker(project.clone(), position, name).await;
        if id != 0 {
            // Find the enumeration index for the newly created marker and set its lane
            main_thread::run(move || {
                let reaper = reaper_high::Reaper::get();
                let medium = reaper.medium_reaper();
                let low = medium.low();
                let reaper_ctx = ReaperProjectContext::CurrentProject;
                let count_result = medium.count_project_markers(reaper_ctx);
                let total_count = count_result.total_count;

                for idx in 0..total_count {
                    medium.enum_project_markers_3(reaper_ctx, idx, |result| {
                        if let Some(info) = result {
                            if info.region_end_position.is_none() && info.id.get() == id {
                                ruler_lanes::set_marker_lane(low, reaper_ctx, idx, lane);
                            }
                        }
                    });
                }
            });
        }
        id
    }

    async fn set_marker_lane(&self, _project: ProjectContext, _id: u32, _lane: Option<u32>) {
        // TODO: Implement once GetRegionOrMarkerInfo_Value FFI wrappers are available
        debug!("ReaperMarker: set_marker_lane not yet implemented");
    }

    async fn get_markers_in_lane(&self, project: ProjectContext, lane: u32) -> Vec<Marker> {
        let markers = self.get_markers(project).await;
        markers
            .into_iter()
            .filter(|m| m.lane == Some(lane))
            .collect()
    }

    async fn goto_marker(&self, _project: ProjectContext, id: u32) {
        debug!("ReaperMarker: goto_marker {}", id);
        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            medium.go_to_marker(
                ReaperProjectContext::CurrentProject,
                BookmarkRef::Id(reaper_medium::BookmarkId::new(id as _)),
            );
        });
    }

    async fn subscribe(&self, project: ProjectContext, tx: Tx<MarkerEvent>) {
        info!("ReaperMarker::subscribe() - starting marker stream");

        // Clone self for the spawned task
        let this = self.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        tokio::spawn(async move {
            // Send initial state
            let markers = this.get_markers(project.clone()).await;
            if tx
                .send(MarkerEvent::MarkersChanged(markers.clone()))
                .await
                .is_err()
            {
                debug!("ReaperMarker::subscribe() - client disconnected during initial send");
                return;
            }

            // Poll for changes at 60Hz
            let mut last_markers = markers;

            loop {
                tokio::time::sleep(Duration::from_micros(16667)).await;

                let current_markers = this.get_markers(project.clone()).await;
                if current_markers != last_markers {
                    if tx
                        .send(MarkerEvent::MarkersChanged(current_markers.clone()))
                        .await
                        .is_err()
                    {
                        debug!("ReaperMarker::subscribe() - client disconnected");
                        break;
                    }
                    last_markers = current_markers;
                }
            }

            info!("ReaperMarker::subscribe() - stream ended");
        });
    }
}
