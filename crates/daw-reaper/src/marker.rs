//! REAPER Marker Implementation
//!
//! Implements MarkerService by dispatching REAPER API calls to the main thread
//! using `crate::main_thread`.

use crate::main_thread;
use crate::project_context::resolve_project_context;
use daw_proto::{Marker, MarkerEvent, MarkerService, Position, ProjectContext, TimePosition};
use reaper_medium::{BookmarkRef, MarkerOrRegionPosition, ProjectContext as ReaperProjectContext};
use roam::{Context, Tx};
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

    async fn get_markers(&self, _cx: &Context, project: ProjectContext) -> Vec<Marker> {
        main_thread::query(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
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

    async fn get_marker(&self, cx: &Context, project: ProjectContext, id: u32) -> Option<Marker> {
        let markers = self.get_markers(cx, project).await;
        markers.into_iter().find(|m| m.id == Some(id))
    }

    async fn get_markers_in_range(
        &self,
        cx: &Context,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Marker> {
        let markers = self.get_markers(cx, project).await;
        markers
            .into_iter()
            .filter(|m| m.is_in_range(start, end))
            .collect()
    }

    async fn get_next_marker(
        &self,
        cx: &Context,
        project: ProjectContext,
        after: f64,
    ) -> Option<Marker> {
        let markers = self.get_markers(cx, project).await;
        markers
            .into_iter()
            .filter(|m| m.position_seconds() > after)
            .min_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    async fn get_previous_marker(
        &self,
        cx: &Context,
        project: ProjectContext,
        before: f64,
    ) -> Option<Marker> {
        let markers = self.get_markers(cx, project).await;
        markers
            .into_iter()
            .filter(|m| m.position_seconds() < before)
            .max_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    async fn marker_count(&self, cx: &Context, project: ProjectContext) -> usize {
        self.get_markers(cx, project).await.len()
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    async fn add_marker(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        position: f64,
        name: String,
    ) -> u32 {
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

    async fn remove_marker(&self, _cx: &Context, _project: ProjectContext, id: u32) {
        debug!("ReaperMarker: remove_marker {}", id);
        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            unsafe {
                medium.low().DeleteProjectMarker(
                    ReaperProjectContext::CurrentProject.to_raw(),
                    id as i32,
                    false,
                );
            }
        });
    }

    async fn move_marker(&self, _cx: &Context, _project: ProjectContext, id: u32, position: f64) {
        debug!("ReaperMarker: move_marker {} to {}", id, position);
        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            unsafe {
                medium
                    .low()
                    .SetProjectMarker(id as i32, false, position, 0.0, std::ptr::null());
            }
        });
    }

    async fn rename_marker(&self, _cx: &Context, _project: ProjectContext, id: u32, name: String) {
        debug!("ReaperMarker: rename_marker {} to '{}'", id, name);
        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            if let Ok(cname) = CString::new(name) {
                unsafe {
                    medium
                        .low()
                        .SetProjectMarker(id as i32, false, -1.0, 0.0, cname.as_ptr());
                }
            }
        });
    }

    async fn set_marker_color(&self, _cx: &Context, _project: ProjectContext, id: u32, color: u32) {
        debug!("ReaperMarker: set_marker_color {} to {}", id, color);
        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            let medium = reaper.medium_reaper();
            unsafe {
                medium.low().SetProjectMarkerByIndex2(
                    ReaperProjectContext::CurrentProject.to_raw(),
                    id as i32,
                    false,
                    -1.0,
                    0.0,
                    -1,
                    std::ptr::null(),
                    color as i32,
                    0,
                );
            }
        });
    }

    // =========================================================================
    // Navigation Methods
    // =========================================================================

    async fn goto_next_marker(&self, _cx: &Context, _project: ProjectContext) {
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

    async fn goto_previous_marker(&self, _cx: &Context, _project: ProjectContext) {
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

    async fn goto_marker(&self, _cx: &Context, _project: ProjectContext, id: u32) {
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

    async fn subscribe(&self, cx: &Context, project: ProjectContext, tx: Tx<MarkerEvent>) {
        info!("ReaperMarker::subscribe() - starting marker stream");

        // Clone self for the spawned task
        let this = self.clone();
        let cx = cx.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        peeps::spawn_tracked!("reaper-marker-subscribe", async move {
            // Send initial state
            let markers = this.get_markers(&cx, project.clone()).await;
            if tx
                .send(&MarkerEvent::MarkersChanged(markers.clone()))
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

                let current_markers = this.get_markers(&cx, project.clone()).await;
                if current_markers != last_markers {
                    if tx
                        .send(&MarkerEvent::MarkersChanged(current_markers.clone()))
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
