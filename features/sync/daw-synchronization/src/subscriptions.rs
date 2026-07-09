//! Per-project subscriptions that forward local DAW events as `SyncEvent`s.
//!
//! Subscribes directly to `daw_reaper::event_hub()` broadcast channels and
//! wraps every event in a [`SyncEvent`] envelope with this peer's identity.
//! No vox round-trip — the engine and the hub live in the same process; the
//! vox streaming clients exist for *out-of-process* consumers, not for us.
//!
//! One forwarder task per enabled domain. Cancellation is shared via a
//! single [`CancellationToken`] held by the returned [`ProjectSubscriptions`].
//!
//! Domains not yet wired through `DawEventHub` (items, takes, fx, routing,
//! action_registry, project, live_midi) have no forwarder here — they'll
//! plug in as streaming Phase 2 finishes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::suppression::SuppressionSet;
use crate::{SyncConfig, SyncDomain, SyncEvent};

/// Handle to a set of subscriptions for a single project.
///
/// Dropping cancels every forwarder task via the shared cancellation token.
pub struct ProjectSubscriptions {
    project_guid: String,
    cancel: CancellationToken,
}

impl ProjectSubscriptions {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn project_guid(&self) -> &str {
        &self.project_guid
    }
}

impl Drop for ProjectSubscriptions {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Spawn per-domain forwarder tasks for the given project.
///
/// Each enabled domain in `config` gets one task that reads from the local
/// event hub, wraps the event in a `SyncEvent`, and broadcasts on `event_tx`.
/// Echo suppression filtering happens at broadcast time inside the engine
/// (see `SynchronizationEngine::broadcast_local`); these forwarders are
/// intentionally suppression-free so the engine sees every local event.
#[allow(clippy::too_many_arguments)]
pub async fn subscribe_project(
    daw: &daw::rpc::Daw,
    _project: &daw::rpc::Project,
    project_guid: String,
    peer_id: String,
    sequence: Arc<AtomicU64>,
    event_tx: broadcast::Sender<SyncEvent>,
    _suppression: Arc<Mutex<SuppressionSet>>,
    config: &SyncConfig,
) -> Result<ProjectSubscriptions, daw::rpc::Error> {
    let cancel = CancellationToken::new();
    let ctx = ForwarderCtx {
        project_guid: project_guid.clone(),
        peer_id,
        sequence,
        event_tx,
        cancel: cancel.clone(),
    };

    if config.transport {
        spawn_transport_forwarder(ctx.clone(), daw.clone());
    }
    if config.markers {
        spawn_marker_forwarder(ctx.clone());
    }
    if config.regions {
        spawn_region_forwarder(ctx.clone());
    }
    if config.tracks {
        spawn_track_forwarder(ctx.clone());
    }
    if config.tempo_map {
        spawn_tempo_map_forwarder(ctx.clone());
    }
    if config.items {
        spawn_item_forwarder(ctx.clone());
        // Takes follow items config (see `is_domain_enabled`).
        spawn_take_forwarder(ctx.clone());
    }
    if config.fx {
        spawn_fx_forwarder(ctx.clone());
    }
    if config.routing {
        spawn_routing_forwarder(ctx.clone());
    }

    Ok(ProjectSubscriptions {
        project_guid,
        cancel,
    })
}

/// Spawn a watcher that auto-subscribes new projects as they open.
///
/// Currently a no-op stub. Project open/close events aren't yet wired
/// through the streaming surface; once a `ProjectStream` lands, this
/// should poll/subscribe and call `subscribe_project` for new projects.
#[allow(clippy::too_many_arguments)]
pub fn watch_projects(
    _daw: daw::rpc::Daw,
    _peer_id: String,
    _sequence: Arc<AtomicU64>,
    _event_tx: broadcast::Sender<SyncEvent>,
    _suppression: Arc<Mutex<SuppressionSet>>,
    _config: SyncConfig,
    _project_subs: Arc<Mutex<Vec<ProjectSubscriptions>>>,
) -> CancellationToken {
    CancellationToken::new()
}

// ── Forwarder plumbing ──────────────────────────────────────────────

#[derive(Clone)]
struct ForwarderCtx {
    project_guid: String,
    peer_id: String,
    sequence: Arc<AtomicU64>,
    event_tx: broadcast::Sender<SyncEvent>,
    cancel: CancellationToken,
}

impl ForwarderCtx {
    fn wrap(&self, project_guid: String, domain: SyncDomain) -> SyncEvent {
        SyncEvent {
            origin_peer: self.peer_id.clone(),
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            project_guid,
            domain,
            created_at_ms: SyncEvent::now_ms(),
        }
    }
}

fn spawn_transport_forwarder(ctx: ForwarderCtx, daw: daw::rpc::Daw) {
    use daw::service::TransportEvent;
    let mut rx = daw_reaper::event_hub().subscribe_transport_state();
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(event) => {
                        // Extract project_guid from whatever variant we got.
                        let event_guid = match &event {
                            TransportEvent::Snapshot { project_guid, .. }
                            | TransportEvent::PlayStateChanged { project_guid, .. }
                            | TransportEvent::RecordModeChanged { project_guid, .. }
                            | TransportEvent::LoopingChanged { project_guid, .. }
                            | TransportEvent::MetronomeChanged { project_guid, .. }
                            | TransportEvent::LoopRegionChanged { project_guid, .. }
                            | TransportEvent::TimeSelectionChanged { project_guid, .. }
                            | TransportEvent::TempoChanged { project_guid, .. }
                            | TransportEvent::PlayrateChanged { project_guid, .. } => {
                                project_guid.clone()
                            }
                        };
                        if event_guid != target_guid {
                            continue;
                        }
                        // SyncDomain::Transport currently carries a full
                        // snapshot. For non-Snapshot deltas, query current
                        // state and forward that. This is a snapshot-style
                        // sync — every transport change publishes the full
                        // post-change state to peers.
                        let snapshot = match event {
                            TransportEvent::Snapshot { state, .. } => state,
                            _ => match current_transport(&daw).await {
                                Some(s) => s,
                                None => continue,
                            },
                        };
                        let sync_event = ctx.wrap(event_guid, SyncDomain::Transport(snapshot));
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("transport forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

async fn current_transport(daw: &daw::rpc::Daw) -> Option<daw::service::Transport> {
    let project = daw.current_project().await.ok()?;
    project.transport().get_state().await.ok()
}

fn spawn_marker_forwarder(ctx: ForwarderCtx) {
    let mut rx = daw_reaper::event_hub().subscribe_markers();
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(stream_event) => {
                        if stream_event.project_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(
                            stream_event.project_guid,
                            SyncDomain::Marker(stream_event.event),
                        );
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("marker forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_region_forwarder(ctx: ForwarderCtx) {
    let mut rx = daw_reaper::event_hub().subscribe_regions();
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(stream_event) => {
                        if stream_event.project_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(
                            stream_event.project_guid,
                            SyncDomain::Region(stream_event.event),
                        );
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("region forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_track_forwarder(ctx: ForwarderCtx) {
    let mut rx = daw_reaper::event_hub().subscribe_tracks();
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(stream_event) => {
                        if stream_event.project_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(
                            stream_event.project_guid,
                            SyncDomain::Track(stream_event.event),
                        );
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("track forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_item_forwarder(ctx: ForwarderCtx) {
    use daw::service::ItemEvent;
    let Some(mut rx) = daw_reaper::subscribe_items() else {
        return;
    };
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(event) => {
                        let event_guid = match &event {
                            ItemEvent::Created { project_guid, .. }
                            | ItemEvent::Deleted { project_guid, .. }
                            | ItemEvent::PositionChanged { project_guid, .. }
                            | ItemEvent::LengthChanged { project_guid, .. }
                            | ItemEvent::MovedToTrack { project_guid, .. }
                            | ItemEvent::MuteChanged { project_guid, .. }
                            | ItemEvent::SelectionChanged { project_guid, .. }
                            | ItemEvent::VolumeChanged { project_guid, .. }
                            | ItemEvent::ActiveTakeChanged { project_guid, .. } => {
                                project_guid.clone()
                            }
                        };
                        if event_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(event_guid, SyncDomain::Item(event));
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("item forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_fx_forwarder(ctx: ForwarderCtx) {
    let Some(mut rx) = daw_reaper::subscribe_fx() else {
        return;
    };
    // FxEvent variants don't carry `project_guid` directly — chain
    // contexts only have track guids. Since the bridge has one
    // FX broadcaster per process and each daw-bridge serves one
    // REAPER instance with one active project tab at a time for the
    // sync use case, wrap every event with this subscription's
    // target_guid. The apply side resolves chain contexts by track
    // guid + chain kind, so cross-project leakage is not a concern
    // in practice; tighten when multi-project sync per peer lands.
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(event) => {
                        let sync_event = ctx.wrap(target_guid.clone(), SyncDomain::Fx(event));
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("fx forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_routing_forwarder(ctx: ForwarderCtx) {
    use daw::service::RoutingEvent;
    let Some(mut rx) = daw_reaper::subscribe_routing() else {
        return;
    };
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(event) => {
                        let event_guid = match &event {
                            RoutingEvent::RouteCreated { project_guid, .. }
                            | RoutingEvent::RouteDeleted { project_guid, .. }
                            | RoutingEvent::VolumeChanged { project_guid, .. }
                            | RoutingEvent::PanChanged { project_guid, .. }
                            | RoutingEvent::MuteChanged { project_guid, .. }
                            | RoutingEvent::ParentSendChanged { project_guid, .. } => {
                                project_guid.clone()
                            }
                        };
                        if event_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(event_guid, SyncDomain::Routing(event));
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("routing forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_take_forwarder(ctx: ForwarderCtx) {
    use daw::service::TakeEvent;
    let Some(mut rx) = daw_reaper::subscribe_takes() else {
        return;
    };
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(event) => {
                        let event_guid = match &event {
                            TakeEvent::Created { project_guid, .. }
                            | TakeEvent::Deleted { project_guid, .. }
                            | TakeEvent::NameChanged { project_guid, .. }
                            | TakeEvent::PitchChanged { project_guid, .. }
                            | TakeEvent::PlayRateChanged { project_guid, .. }
                            | TakeEvent::VolumeChanged { project_guid, .. }
                            | TakeEvent::SourceChanged { project_guid, .. } => {
                                project_guid.clone()
                            }
                        };
                        if event_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(event_guid, SyncDomain::Take(event));
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("take forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}

fn spawn_tempo_map_forwarder(ctx: ForwarderCtx) {
    let mut rx = daw_reaper::event_hub().subscribe_tempo_map();
    let target_guid = ctx.project_guid.clone();
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => break,
                result = rx.recv() => match result {
                    Ok(stream_event) => {
                        if stream_event.project_guid != target_guid {
                            continue;
                        }
                        let sync_event = ctx.wrap(
                            stream_event.project_guid,
                            SyncDomain::TempoMap(stream_event.event),
                        );
                        let _ = ctx.event_tx.send(sync_event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("tempo_map forwarder lagged {n} events");
                    }
                }
            }
        }
    });
}
