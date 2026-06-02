//! `impl Markers for Standalone` — post-architect::rpc port.
//!
//! The standalone backend is single-threaded and lives in-process, so
//! its dispatcher is the current thread. All state is held in the
//! existing per-project `ProjectState::markers` map keyed by
//! `next_marker_id`.
//!
//! The 500-line async `MarkerService` impl that used to live here
//! (event streaming, mock-data scaffolding, navigation goto, range
//! queries, lane methods) was retired alongside the sync trait
//! consolidation — see the architect refactor notes. Restoring any
//! of those surfaces happens on a sibling trait, not by re-bloating
//! the canonical `Markers`.

use architect::HasDispatcher;
use architect::dispatch::CurrentThreadDispatcher;
use daw_proto::Markers;
use daw_proto::marker::{MarkerEvent, MarkerStreamEvent};
use daw_proto::{DawError, DawResult, Marker, Position, PositionInSeconds, ProjectContext};

use crate::sync::Standalone;

impl HasDispatcher for Standalone {
    type Dispatcher = CurrentThreadDispatcher;

    fn dispatcher(&self) -> Self::Dispatcher {
        CurrentThreadDispatcher
    }
}

fn publish_marker_event(daw: &Standalone, project_guid: &str, event: MarkerEvent) {
    let _ = daw.marker_events.send(MarkerStreamEvent {
        project_guid: project_guid.to_string(),
        event,
    });
}

impl Markers for Standalone {
    fn all(&self, project: ProjectContext) -> Vec<Marker> {
        let guid = match resolve_project(self, &project) {
            Some(g) => g,
            None => return Vec::new(),
        };
        self.with_project(&guid, |p| p.markers.values().cloned().collect())
            .unwrap_or_default()
    }

    fn get(&self, project: ProjectContext, id: u32) -> Option<Marker> {
        let guid = resolve_project(self, &project)?;
        self.with_project(&guid, |p| p.markers.get(&id).cloned())
            .ok()
            .flatten()
    }

    fn count(&self, project: ProjectContext) -> u32 {
        let Some(guid) = resolve_project(self, &project) else {
            return 0;
        };
        self.with_project(&guid, |p| p.markers.len() as u32)
            .unwrap_or(0)
    }

    fn add(&self, project: ProjectContext, position: f64, name: &str) -> DawResult<u32> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("Project", "current"))?;
        let (id, marker) = self.with_project_mut(&guid, |p| {
            let id = p.next_marker_id;
            p.next_marker_id += 1;
            let marker = Marker {
                id: Some(id),
                ..Marker::new(
                    Position::from_time(PositionInSeconds::from_seconds(position)),
                    name.to_string(),
                )
            };
            p.markers.insert(id, marker.clone());
            (id, marker)
        })?;
        publish_marker_event(self, &guid, MarkerEvent::Added(marker));
        Ok(id)
    }

    fn remove(&self, project: ProjectContext, id: u32) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("Project", "current"))?;
        self.with_project_mut(&guid, |p| {
            p.markers
                .remove(&id)
                .map(|_| ())
                .ok_or_else(|| DawError::not_found("Marker", &id.to_string()))
        })??;
        publish_marker_event(self, &guid, MarkerEvent::Removed(id));
        Ok(())
    }

    fn set_position(&self, project: ProjectContext, id: u32, position: f64) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("Project", "current"))?;
        let marker = self.with_project_mut(&guid, |p| {
            let m = p
                .markers
                .get_mut(&id)
                .ok_or_else(|| DawError::not_found("Marker", &id.to_string()))?;
            m.position = Position::from_time(PositionInSeconds::from_seconds(position));
            Ok::<_, DawError>(m.clone())
        })??;
        publish_marker_event(self, &guid, MarkerEvent::Changed(marker));
        Ok(())
    }

    fn rename(&self, project: ProjectContext, id: u32, name: &str) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("Project", "current"))?;
        let marker = self.with_project_mut(&guid, |p| {
            let m = p
                .markers
                .get_mut(&id)
                .ok_or_else(|| DawError::not_found("Marker", &id.to_string()))?;
            m.name = name.to_string();
            Ok::<_, DawError>(m.clone())
        })??;
        publish_marker_event(self, &guid, MarkerEvent::Changed(marker));
        Ok(())
    }

    fn set_color(&self, project: ProjectContext, id: u32, color: u32) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("Project", "current"))?;
        let marker = self.with_project_mut(&guid, |p| {
            let m = p
                .markers
                .get_mut(&id)
                .ok_or_else(|| DawError::not_found("Marker", &id.to_string()))?;
            m.color = if color == 0 { None } else { Some(color) };
            Ok::<_, DawError>(m.clone())
        })??;
        publish_marker_event(self, &guid, MarkerEvent::Changed(marker));
        Ok(())
    }

    fn set_lane(&self, project: ProjectContext, id: u32, lane: Option<u32>) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("Project", "current"))?;
        let marker = self.with_project_mut(&guid, |p| {
            let m = p
                .markers
                .get_mut(&id)
                .ok_or_else(|| DawError::not_found("Marker", &id.to_string()))?;
            m.lane = lane;
            Ok::<_, DawError>(m.clone())
        })??;
        publish_marker_event(self, &guid, MarkerEvent::Changed(marker));
        Ok(())
    }

    async fn subscribe(
        &self,
        project: ProjectContext,
        tx: vox::Tx<daw_proto::marker::MarkerStreamEvent>,
    ) {
        let project_guid = resolve_project(self, &project);
        let mut rx = self.marker_events.subscribe();
        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if project_guid
                            .as_ref()
                            .is_some_and(|guid| event.project_guid != *guid)
                        {
                            continue;
                        }
                        if tx.send(event).await.is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }
}

/// Map a `ProjectContext` onto a concrete guid the standalone state
/// can index. `Current` resolves to the standalone's tracked current
/// project (or `None` if none was ever seeded).
fn resolve_project(daw: &Standalone, ctx: &ProjectContext) -> Option<String> {
    match ctx {
        ProjectContext::Project(guid) => Some(guid.clone()),
        ProjectContext::Current => {
            let state = daw.state.lock().ok()?;
            state.current_project_guid.clone()
        }
    }
}
