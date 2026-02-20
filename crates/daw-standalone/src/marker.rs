//! Standalone marker implementation
//!
//! In-memory marker storage with mock data for testing.
//! Each project has its own set of markers.

use crate::project::project_guids;
use daw_proto::{
    Position, ProjectContext, TimePosition,
    marker::{Marker, MarkerEvent, MarkerService},
};
use roam::{Context, Tx};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Internal marker state per project
#[derive(Clone)]
struct MarkerState {
    /// Markers by project GUID
    markers_by_project: HashMap<String, Vec<Marker>>,
    next_id: u32,
    /// Current playhead position (for navigation)
    position: f64,
}

impl Default for MarkerState {
    fn default() -> Self {
        Self {
            markers_by_project: create_mock_markers_by_project(),
            next_id: 100, // Start after mock marker IDs
            position: 0.0,
        }
    }
}

/// Create mock markers for each project
///
/// Each project has its own markers representing the song structure:
/// - COUNT-IN (optional): Start of count-in clicks before song
/// - =START: Absolute start of song timeline (may equal COUNT-IN or SONGSTART)
/// - SONGSTART: Where the actual song content begins (after count-in)
/// - SONGEND: Where the song content ends
/// - =END: Absolute end (sections may continue past SONGEND until =END)
fn create_mock_markers_by_project() -> HashMap<String, Vec<Marker>> {
    let mut markers = HashMap::new();

    // Song 1: "How Great is Our God" (count-in at -4s, song 0-240s, tail to 245s)
    markers.insert(
        project_guids::SONG1.to_string(),
        vec![
            Marker::new_full(
                Some(1),
                Position::from_time(TimePosition::from_seconds(-4.0)),
                "COUNT-IN".to_string(),
                Some(0xFFFF00), // Yellow
                Some("marker-song1-countin".to_string()),
            ),
            Marker::new_full(
                Some(2),
                Position::from_time(TimePosition::from_seconds(-4.0)),
                "=START".to_string(),
                Some(0x00FFFF), // Cyan
                Some("marker-song1-start".to_string()),
            ),
            Marker::new_full(
                Some(3),
                Position::from_time(TimePosition::from_seconds(0.0)),
                "SONGSTART How Great is Our God".to_string(),
                Some(0x00FF00), // Green
                Some("marker-song1-songstart".to_string()),
            ),
            Marker::new_full(
                Some(4),
                Position::from_time(TimePosition::from_seconds(240.0)),
                "SONGEND".to_string(),
                Some(0xFF0000), // Red
                Some("marker-song1-songend".to_string()),
            ),
            Marker::new_full(
                Some(5),
                Position::from_time(TimePosition::from_seconds(245.0)),
                "=END".to_string(),
                Some(0xFF00FF), // Magenta
                Some("marker-song1-end".to_string()),
            ),
        ],
    );

    // Song 2: "Holy, Holy, Holy" (no count-in, song 0-180s, tail to 185s)
    markers.insert(
        project_guids::SONG2.to_string(),
        vec![
            Marker::new_full(
                Some(6),
                Position::from_time(TimePosition::from_seconds(0.0)),
                "=START".to_string(),
                Some(0x00FFFF),
                Some("marker-song2-start".to_string()),
            ),
            Marker::new_full(
                Some(7),
                Position::from_time(TimePosition::from_seconds(0.0)),
                "SONGSTART Holy, Holy, Holy".to_string(),
                Some(0x00FF00),
                Some("marker-song2-songstart".to_string()),
            ),
            Marker::new_full(
                Some(8),
                Position::from_time(TimePosition::from_seconds(180.0)),
                "SONGEND".to_string(),
                Some(0xFF0000),
                Some("marker-song2-songend".to_string()),
            ),
            Marker::new_full(
                Some(9),
                Position::from_time(TimePosition::from_seconds(185.0)),
                "=END".to_string(),
                Some(0xFF00FF),
                Some("marker-song2-end".to_string()),
            ),
        ],
    );

    // Song 3: "Amazing Grace" (count-in at -4s, song 0-200s, tail to 210s)
    markers.insert(
        project_guids::SONG3.to_string(),
        vec![
            Marker::new_full(
                Some(10),
                Position::from_time(TimePosition::from_seconds(-4.0)),
                "COUNT-IN".to_string(),
                Some(0xFFFF00),
                Some("marker-song3-countin".to_string()),
            ),
            Marker::new_full(
                Some(11),
                Position::from_time(TimePosition::from_seconds(-4.0)),
                "=START".to_string(),
                Some(0x00FFFF),
                Some("marker-song3-start".to_string()),
            ),
            Marker::new_full(
                Some(12),
                Position::from_time(TimePosition::from_seconds(0.0)),
                "SONGSTART Amazing Grace".to_string(),
                Some(0x00FF00),
                Some("marker-song3-songstart".to_string()),
            ),
            Marker::new_full(
                Some(13),
                Position::from_time(TimePosition::from_seconds(200.0)),
                "SONGEND".to_string(),
                Some(0xFF0000),
                Some("marker-song3-songend".to_string()),
            ),
            Marker::new_full(
                Some(14),
                Position::from_time(TimePosition::from_seconds(210.0)),
                "=END".to_string(),
                Some(0xFF00FF),
                Some("marker-song3-end".to_string()),
            ),
        ],
    );

    markers
}

/// Standalone marker implementation with mock data
#[derive(Clone)]
pub struct StandaloneMarker {
    state: Arc<RwLock<MarkerState>>,
}

impl Default for StandaloneMarker {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneMarker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(MarkerState::default())),
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

impl StandaloneMarker {
    /// Helper to get markers for a project
    fn get_project_markers<'a>(state: &'a MarkerState, project: &ProjectContext) -> &'a [Marker] {
        if let Some(id) = project_id(project) {
            state
                .markers_by_project
                .get(id)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
        } else {
            // For Current, return markers from the first project (default behavior)
            state
                .markers_by_project
                .values()
                .next()
                .map(|v| v.as_slice())
                .unwrap_or(&[])
        }
    }
}

impl MarkerService for StandaloneMarker {
    async fn get_markers(&self, _cx: &Context, project: ProjectContext) -> Vec<Marker> {
        let state = self.state.read().await;
        Self::get_project_markers(&state, &project).to_vec()
    }

    async fn get_marker(&self, _cx: &Context, project: ProjectContext, id: u32) -> Option<Marker> {
        let state = self.state.read().await;
        Self::get_project_markers(&state, &project)
            .iter()
            .find(|m| m.id == Some(id))
            .cloned()
    }

    async fn get_markers_in_range(
        &self,
        _cx: &Context,
        project: ProjectContext,
        start: f64,
        end: f64,
    ) -> Vec<Marker> {
        let state = self.state.read().await;
        Self::get_project_markers(&state, &project)
            .iter()
            .filter(|m| m.is_in_range(start, end))
            .cloned()
            .collect()
    }

    async fn get_next_marker(
        &self,
        _cx: &Context,
        project: ProjectContext,
        after: f64,
    ) -> Option<Marker> {
        let state = self.state.read().await;
        Self::get_project_markers(&state, &project)
            .iter()
            .filter(|m| m.position_seconds() > after)
            .min_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap()
            })
            .cloned()
    }

    async fn get_previous_marker(
        &self,
        _cx: &Context,
        project: ProjectContext,
        before: f64,
    ) -> Option<Marker> {
        let state = self.state.read().await;
        Self::get_project_markers(&state, &project)
            .iter()
            .filter(|m| m.position_seconds() < before)
            .max_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap()
            })
            .cloned()
    }

    async fn marker_count(&self, _cx: &Context, project: ProjectContext) -> usize {
        let state = self.state.read().await;
        Self::get_project_markers(&state, &project).len()
    }

    async fn add_marker(
        &self,
        _cx: &Context,
        project: ProjectContext,
        position: f64,
        name: String,
    ) -> u32 {
        let mut state = self.state.write().await;
        let id = state.next_id;
        state.next_id += 1;

        let marker = Marker::new_full(
            Some(id),
            Position::from_time(TimePosition::from_seconds(position)),
            name,
            None,
            None,
        );

        let proj_id = project_id(&project)
            .map(String::from)
            .unwrap_or_else(|| project_guids::SONG1.to_string());
        let markers = state
            .markers_by_project
            .entry(proj_id.clone())
            .or_insert_with(Vec::new);
        markers.push(marker);
        markers.sort_by(|a, b| {
            a.position_seconds()
                .partial_cmp(&b.position_seconds())
                .unwrap()
        });

        debug!("Added marker {} at {} in project {}", id, position, proj_id);
        id
    }

    async fn remove_marker(&self, _cx: &Context, project: ProjectContext, id: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get_mut(proj_id)
        {
            markers.retain(|m| m.id != Some(id));
            debug!("Removed marker {} from project {}", id, proj_id);
        }
    }

    async fn move_marker(&self, _cx: &Context, project: ProjectContext, id: u32, position: f64) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get_mut(proj_id)
        {
            if let Some(marker) = markers.iter_mut().find(|m| m.id == Some(id)) {
                marker.position = Position::from_time(TimePosition::from_seconds(position));
                debug!("Moved marker {} to {} in project {}", id, position, proj_id);
            }
            markers.sort_by(|a, b| {
                a.position_seconds()
                    .partial_cmp(&b.position_seconds())
                    .unwrap()
            });
        }
    }

    async fn rename_marker(&self, _cx: &Context, project: ProjectContext, id: u32, name: String) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get_mut(proj_id)
            && let Some(marker) = markers.iter_mut().find(|m| m.id == Some(id))
        {
            marker.name = name.clone();
            debug!("Renamed marker {} to '{}' in project {}", id, name, proj_id);
        }
    }

    async fn set_marker_color(&self, _cx: &Context, project: ProjectContext, id: u32, color: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get_mut(proj_id)
            && let Some(marker) = markers.iter_mut().find(|m| m.id == Some(id))
        {
            marker.color = if color == 0 { None } else { Some(color) };
            debug!(
                "Set marker {} color to {:06x} in project {}",
                id, color, proj_id
            );
        }
    }

    async fn goto_next_marker(&self, _cx: &Context, project: ProjectContext) {
        let mut state = self.state.write().await;
        let current = state.position;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get(proj_id)
            && let Some(marker) = markers
                .iter()
                .filter(|m| m.position_seconds() > current + 0.001)
                .min_by(|a, b| {
                    a.position_seconds()
                        .partial_cmp(&b.position_seconds())
                        .unwrap()
                })
        {
            state.position = marker.position_seconds();
            debug!(
                "Navigated to next marker at {} in project {}",
                state.position, proj_id
            );
        }
    }

    async fn goto_previous_marker(&self, _cx: &Context, project: ProjectContext) {
        let mut state = self.state.write().await;
        let current = state.position;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get(proj_id)
            && let Some(marker) = markers
                .iter()
                .filter(|m| m.position_seconds() < current - 0.001)
                .max_by(|a, b| {
                    a.position_seconds()
                        .partial_cmp(&b.position_seconds())
                        .unwrap()
                })
        {
            state.position = marker.position_seconds();
            debug!(
                "Navigated to previous marker at {} in project {}",
                state.position, proj_id
            );
        }
    }

    async fn goto_marker(&self, _cx: &Context, project: ProjectContext, id: u32) {
        let mut state = self.state.write().await;
        if let Some(proj_id) = project_id(&project)
            && let Some(markers) = state.markers_by_project.get(proj_id)
            && let Some(marker) = markers.iter().find(|m| m.id == Some(id))
        {
            state.position = marker.position_seconds();
            debug!(
                "Navigated to marker {} at {} in project {}",
                id, state.position, proj_id
            );
        }
    }

    async fn subscribe(&self, _cx: &Context, project: ProjectContext, tx: Tx<MarkerEvent>) {
        info!("MarkerService::subscribe() - starting marker stream");

        // Clone state for the spawned task
        let state = self.state.clone();

        // Spawn the streaming loop so this method returns immediately
        // (roam needs the method to return so it can send the Response)
        peeps::spawn_tracked!("marker-subscribe", async move {
            // Send initial state: all markers for this project
            let markers = {
                let state = state.read().await;
                StandaloneMarker::get_project_markers(&state, &project).to_vec()
            };

            if tx
                .send(&MarkerEvent::MarkersChanged(markers.clone()))
                .await
                .is_err()
            {
                debug!("MarkerService::subscribe() - client disconnected during initial send");
                return;
            }

            // Poll for changes at 2Hz (markers rarely change, no need for 60Hz)
            let mut last_markers = markers;

            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Check for marker changes
                let current_markers = {
                    let state = state.read().await;
                    StandaloneMarker::get_project_markers(&state, &project).to_vec()
                };

                if current_markers != last_markers {
                    // Send full marker list on change
                    // (A more sophisticated implementation would send granular events)
                    if tx
                        .send(&MarkerEvent::MarkersChanged(current_markers.clone()))
                        .await
                        .is_err()
                    {
                        debug!("MarkerService::subscribe() - client disconnected");
                        break;
                    }
                    last_markers = current_markers;
                }
            }

            info!("MarkerService::subscribe() - stream ended");
        });
    }
}
