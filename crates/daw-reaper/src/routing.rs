//! REAPER Routing Implementation (Send/Receive/Hardware Output)
//!
//! Implements RoutingService for REAPER by dispatching operations to the main thread
//! using `crate::main_thread`.

use crate::main_thread;
use crate::project_context::{find_project_by_guid, project_guid as project_guid_from};
use crate::safe_wrappers::routing as routing_sw;
use daw_proto::{
    AutomationMode, ChannelMapping, MidiChannelMapping, MidiDestinationChannel, MidiSourceChannel,
    ProjectContext, RouteLocation, RouteRef, RouteType, RoutingEvent, RoutingService, SendMode,
    TrackRef, TrackRoute,
};
use reaper_high::{Project, Reaper, SendPartnerType, Track, TrackRoute as ReaperTrackRoute};
use reaper_medium::{
    EditMode, ProjectRef, ReaperVolumeValue, SendTarget, TrackSendAttributeKey, TrackSendCategory,
};
use roam::Tx;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

// =============================================================================
// Routing Change Detection Broadcasting Infrastructure
//
// Follows the same reactive polling pattern as transport.rs and fx.rs:
// poll_and_broadcast_routing() is called from the timer callback on the main
// thread at ~30Hz. It reads current send/receive/hw-output counts per track,
// diffs against a cache, and only broadcasts RoutingEvent when something
// actually changed.
// =============================================================================

/// Cached routing counts for a single track (for change detection)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CachedTrackRouting {
    send_count: u32,
    receive_count: u32,
    hw_output_count: u32,
}

/// Cached routing state for an entire project (per-track routing counts)
#[derive(Clone, Debug, Default)]
struct CachedRoutingState {
    tracks: HashMap<String, CachedTrackRouting>,
}

/// Global routing event broadcaster
static ROUTING_BROADCASTER: OnceLock<broadcast::Sender<RoutingEvent>> = OnceLock::new();

/// Cached per-project routing states for change detection
/// Key is project GUID, value is last known routing state
static ROUTING_CACHE: OnceLock<Mutex<HashMap<String, CachedRoutingState>>> = OnceLock::new();

/// Initialize the routing broadcaster.
/// Called by the extension during initialization.
pub fn init_routing_broadcaster() {
    let (tx, _rx) = broadcast::channel::<RoutingEvent>(256);
    let _ = ROUTING_BROADCASTER.set(tx);
    let _ = ROUTING_CACHE.set(Mutex::new(HashMap::new()));
}

/// Get a receiver for routing change events.
fn routing_receiver() -> Option<broadcast::Receiver<RoutingEvent>> {
    ROUTING_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Get project GUID from a REAPER project (delegates to shared implementation).
fn project_guid(project: &Project) -> String {
    project_guid_from(project)
}

/// Poll REAPER routing state for ALL open projects and broadcast changes.
/// **MUST be called from the main thread** (e.g., from timer callback).
///
/// This function reads REAPER state directly without async overhead,
/// enabling low-latency change detection.
///
/// **Reactive Pattern**: Only broadcasts when a track's route counts actually change.
/// Compares send/receive/hw-output counts per track against the cache.
pub fn poll_and_broadcast_routing() {
    let Some(tx) = ROUTING_BROADCASTER.get() else {
        return;
    };

    // Skip if no subscribers
    if tx.receiver_count() == 0 {
        return;
    }

    let Some(cache) = ROUTING_CACHE.get() else {
        return;
    };
    let mut cache_guard = cache.lock().unwrap();

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    let mut seen_guids = Vec::new();

    // Iterate through all open projects
    for tab_index in 0..128u32 {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            break;
        };

        let project = Project::new(result.project);
        let guid = project_guid(&project);
        seen_guids.push(guid.clone());

        let cached_state = cache_guard
            .entry(guid.clone())
            .or_insert_with(CachedRoutingState::default);

        let mut seen_track_guids = Vec::new();

        // Iterate all tracks in this project (including master)
        let track_count = project.track_count();
        for track_idx in 0..track_count {
            let Some(track) = project.track_by_index(track_idx) else {
                continue;
            };
            let track_guid = track.guid().to_string_without_braces();
            seen_track_guids.push(track_guid.clone());

            let current = CachedTrackRouting {
                send_count: track.typed_sends(SendPartnerType::Track).count() as u32,
                receive_count: track.receives().count() as u32,
                hw_output_count: track
                    .typed_sends(SendPartnerType::HardwareOutput)
                    .count() as u32,
            };

            let prev = cached_state
                .tracks
                .get(&track_guid)
                .cloned()
                .unwrap_or_default();

            if current != prev {
                // Detect created/deleted sends
                emit_count_changes(
                    tx,
                    &guid,
                    &track_guid,
                    &track,
                    RouteType::Send,
                    prev.send_count,
                    current.send_count,
                );
                // Detect created/deleted receives
                emit_count_changes(
                    tx,
                    &guid,
                    &track_guid,
                    &track,
                    RouteType::Receive,
                    prev.receive_count,
                    current.receive_count,
                );
                // Detect created/deleted hardware outputs
                emit_count_changes(
                    tx,
                    &guid,
                    &track_guid,
                    &track,
                    RouteType::HardwareOutput,
                    prev.hw_output_count,
                    current.hw_output_count,
                );

                cached_state.tracks.insert(track_guid, current);
            }
        }

        // Clean up cache entries for tracks that no longer exist
        cached_state
            .tracks
            .retain(|guid, _| seen_track_guids.contains(guid));
    }

    // Clean up cache entries for projects that are no longer open
    cache_guard.retain(|guid, _| seen_guids.contains(guid));
}

/// Emit RouteCreated or RouteDeleted events based on count changes.
///
/// When count increases, we emit RouteCreated for each new route (the last N routes).
/// When count decreases, we emit RouteDeleted for each removed route.
fn emit_count_changes(
    tx: &broadcast::Sender<RoutingEvent>,
    project_guid: &str,
    track_guid: &str,
    track: &Track,
    route_type: RouteType,
    old_count: u32,
    new_count: u32,
) {
    if new_count > old_count {
        // Routes were added - emit RouteCreated for the new ones
        for idx in old_count..new_count {
            // Try to build a full TrackRoute for the new route
            let route = match route_type {
                RouteType::Send => track
                    .typed_sends(SendPartnerType::Track)
                    .nth(idx as usize)
                    .map(|r| convert_track_route(&r, RouteType::Send, idx)),
                RouteType::Receive => track
                    .receives()
                    .nth(idx as usize)
                    .map(|r| convert_track_route(&r, RouteType::Receive, idx)),
                RouteType::HardwareOutput => track
                    .typed_sends(SendPartnerType::HardwareOutput)
                    .nth(idx as usize)
                    .map(|r| convert_track_route(&r, RouteType::HardwareOutput, idx)),
            };

            if let Some(route) = route {
                let _ = tx.send(RoutingEvent::RouteCreated {
                    project_guid: project_guid.to_string(),
                    source_track_guid: track_guid.to_string(),
                    route,
                });
            }
        }
    } else if new_count < old_count {
        // Routes were removed - emit RouteDeleted for the removed ones
        // We don't know exactly which routes were removed, so emit for the
        // indices that no longer exist (from new_count to old_count)
        for idx in new_count..old_count {
            let _ = tx.send(RoutingEvent::RouteDeleted {
                project_guid: project_guid.to_string(),
                source_track_guid: track_guid.to_string(),
                route_type,
                route_index: idx,
            });
        }
    }
}

/// REAPER routing implementation that dispatches to the main thread via `main_thread`.
#[derive(Clone)]
pub struct ReaperRouting;

impl ReaperRouting {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperRouting {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions for Track Resolution
// ============================================================================

/// Resolve a ProjectContext to a REAPER Project
fn resolve_project(ctx: &ProjectContext) -> Option<Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

/// Resolve a TrackRef to a REAPER Track within a project.
///
/// Validates the raw MediaTrack pointer after resolution to guard against
/// stale pointers from deleted tracks.
fn resolve_track(project: &Project, track_ref: &TrackRef) -> Option<Track> {
    let track = match track_ref {
        TrackRef::Master => project.master_track().ok()?,
        TrackRef::Index(idx) => project.track_by_index(*idx)?,
        TrackRef::Guid(guid) => {
            let mut found = None;
            for i in 0..project.track_count() {
                if let Some(track) = project.track_by_index(i)
                    && track.guid().to_string_without_braces() == *guid
                {
                    found = Some(track);
                    break;
                }
            }
            found?
        }
    };
    if !main_thread::is_track_valid(project, &track) {
        return None;
    }
    Some(track)
}

/// Find a track by name within a specific project
pub fn find_track_by_name(project: &Project, name: &str) -> Option<Track> {
    let name_lower = name.to_lowercase();
    for i in 0..project.track_count() {
        if let Some(track) = project.track_by_index(i)
            && track.name().map(|n| n.to_str().to_lowercase()) == Some(name_lower.clone())
        {
            return Some(track);
        }
    }
    None
}

/// Find a project by name (e.g., "FTS-ROUTING")
pub fn find_project_by_name(name: &str) -> Option<Project> {
    let reaper = Reaper::get();
    let name_upper = name.to_uppercase();

    for tab_index in 0..128u32 {
        if let Some(result) = reaper
            .medium_reaper()
            .enum_projects(reaper_medium::ProjectRef::Tab(tab_index), 0)
        {
            let project = Project::new(result.project);
            let project_name = project
                .file()
                .and_then(|p| {
                    std::path::Path::new(&p.to_string())
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "Untitled".to_string());

            if project_name.to_uppercase().contains(&name_upper) {
                return Some(project);
            }
        } else {
            break;
        }
    }
    None
}

// ============================================================================
// Conversion Functions (REAPER types <-> daw-proto types)
// ============================================================================

/// Convert a REAPER TrackRoute to a daw-proto TrackRoute
fn convert_track_route(
    reaper_route: &ReaperTrackRoute,
    route_type: RouteType,
    index: u32,
) -> TrackRoute {
    let source_track = reaper_route.track();
    let source_track_guid = source_track.guid().to_string_without_braces();

    // Get partner info
    let (dest_track_guid, dest_track_name, hw_output_index, hw_output_name) =
        match reaper_route.partner() {
            Some(reaper_high::TrackRoutePartner::Track(track)) => {
                let guid = track.guid().to_string_without_braces();
                let name = track.name().map(|n| n.to_str().to_string());
                (Some(guid), name, None, None)
            }
            Some(reaper_high::TrackRoutePartner::HardwareOutput(idx)) => {
                // Get hardware output name from REAPER
                let name = reaper_route.name().to_str().to_string();
                (None, None, Some(idx), Some(name))
            }
            None => (None, None, None, None),
        };

    // Get volume and pan
    let volume = reaper_route.volume().map(|v| v.get()).unwrap_or(1.0);
    let pan = reaper_route
        .pan()
        .map(|p| p.reaper_value().get())
        .unwrap_or(0.0);

    // Get state
    let muted = reaper_route.is_muted().unwrap_or(false);
    let mono = reaper_route.is_mono();
    let phase_inverted = reaper_route.phase_is_inverted();

    // Get automation mode
    let automation_mode = match reaper_route.automation_mode() {
        reaper_medium::AutomationMode::TrimRead => AutomationMode::TrimRead,
        reaper_medium::AutomationMode::Read => AutomationMode::Read,
        reaper_medium::AutomationMode::Touch => AutomationMode::Touch,
        reaper_medium::AutomationMode::Write => AutomationMode::Write,
        reaper_medium::AutomationMode::Latch => AutomationMode::Latch,
        _ => AutomationMode::TrimRead,
    };
    let midi_channel_mapping = read_midi_channel_mapping(reaper_route, route_type);

    TrackRoute {
        index,
        route_type,
        source_track_guid,
        dest_track_guid,
        dest_track_name,
        hw_output_index,
        hw_output_name,
        volume,
        pan,
        muted,
        mono,
        phase_inverted,
        send_mode: read_send_mode(reaper_route, route_type),
        automation_mode,
        source_channels: ChannelMapping::default(),
        dest_channels: ChannelMapping::default(),
        midi_channel_mapping,
    }
}

/// Read the send mode from a REAPER track route via medium-level API.
///
/// REAPER send mode values: 0 = post-fader, 1 = pre-FX, 3 = post-FX (modern).
fn read_send_mode(reaper_route: &ReaperTrackRoute, route_type: RouteType) -> SendMode {
    let track = reaper_route.track();

    // Determine the REAPER category and index for this route
    let (category, cat_index) = match route_type {
        RouteType::Send => {
            let hw_count = track.typed_send_count(SendPartnerType::HardwareOutput);
            let route_idx = reaper_route.index();
            if route_idx < hw_count {
                (TrackSendCategory::HardwareOutput, route_idx)
            } else {
                (TrackSendCategory::Send, route_idx - hw_count)
            }
        }
        RouteType::Receive => (TrackSendCategory::Receive, reaper_route.index()),
        RouteType::HardwareOutput => (TrackSendCategory::HardwareOutput, reaper_route.index()),
    };

    let Ok(media_track) = track.raw() else {
        return SendMode::PostFader;
    };

    let raw_mode = routing_sw::get_track_send_info_value(
        Reaper::get().medium_reaper(),
        media_track,
        category,
        cat_index,
        TrackSendAttributeKey::SendMode,
    ) as i32;

    match raw_mode {
        1 => SendMode::PreFx,
        3 => SendMode::PostFx,
        _ => SendMode::PostFader, // 0 and any unknown
    }
}

fn read_midi_channel_mapping(
    reaper_route: &ReaperTrackRoute,
    route_type: RouteType,
) -> Option<MidiChannelMapping> {
    let track = reaper_route.track();
    let (category, cat_index) = match route_type {
        RouteType::Send => {
            let hw_count = track.typed_send_count(SendPartnerType::HardwareOutput);
            let route_idx = reaper_route.index();
            if route_idx < hw_count {
                return None;
            }
            (TrackSendCategory::Send, route_idx - hw_count)
        }
        RouteType::Receive => (TrackSendCategory::Receive, reaper_route.index()),
        RouteType::HardwareOutput => return None,
    };

    let Ok(media_track) = track.raw() else {
        return None;
    };

    let raw_flags = routing_sw::get_track_send_info_value(
        Reaper::get().medium_reaper(),
        media_track,
        category,
        cat_index,
        TrackSendAttributeKey::MidiFlags,
    ) as i32;

    Some(parse_midi_channel_mapping(raw_flags))
}

fn parse_midi_channel_mapping(raw_flags: i32) -> MidiChannelMapping {
    let src_bits = raw_flags & 0x1f;
    let dst_bits = (raw_flags >> 5) & 0x1f;

    let source = match src_bits {
        1..=16 => MidiSourceChannel::Channel(src_bits as u8),
        _ => MidiSourceChannel::All,
    };
    let destination = match dst_bits {
        1..=16 => MidiDestinationChannel::Channel(dst_bits as u8),
        _ => MidiDestinationChannel::Original,
    };

    MidiChannelMapping {
        source,
        destination,
    }
}

/// Convert daw-proto SendMode to REAPER send mode value
fn send_mode_to_raw(mode: SendMode) -> i32 {
    match mode {
        SendMode::PostFader => 0,
        SendMode::PreFx => 1,
        SendMode::PostFx => 3,
    }
}

// ============================================================================
// RoutingService Implementation
// ============================================================================

impl RoutingService for ReaperRouting {
    // === Queries ===

    async fn get_sends(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute> {
        debug!("ReaperRouting::get_sends({:?}, {:?})", project, track);

        main_thread::query(move || {
            let Some(proj) = resolve_project(&project) else {
                warn!("Project not found");
                return vec![];
            };
            let Some(reaper_track) = resolve_track(&proj, &track) else {
                warn!("Track not found");
                return vec![];
            };

            reaper_track
                .typed_sends(SendPartnerType::Track)
                .enumerate()
                .map(|(i, route)| convert_track_route(&route, RouteType::Send, i as u32))
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn get_receives(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute> {
        debug!("ReaperRouting::get_receives({:?}, {:?})", project, track);

        main_thread::query(move || {
            let Some(proj) = resolve_project(&project) else {
                return vec![];
            };
            let Some(reaper_track) = resolve_track(&proj, &track) else {
                return vec![];
            };

            reaper_track
                .receives()
                .enumerate()
                .map(|(i, route)| convert_track_route(&route, RouteType::Receive, i as u32))
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn get_hardware_outputs(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Vec<TrackRoute> {
        debug!(
            "ReaperRouting::get_hardware_outputs({:?}, {:?})",
            project, track
        );

        main_thread::query(move || {
            let Some(proj) = resolve_project(&project) else {
                return vec![];
            };
            let Some(reaper_track) = resolve_track(&proj, &track) else {
                return vec![];
            };

            reaper_track
                .typed_sends(SendPartnerType::HardwareOutput)
                .enumerate()
                .map(|(i, route)| convert_track_route(&route, RouteType::HardwareOutput, i as u32))
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn get_route(
        &self,
        project: ProjectContext,
        location: RouteLocation,
    ) -> Option<TrackRoute> {
        debug!("ReaperRouting::get_route({:?}, {:?})", project, location);

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let reaper_track = resolve_track(&proj, &location.track)?;

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_dest) => {
                    // TODO: Search by destination
                    warn!("ByDestination lookup not yet implemented");
                    return None;
                }
            };

            let reaper_route = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    reaper_track.send_by_index(hw_count + route_index)?
                }
                RouteType::Receive => reaper_track.receive_by_index(route_index)?,
                RouteType::HardwareOutput => reaper_track
                    .typed_send_by_index(SendPartnerType::HardwareOutput, route_index)?,
            };

            Some(convert_track_route(
                &reaper_route,
                location.route_type,
                route_index,
            ))
        })
        .await
        .flatten()
    }

    // === CRUD ===

    async fn add_send(
        &self,
        project: ProjectContext,
        source: TrackRef,
        dest: TrackRef,
    ) -> Option<u32> {
        debug!(
            "ReaperRouting::add_send({:?}, {:?} -> {:?})",
            project, source, dest
        );

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let source_track = resolve_track(&proj, &source)?;
            let dest_track = resolve_track(&proj, &dest)?;

            let route = source_track.add_send_to(&dest_track);
            // Return the track-send index (not including hardware outputs)
            route.track_route_index()
        })
        .await
        .flatten()
    }

    async fn add_hardware_output(
        &self,
        project: ProjectContext,
        track: TrackRef,
        hw_output: u32,
    ) -> Option<u32> {
        debug!(
            "ReaperRouting::add_hardware_output({:?}, {:?}, hw={})",
            project, track, hw_output
        );

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let reaper_track = resolve_track(&proj, &track)?;
            let raw_track = reaper_track.raw().ok()?;

            // CreateTrackSend with HardwareOutput target creates a hardware output
            let medium = Reaper::get().medium_reaper();
            match routing_sw::create_track_send(medium, raw_track, SendTarget::HardwareOutput) {
                Ok(index) => {
                    // Set the destination channel for the hardware output
                    // hw_output is typically the stereo pair index (0 = 1-2, 1 = 3-4, etc.)
                    let dst_chan = (hw_output * 2) as f64;
                    routing_sw::set_track_send_info_value(
                        medium,
                        raw_track,
                        TrackSendCategory::HardwareOutput,
                        index,
                        TrackSendAttributeKey::DstChan,
                        dst_chan,
                    );
                    Some(index)
                }
                Err(e) => {
                    warn!("Failed to create hardware output: {:?}", e);
                    None
                }
            }
        })
        .await
        .flatten()
    }

    async fn remove_route(&self, project: ProjectContext, location: RouteLocation) {
        debug!("ReaperRouting::remove_route({:?}, {:?})", project, location);

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &location.track) else {
                return;
            };
            let Some(raw_track) = reaper_track.raw().ok() else {
                return;
            };

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => {
                    warn!("ByDestination removal not yet implemented");
                    return;
                }
            };

            let (category, actual_index) = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    (
                        TrackSendCategory::Send,
                        route_index - hw_count.min(route_index),
                    )
                }
                RouteType::Receive => (TrackSendCategory::Receive, route_index),
                RouteType::HardwareOutput => (TrackSendCategory::HardwareOutput, route_index),
            };

            routing_sw::remove_track_send(
                Reaper::get().medium_reaper(),
                raw_track,
                category,
                actual_index,
            );
        });
    }

    // === Levels ===

    async fn set_volume(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        volume: f64,
    ) {
        debug!(
            "ReaperRouting::set_volume({:?}, {:?}, {})",
            project, location, volume
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &location.track) else {
                return;
            };

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => return,
            };

            let reaper_route = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    let Some(r) = reaper_track.send_by_index(hw_count + route_index) else {
                        return;
                    };
                    r
                }
                RouteType::Receive => {
                    let Some(r) = reaper_track.receive_by_index(route_index) else {
                        return;
                    };
                    r
                }
                RouteType::HardwareOutput => {
                    let Some(r) = reaper_track
                        .typed_send_by_index(SendPartnerType::HardwareOutput, route_index)
                    else {
                        return;
                    };
                    r
                }
            };

            if let Ok(volume_value) = ReaperVolumeValue::new(volume) {
                let _ = reaper_route.set_volume(volume_value, EditMode::NormalTweak);
            }
        });
    }

    async fn set_pan(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        pan: f64,
    ) {
        debug!(
            "ReaperRouting::set_pan({:?}, {:?}, {})",
            project, location, pan
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &location.track) else {
                return;
            };

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => return,
            };

            let reaper_route = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    let Some(r) = reaper_track.send_by_index(hw_count + route_index) else {
                        return;
                    };
                    r
                }
                RouteType::Receive => {
                    let Some(r) = reaper_track.receive_by_index(route_index) else {
                        return;
                    };
                    r
                }
                RouteType::HardwareOutput => {
                    let Some(r) = reaper_track
                        .typed_send_by_index(SendPartnerType::HardwareOutput, route_index)
                    else {
                        return;
                    };
                    r
                }
            };

            let pan_obj = reaper_high::Pan::from_normalized_value(pan);
            let _ = reaper_route.set_pan(pan_obj, EditMode::NormalTweak);
        });
    }

    // === State ===

    async fn set_muted(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        muted: bool,
    ) {
        debug!(
            "ReaperRouting::set_muted({:?}, {:?}, {})",
            project, location, muted
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &location.track) else {
                return;
            };

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => return,
            };

            let reaper_route = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    let Some(r) = reaper_track.send_by_index(hw_count + route_index) else {
                        return;
                    };
                    r
                }
                RouteType::Receive => {
                    let Some(r) = reaper_track.receive_by_index(route_index) else {
                        return;
                    };
                    r
                }
                RouteType::HardwareOutput => {
                    let Some(r) = reaper_track
                        .typed_send_by_index(SendPartnerType::HardwareOutput, route_index)
                    else {
                        return;
                    };
                    r
                }
            };

            if muted {
                let _ = reaper_route.mute();
            } else {
                let _ = reaper_route.unmute();
            }
        });
    }

    async fn set_mono(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        mono: bool,
    ) {
        debug!(
            "ReaperRouting::set_mono({:?}, {:?}, {})",
            project, location, mono
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &location.track) else {
                return;
            };

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => return,
            };

            let reaper_route = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    let Some(r) = reaper_track.send_by_index(hw_count + route_index) else {
                        return;
                    };
                    r
                }
                RouteType::Receive => {
                    let Some(r) = reaper_track.receive_by_index(route_index) else {
                        return;
                    };
                    r
                }
                RouteType::HardwareOutput => {
                    let Some(r) = reaper_track
                        .typed_send_by_index(SendPartnerType::HardwareOutput, route_index)
                    else {
                        return;
                    };
                    r
                }
            };

            let _ = reaper_route.set_mono(mono);
        });
    }

    async fn set_phase(
        &self,
        project: ProjectContext,
        location: RouteLocation,
        inverted: bool,
    ) {
        debug!(
            "ReaperRouting::set_phase({:?}, {:?}, {})",
            project, location, inverted
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &location.track) else {
                return;
            };

            let route_index = match &location.route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => return,
            };

            let reaper_route = match location.route_type {
                RouteType::Send => {
                    let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
                    let Some(r) = reaper_track.send_by_index(hw_count + route_index) else {
                        return;
                    };
                    r
                }
                RouteType::Receive => {
                    let Some(r) = reaper_track.receive_by_index(route_index) else {
                        return;
                    };
                    r
                }
                RouteType::HardwareOutput => {
                    let Some(r) = reaper_track
                        .typed_send_by_index(SendPartnerType::HardwareOutput, route_index)
                    else {
                        return;
                    };
                    r
                }
            };

            let _ = reaper_route.set_phase_inverted(inverted);
        });
    }

    // === Mode ===

    async fn set_send_mode(
        &self,
        project: ProjectContext,
        track: TrackRef,
        route: RouteRef,
        mode: SendMode,
    ) {
        debug!(
            "ReaperRouting::set_send_mode({:?}, {:?}, {:?}, {:?})",
            project, track, route, mode
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &track) else {
                return;
            };
            let Some(raw_track) = reaper_track.raw().ok() else {
                return;
            };

            let route_index = match &route {
                RouteRef::Index(idx) => *idx,
                RouteRef::ByDestination(_) => return,
            };

            let hw_count = reaper_track.typed_send_count(SendPartnerType::HardwareOutput);
            let (category, actual_index) = if route_index < hw_count {
                (TrackSendCategory::HardwareOutput, route_index)
            } else {
                (TrackSendCategory::Send, route_index - hw_count)
            };

            routing_sw::set_track_send_info_value(
                Reaper::get().medium_reaper(),
                raw_track,
                category,
                actual_index,
                TrackSendAttributeKey::SendMode,
                send_mode_to_raw(mode) as f64,
            );
        });
    }

    // === Parent Send (Folder routing) ===

    async fn get_parent_send_enabled(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> bool {
        debug!(
            "ReaperRouting::get_parent_send_enabled({:?}, {:?})",
            project, track
        );

        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let reaper_track = resolve_track(&proj, &track)?;
            let raw_track = reaper_track.raw().ok()?;

            // B_MAINSEND: true if track sends to parent (or master if no parent)
            let value = routing_sw::get_media_track_info_value(
                Reaper::get().medium_reaper(),
                raw_track,
                reaper_medium::TrackAttributeKey::MainSend,
            );
            Some(value > 0.0)
        })
        .await
        .flatten()
        .unwrap_or(true)
    }

    async fn set_parent_send_enabled(
        &self,
        project: ProjectContext,
        track: TrackRef,
        enabled: bool,
    ) {
        debug!(
            "ReaperRouting::set_parent_send_enabled({:?}, {:?}, {})",
            project, track, enabled
        );

        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(reaper_track) = resolve_track(&proj, &track) else {
                return;
            };
            let Some(raw_track) = reaper_track.raw().ok() else {
                return;
            };

            routing_sw::set_media_track_info_value(
                Reaper::get().medium_reaper(),
                raw_track,
                reaper_medium::TrackAttributeKey::MainSend,
                if enabled { 1.0 } else { 0.0 },
            );
        });
    }

    // === Subscriptions ===

    async fn subscribe_routing(&self, project: ProjectContext, tx: Tx<RoutingEvent>) {
        info!(
            "ReaperRouting: subscribe_routing for {:?} - subscribing to broadcast channel",
            project
        );

        // Determine the project GUID to filter events
        let filter_guid = match &project {
            ProjectContext::Current => {
                // Resolve current project GUID on the main thread
                main_thread::query(|| {
                    let reaper = Reaper::get();
                    project_guid(&reaper.current_project())
                })
                .await
            }
            ProjectContext::Project(guid) => Some(guid.clone()),
        };

        let Some(filter_guid) = filter_guid else {
            info!("ReaperRouting: could not resolve project GUID for subscription");
            return;
        };

        let Some(mut rx) = routing_receiver() else {
            info!("ReaperRouting: routing broadcaster not initialized, subscriber will not receive events");
            return;
        };

        // Spawn the forwarding loop that filters events for this project
        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Filter events to only forward those for the subscribed project
                        let event_project_guid = match &event {
                            RoutingEvent::RouteCreated { project_guid, .. } => project_guid,
                            RoutingEvent::RouteDeleted { project_guid, .. } => project_guid,
                            RoutingEvent::VolumeChanged { project_guid, .. } => project_guid,
                            RoutingEvent::PanChanged { project_guid, .. } => project_guid,
                            RoutingEvent::MuteChanged { project_guid, .. } => project_guid,
                            RoutingEvent::ParentSendChanged { project_guid, .. } => project_guid,
                        };

                        if event_project_guid != &filter_guid {
                            continue;
                        }

                        if let Err(e) = tx.send(event).await {
                            debug!("ReaperRouting: subscribe_routing stream closed: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!(
                            "ReaperRouting: subscribe_routing lagged by {} messages",
                            count
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("ReaperRouting: routing broadcast channel closed");
                        break;
                    }
                }
            }

            info!("ReaperRouting: subscribe_routing stream ended");
        });
    }
}

// ============================================================================
// Additional Helper Functions for FTS-ROUTING Integration
// ============================================================================

impl ReaperRouting {
    /// Find a track by name in a specific named project (e.g., "FTS-ROUTING")
    ///
    /// This is useful for routing to utility projects that may not be the current project.
    pub async fn find_track_in_named_project(
        &self,
        project_name: &str,
        track_name: &str,
    ) -> Option<(String, String)> {
        let project_name = project_name.to_string();
        let track_name = track_name.to_string();

        main_thread::query(move || {
            let project = find_project_by_name(&project_name)?;
            let track = find_track_by_name(&project, &track_name)?;

            // Return project GUID and track GUID
            let project_path = project.file().map(|p| p.to_string()).unwrap_or_default();
            let project_guid = format!("{:x}", {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                project_path.hash(&mut hasher);
                hasher.finish()
            });
            let track_guid = track.guid().to_string_without_braces();

            Some((project_guid, track_guid))
        })
        .await
        .flatten()
    }

    /// Route a track's hardware output to a specific stereo pair
    ///
    /// This is a convenience method that:
    /// 1. Disables the parent/master send
    /// 2. Adds a hardware output to the specified stereo pair
    pub async fn route_to_hardware_output(
        &self,
        project: ProjectContext,
        track: TrackRef,
        stereo_pair: u32,
    ) -> bool {
        // First disable parent send
        self.set_parent_send_enabled(project.clone(), track.clone(), false)
            .await;

        // Then add hardware output
        self.add_hardware_output(project, track, stereo_pair)
            .await
            .is_some()
    }
}
