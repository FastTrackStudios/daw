//! REAPER Track Service Implementation
//!
//! Implements TrackService for REAPER by dispatching operations to the main thread
//! using TaskSupport from reaper-high. Follows the same pattern as ReaperFx and
//! ReaperTransport.

use daw_proto::{
    InputMonitoringMode, ProjectContext, RecordInput, Track, TrackEvent, TrackExtStateRequest,
    TrackRef, TrackService,
};
use reaper_high::{GroupingBehavior, Project, Reaper};
use reaper_medium::{GangBehavior, ProjectRef};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use vox::Tx;

use crate::main_thread;
use crate::project_context::{find_project_by_guid, project_guid};
use crate::safe_wrappers::routing as routing_sw;

// =============================================================================
// Track Change Detection (Broadcaster + Cache + Poll)
// =============================================================================

/// Global track event broadcaster
static TRACK_BROADCASTER: OnceLock<broadcast::Sender<TrackEvent>> = OnceLock::new();

/// Cached per-project track states for change detection.
/// Key is project GUID, value is the list of cached track states for that project.
static TRACK_CACHE: OnceLock<Mutex<HashMap<String, Vec<CachedTrackState>>>> = OnceLock::new();

/// Cached track state for change detection.
/// Mirrors the fields we want to diff between polls.
#[derive(Clone, Debug)]
struct CachedTrackState {
    guid: String,
    name: String,
    muted: bool,
    soloed: bool,
    armed: bool,
    selected: bool,
    volume: f64,
    pan: f64,
    color: Option<u32>,
    visible_in_tcp: bool,
    visible_in_mixer: bool,
    index: u32,
    folder_depth: i32,
    fx_count: u32,
    input_fx_count: u32,
    is_folder: bool,
}

/// Build a CachedTrackState from a reaper-high Track.
fn build_cached_track_state(track: &reaper_high::Track) -> CachedTrackState {
    let guid = track.guid().to_string_without_braces();
    let index = track.index().unwrap_or(0);
    let name = track
        .name()
        .map(|n| n.to_str().to_string())
        .unwrap_or_else(|| {
            if track.is_master_track() {
                "MASTER".to_string()
            } else {
                format!("Track {}", index + 1)
            }
        });
    let color = track
        .custom_color()
        .map(|c| ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32));
    let volume = track.volume().get();
    let pan = track.pan().reaper_value().get();
    let muted = track.is_muted();
    let soloed = track.is_solo();
    let armed = track.is_armed(false);
    let selected = track.is_selected();
    let folder_depth = track.folder_depth_change();
    let is_folder = folder_depth > 0;
    let fx_count = track.normal_fx_chain().fx_count();
    let input_fx_count = track.input_fx_chain().fx_count();
    let visible_in_tcp = track.is_shown(reaper_medium::TrackArea::Tcp);
    let visible_in_mixer = track.is_shown(reaper_medium::TrackArea::Mcp);

    CachedTrackState {
        guid,
        name,
        muted,
        soloed,
        armed,
        selected,
        volume,
        pan,
        color,
        visible_in_tcp,
        visible_in_mixer,
        index,
        folder_depth,
        fx_count,
        input_fx_count,
        is_folder,
    }
}

/// Convert a CachedTrackState into a daw_proto::Track for Added events.
fn cached_to_track(cached: &CachedTrackState) -> Track {
    Track {
        guid: cached.guid.clone(),
        index: cached.index,
        name: cached.name.clone(),
        color: cached.color,
        muted: cached.muted,
        soloed: cached.soloed,
        armed: cached.armed,
        selected: cached.selected,
        volume: cached.volume,
        pan: cached.pan,
        parent_guid: None, // parent_guid is assigned via post-processing
        folder_depth: cached.folder_depth,
        is_folder: cached.is_folder,
        visible_in_tcp: cached.visible_in_tcp,
        visible_in_mixer: cached.visible_in_mixer,
        fx_count: cached.fx_count,
        input_fx_count: cached.input_fx_count,
    }
}

/// Initialize the track broadcaster.
/// Called by the extension during initialization.
pub fn init_track_broadcaster() {
    let (tx, _rx) = broadcast::channel::<TrackEvent>(256);
    let _ = TRACK_BROADCASTER.set(tx);
    let _ = TRACK_CACHE.set(Mutex::new(HashMap::new()));
}

/// Get a receiver for track events.
fn track_receiver() -> Option<broadcast::Receiver<TrackEvent>> {
    TRACK_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Threshold for volume/pan change detection
const VOLUME_PAN_THRESHOLD: f64 = 0.0001;

/// Poll REAPER track state for ALL open projects and broadcast changes.
/// **MUST be called from the main thread** (e.g., from timer callback).
///
/// Iterates all open projects, reads track state, diffs against cache,
/// and emits granular TrackEvent variants for each change.
pub fn poll_and_broadcast_tracks() {
    let Some(tx) = TRACK_BROADCASTER.get() else {
        return;
    };
    if tx.receiver_count() == 0 {
        return;
    }

    let Some(cache) = TRACK_CACHE.get() else {
        return;
    };
    let mut cache_guard = cache.lock().unwrap();

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    // Track which project GUIDs we've seen this poll (for cleanup)
    let mut seen_guids = Vec::new();

    // Iterate through all open projects
    for tab_index in 0..128u32 {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            break;
        };

        let project = Project::new(result.project);
        let proj_guid = project_guid(&project);
        seen_guids.push(proj_guid.clone());

        // Build current track states for this project
        let current: Vec<CachedTrackState> = project
            .tracks()
            .map(|t| build_cached_track_state(&t))
            .collect();

        // Get previous cached state
        let previous = cache_guard.get(&proj_guid);

        // Diff and emit events
        diff_and_emit(tx, previous.map(|v| v.as_slice()).unwrap_or(&[]), &current);

        // Update cache
        cache_guard.insert(proj_guid, current);
    }

    // Clean up cache entries for projects that are no longer open
    cache_guard.retain(|guid, _| seen_guids.contains(guid));
}

/// Diff previous and current track states and emit TrackEvent variants.
fn diff_and_emit(
    tx: &broadcast::Sender<TrackEvent>,
    previous: &[CachedTrackState],
    current: &[CachedTrackState],
) {
    // Build lookup of previous tracks by GUID
    let prev_map: HashMap<&str, &CachedTrackState> =
        previous.iter().map(|t| (t.guid.as_str(), t)).collect();

    let curr_map: HashMap<&str, &CachedTrackState> =
        current.iter().map(|t| (t.guid.as_str(), t)).collect();

    // Check for added tracks (in current but not in previous)
    for curr in current {
        if !prev_map.contains_key(curr.guid.as_str()) {
            let _ = tx.send(TrackEvent::Added(cached_to_track(curr)));
        }
    }

    // Check for removed tracks (in previous but not in current)
    for prev in previous {
        if !curr_map.contains_key(prev.guid.as_str()) {
            let _ = tx.send(TrackEvent::Removed(prev.guid.clone()));
        }
    }

    // Check for changed tracks (in both previous and current)
    for curr in current {
        let Some(prev) = prev_map.get(curr.guid.as_str()) else {
            continue; // Already handled as Added
        };

        let guid = &curr.guid;

        if prev.name != curr.name {
            let _ = tx.send(TrackEvent::Renamed {
                guid: guid.clone(),
                name: curr.name.clone(),
            });
        }
        if prev.muted != curr.muted {
            let _ = tx.send(TrackEvent::MuteChanged {
                guid: guid.clone(),
                muted: curr.muted,
            });
        }
        if prev.soloed != curr.soloed {
            let _ = tx.send(TrackEvent::SoloChanged {
                guid: guid.clone(),
                soloed: curr.soloed,
            });
        }
        if prev.armed != curr.armed {
            let _ = tx.send(TrackEvent::ArmChanged {
                guid: guid.clone(),
                armed: curr.armed,
            });
        }
        if prev.selected != curr.selected {
            let _ = tx.send(TrackEvent::SelectionChanged {
                guid: guid.clone(),
                selected: curr.selected,
            });
        }
        if (prev.volume - curr.volume).abs() > VOLUME_PAN_THRESHOLD {
            let _ = tx.send(TrackEvent::VolumeChanged {
                guid: guid.clone(),
                volume: curr.volume,
            });
        }
        if (prev.pan - curr.pan).abs() > VOLUME_PAN_THRESHOLD {
            let _ = tx.send(TrackEvent::PanChanged {
                guid: guid.clone(),
                pan: curr.pan,
            });
        }
        if prev.color != curr.color {
            let _ = tx.send(TrackEvent::ColorChanged {
                guid: guid.clone(),
                color: curr.color,
            });
        }
        if prev.visible_in_tcp != curr.visible_in_tcp {
            let _ = tx.send(TrackEvent::TcpVisibilityChanged {
                guid: guid.clone(),
                visible: curr.visible_in_tcp,
            });
        }
        if prev.visible_in_mixer != curr.visible_in_mixer {
            let _ = tx.send(TrackEvent::MixerVisibilityChanged {
                guid: guid.clone(),
                visible: curr.visible_in_mixer,
            });
        }
        if prev.index != curr.index {
            let _ = tx.send(TrackEvent::Moved {
                guid: guid.clone(),
                old_index: prev.index,
                new_index: curr.index,
            });
        }
    }
}

/// REAPER track service implementation.
///
/// Zero-field struct — all state lives in REAPER itself. Queries dispatch to
/// the main thread via `main_thread_future()`, mutations via
/// `do_later_in_main_thread_asap()`.
#[derive(Clone)]
pub struct ReaperTrack;

impl ReaperTrack {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperTrack {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Resolve a ProjectContext to a REAPER Project
pub(crate) fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

/// Resolve a TrackRef to a reaper-high Track within a project.
///
/// After resolving the track, validates that the raw MediaTrack pointer is
/// still recognized by REAPER. This guards against stale pointers if a track
/// was deleted between resolve and use.
///
/// Public alias for use from other daw-reaper modules (e.g. midi.rs).
pub fn resolve_track_pub(
    project: &reaper_high::Project,
    track_ref: &TrackRef,
) -> Option<reaper_high::Track> {
    resolve_track(project, track_ref)
}

pub(crate) fn resolve_track(
    project: &reaper_high::Project,
    track_ref: &TrackRef,
) -> Option<reaper_high::Track> {
    let track = match track_ref {
        TrackRef::Guid(guid) => {
            // Linear scan to match GUID string
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
        TrackRef::Index(idx) => project.track_by_index(*idx)?,
        TrackRef::Master => project.master_track().ok()?,
    };
    // Validate the pointer is still live
    if !main_thread::is_track_valid(project, &track) {
        return None;
    }
    Some(track)
}

/// Convert a reaper-high Track to our daw_proto::Track
fn build_track_info(track: &reaper_high::Track) -> Track {
    let guid = track.guid().to_string_without_braces();
    let index = track.index().unwrap_or(0);
    let name = track
        .name()
        .map(|n| n.to_str().to_string())
        .unwrap_or_else(|| {
            if track.is_master_track() {
                "MASTER".to_string()
            } else {
                format!("Track {}", index + 1)
            }
        });

    // Color: RgbColor { r, g, b } → 0xRRGGBB
    let color = track
        .custom_color()
        .map(|c| ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32));

    // Volume: ReaperVolumeValue → f64 (linear, 0.0 = -inf, 1.0 = 0dB)
    let volume = track.volume().get();

    // Pan: Pan wraps normalized 0.0-1.0, convert back to -1.0..1.0
    let pan = track.pan().reaper_value().get();

    let muted = track.is_muted();
    let soloed = track.is_solo();
    let armed = track.is_armed(false);
    let selected = track.is_selected();

    // Folder depth: positive = starts folder, negative = closes N levels
    let folder_depth = track.folder_depth_change();
    let is_folder = folder_depth > 0;

    // FX counts
    let fx_count = track.normal_fx_chain().fx_count();
    let input_fx_count = track.input_fx_chain().fx_count();

    // Parent GUID is computed in a post-processing pass over the full track list
    // (see `assign_parent_guids`). Set to None here; `get_tracks` fills it in.
    let parent_guid = None;

    // Visibility
    let visible_in_tcp = track.is_shown(reaper_medium::TrackArea::Tcp);
    let visible_in_mixer = track.is_shown(reaper_medium::TrackArea::Mcp);

    Track {
        guid,
        index,
        name,
        color,
        muted,
        soloed,
        armed,
        selected,
        volume,
        pan,
        parent_guid,
        folder_depth,
        is_folder,
        visible_in_tcp,
        visible_in_mixer,
        fx_count,
        input_fx_count,
    }
}

/// Walk the flat track list and assign `parent_guid` using folder depth changes.
///
/// REAPER's folder hierarchy is encoded as depth deltas on each track:
/// - `folder_depth > 0` (typically 1) means "this track starts a folder"
/// - `folder_depth < 0` means "close N folder levels after this track"
///
/// We maintain a stack of folder GUIDs. When we encounter a folder start,
/// we push its GUID. Children between a folder start and its close inherit
/// the top of the stack as their parent. Negative depth pops the stack.
fn assign_parent_guids(tracks: &mut [Track]) {
    let mut folder_stack: Vec<String> = Vec::new();

    for track in tracks.iter_mut() {
        // Current track's parent is whatever is on top of the stack
        track.parent_guid = folder_stack.last().cloned();

        let depth = track.folder_depth;
        if depth > 0 {
            // This track starts a folder — push it as the new parent
            folder_stack.push(track.guid.clone());
        } else if depth < 0 {
            // Close |depth| folder levels
            for _ in 0..depth.unsigned_abs() {
                folder_stack.pop();
            }
        }
    }
}

// =============================================================================
// Public sync helpers — callable directly from the main thread
// =============================================================================

/// Insert a track in the current project, returning its GUID string.
///
/// Must be called from the main thread.
pub fn add_track_on_main_thread(name: &str, at_index: Option<u32>) -> Option<String> {
    let proj = Reaper::get().current_project();
    let index = at_index.unwrap_or_else(|| proj.track_count());
    let new_track = proj.insert_track_at(index).ok()?;
    new_track.set_name(name);
    Some(new_track.guid().to_string_without_braces())
}

/// Set the folder depth on a track identified by its GUID.
///
/// Must be called from the main thread.
pub fn set_folder_depth_on_main_thread(track_guid: &str, depth: i32) -> Result<(), String> {
    let proj = Reaper::get().current_project();
    let track = resolve_track(&proj, &TrackRef::Guid(track_guid.to_string()))
        .ok_or_else(|| format!("Track not found: {track_guid}"))?;
    let raw = track.raw().map_err(|e| format!("raw track failed: {e}"))?;
    routing_sw::set_media_track_info_value(
        Reaper::get().medium_reaper(),
        raw,
        reaper_medium::TrackAttributeKey::FolderDepth,
        depth as f64,
    );
    Ok(())
}

// =============================================================================
// Bulk Hierarchy Application (single main-thread tick)
// =============================================================================

/// Apply a TrackHierarchy to the current project in one shot.
///
/// The hierarchy contains folder/group nodes whose `items` field lists the
/// original track names that belong under them. This function:
///
/// 1. Builds a flat target track list by expanding hierarchy nodes:
///    - Folder nodes with children → new empty folder track (structural)
///    - Nodes with items → each item maps to an existing track (by name)
///    - Empty leaf nodes → new empty track
///
/// 2. Matches items to existing tracks by name, preserving their FX,
///    routing, envelopes, media items, etc.
///
/// 3. Reorders matched tracks + inserts new folder tracks so the final
///    track order matches the hierarchy. All in one main-thread tick.
///
/// 4. Sets folder depths and colors on every track.
fn apply_hierarchy_on_main_thread(
    proj: &Project,
    hierarchy: &daw_proto::TrackHierarchy,
) -> Result<(), String> {
    use std::collections::HashSet;
    use std::time::Instant;

    let t0 = Instant::now();

    if hierarchy.tracks.is_empty() {
        return Ok(());
    }

    // Step 1: Build a name→GUID lookup of all existing tracks.
    // If multiple tracks share a name, we'll claim them FIFO.
    let mut existing_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..proj.track_count() {
        if let Some(t) = proj.track_by_index(i) {
            let name = t.name().map(|n| n.to_str().to_string()).unwrap_or_default();
            existing_by_name
                .entry(name)
                .or_default()
                .push(t.guid().to_string_without_braces());
        }
    }

    // Track which GUIDs we've already claimed
    let mut claimed_guids: HashSet<String> = HashSet::new();

    /// Claim the first unclaimed GUID for a given track name
    fn claim_guid(
        existing_by_name: &HashMap<String, Vec<String>>,
        claimed: &mut HashSet<String>,
        name: &str,
    ) -> Option<String> {
        if let Some(guids) = existing_by_name.get(name) {
            for guid in guids {
                if !claimed.contains(guid) {
                    claimed.insert(guid.clone());
                    return Some(guid.clone());
                }
            }
        }
        None
    }

    // Step 2: Build the flat target list.
    //
    // Each entry is (guid, folder_depth, color):
    // - guid: existing track GUID (reused) or newly created track GUID
    // - folder_depth: the raw folder depth value for this position
    // - color: optional color to apply
    struct TargetTrack {
        guid: String,
        folder_depth: i32,
        color: Option<u32>,
    }
    let mut targets: Vec<TargetTrack> = Vec::new();

    for node in &hierarchy.tracks {
        if node.is_folder || node.items.is_empty() {
            // Folder track or empty leaf — try to match by node name first,
            // otherwise create a new structural track
            let guid = claim_guid(&existing_by_name, &mut claimed_guids, &node.name)
                .unwrap_or_else(|| {
                    let idx = proj.track_count();
                    let new_track = proj.insert_track_at(idx).expect("insert_track_at failed");
                    new_track.set_name(node.name.as_str());
                    let guid = new_track.guid().to_string_without_braces();
                    claimed_guids.insert(guid.clone());
                    guid
                });
            targets.push(TargetTrack {
                guid,
                folder_depth: node.folder_depth_change.to_raw_value(),
                color: node.color,
            });
        }

        // Expand items: each item maps to an existing track by its original name.
        // Items appear AFTER the node they belong to (under the folder).
        if !node.items.is_empty() {
            let is_also_folder = node.is_folder;
            let num_items = node.items.len();
            for (item_idx, item_name) in node.items.iter().enumerate() {
                let guid = claim_guid(&existing_by_name, &mut claimed_guids, item_name)
                    .unwrap_or_else(|| {
                        // Item track doesn't exist — create it
                        let idx = proj.track_count();
                        let new_track = proj.insert_track_at(idx).expect("insert_track_at failed");
                        new_track.set_name(item_name.as_str());
                        let guid = new_track.guid().to_string_without_braces();
                        claimed_guids.insert(guid.clone());
                        guid
                    });

                // If this node is NOT a folder (leaf node with items), the folder
                // depth from the node applies to the LAST item, not the node itself.
                let folder_depth = if !is_also_folder && item_idx == num_items - 1 {
                    node.folder_depth_change.to_raw_value()
                } else {
                    0 // normal track
                };

                targets.push(TargetTrack {
                    guid,
                    folder_depth,
                    color: node.color,
                });
            }
        }
    }

    let t_build = t0.elapsed();

    // Step 3: Reorder tracks to match target order.
    // Process from top to bottom — move each track to its target index.
    let t1 = Instant::now();
    let mut moves = 0u32;
    for (target_idx, entry) in targets.iter().enumerate() {
        let target_idx = target_idx as u32;

        let current_idx = find_track_index_by_guid(proj, &entry.guid);
        let Some(current_idx) = current_idx else {
            continue;
        };

        if current_idx == target_idx {
            continue;
        }

        // Move via chunk save/remove/insert/restore
        let Some(track) = proj.track_by_index(current_idx) else {
            continue;
        };

        let chunk = track
            .chunk(1_000_000, reaper_medium::ChunkCacheHint::NormalMode)
            .map_err(|e| format!("get_chunk failed: {e}"))?;
        let chunk_str: String = chunk
            .try_into()
            .map_err(|_| "chunk to string conversion failed".to_string())?;

        proj.remove_track(&track);

        // After removal, indices shift
        let insert_idx = if target_idx > current_idx {
            target_idx.saturating_sub(1)
        } else {
            target_idx
        };

        proj.insert_track_at(insert_idx)
            .map_err(|e| format!("insert_track_at failed: {e}"))?;

        if let Some(new_track) = proj.track_by_index(insert_idx) {
            let chunk_obj = reaper_high::Chunk::new(chunk_str);
            new_track
                .set_chunk(chunk_obj)
                .map_err(|e| format!("set_chunk failed: {e}"))?;
        }
        moves += 1;
    }
    let t_reorder = t1.elapsed();

    // Step 4: Set folder depths and colors on all tracks
    let t2 = Instant::now();
    for (i, entry) in targets.iter().enumerate() {
        let Some(track) = proj.track_by_index(i as u32) else {
            continue;
        };

        // Set folder depth
        if let Ok(raw) = track.raw() {
            routing_sw::set_media_track_info_value(
                Reaper::get().medium_reaper(),
                raw,
                reaper_medium::TrackAttributeKey::FolderDepth,
                entry.folder_depth as f64,
            );
        }

        // Set color
        if let Some(color) = entry.color {
            let r = ((color >> 16) & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let b = (color & 0xFF) as u8;
            track.set_custom_color(Some(reaper_medium::RgbColor::rgb(r, g, b)));
        }
    }

    let t_props = t2.elapsed();

    eprintln!(
        "[daw-bridge] apply_hierarchy: {} targets ({} moves), build={:?} reorder={:?} props={:?} total={:?}",
        targets.len(),
        moves,
        t_build,
        t_reorder,
        t_props,
        t0.elapsed()
    );

    Ok(())
}

/// Find a track's current index by GUID.
fn find_track_index_by_guid(proj: &Project, guid: &str) -> Option<u32> {
    for i in 0..proj.track_count() {
        if let Some(t) = proj.track_by_index(i) {
            if t.guid().to_string_without_braces() == guid {
                return Some(i);
            }
        }
    }
    None
}

// =============================================================================
// TrackService Implementation
// =============================================================================

impl TrackService for ReaperTrack {
    // =========================================================================
    // Query Methods
    // =========================================================================

    async fn get_tracks(&self, project: ProjectContext) -> Vec<Track> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let mut tracks: Vec<Track> = proj.tracks().map(|t| build_track_info(&t)).collect();
            assign_parent_guids(&mut tracks);
            Some(tracks)
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn get_track(&self, project: ProjectContext, track: TrackRef) -> Option<Track> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let t = resolve_track(&proj, &track)?;
            Some(build_track_info(&t))
        })
        .await
        .flatten()
    }

    async fn track_count(&self, project: ProjectContext) -> u32 {
        main_thread::query(move || {
            resolve_project(&project)
                .map(|p| p.track_count())
                .unwrap_or(0)
        })
        .await
        .unwrap_or(0)
    }

    async fn get_selected_tracks(&self, project: ProjectContext) -> Vec<Track> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            Some(
                proj.tracks()
                    .filter(|t| t.is_selected())
                    .map(|t| build_track_info(&t))
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn get_master_track(&self, project: ProjectContext) -> Option<Track> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let master = proj.master_track().ok()?;
            Some(build_track_info(&master))
        })
        .await
        .flatten()
    }

    // =========================================================================
    // Mute/Solo/Arm
    // =========================================================================

    async fn set_muted(&self, project: ProjectContext, track: TrackRef, muted: bool) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            if muted {
                t.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            } else {
                t.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
        });
    }

    async fn set_soloed(&self, project: ProjectContext, track: TrackRef, soloed: bool) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            if soloed {
                t.solo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            } else {
                t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
        });
    }

    async fn set_solo_exclusive(&self, project: ProjectContext, track: TrackRef) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            for t in proj.tracks() {
                if t.is_solo() {
                    t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                }
            }
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.solo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        });
    }

    async fn clear_all_solo(&self, project: ProjectContext) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            for t in proj.tracks() {
                if t.is_solo() {
                    t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                }
            }
        });
    }

    async fn set_armed(&self, project: ProjectContext, track: TrackRef, armed: bool) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            if armed {
                t.arm(
                    false,
                    GangBehavior::DenyGang,
                    GroupingBehavior::PreventGrouping,
                );
            } else {
                t.disarm(
                    false,
                    GangBehavior::DenyGang,
                    GroupingBehavior::PreventGrouping,
                );
            }
        });
    }

    async fn set_input_monitoring(
        &self,
        project: ProjectContext,
        track: TrackRef,
        mode: InputMonitoringMode,
    ) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            let reaper_mode = match mode {
                InputMonitoringMode::Off => reaper_medium::InputMonitoringMode::Off,
                InputMonitoringMode::Normal => reaper_medium::InputMonitoringMode::Normal,
                InputMonitoringMode::NotWhenPlaying => {
                    reaper_medium::InputMonitoringMode::NotWhenPlaying
                }
            };
            t.set_input_monitoring_mode(
                reaper_mode,
                GangBehavior::DenyGang,
                GroupingBehavior::PreventGrouping,
            );
        });
    }

    async fn set_record_input(&self, project: ProjectContext, track: TrackRef, input: RecordInput) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            let reaper_input = match input {
                RecordInput::None => None,
                RecordInput::Midi { device_id, channel } => {
                    use reaper_medium::{MidiInputDeviceId, RecordingInput};
                    Some(RecordingInput::Midi {
                        device_id: device_id.map(MidiInputDeviceId::new),
                        channel: channel.and_then(|ch| ch.try_into().ok()),
                    })
                }
                RecordInput::Raw(raw) => reaper_medium::RecordingInput::from_raw(raw),
            };
            t.set_recording_input(reaper_input);
        });
    }

    // =========================================================================
    // Volume/Pan
    // =========================================================================

    async fn set_volume(&self, project: ProjectContext, track: TrackRef, volume: f64) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            let val = reaper_medium::ReaperVolumeValue::new(volume).expect("invalid volume value");
            let _ = t.set_volume_smart(val, Default::default());
        });
    }

    async fn set_pan(&self, project: ProjectContext, track: TrackRef, pan: f64) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            let val = reaper_medium::ReaperPanValue::new_panic(pan.clamp(-1.0, 1.0));
            let _ = t.set_pan_smart(val, Default::default());
        });
    }

    // =========================================================================
    // Selection
    // =========================================================================

    async fn set_selected(&self, project: ProjectContext, track: TrackRef, selected: bool) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            if selected {
                t.select();
            } else {
                t.unselect();
            }
        });
    }

    async fn select_exclusive(&self, project: ProjectContext, track: TrackRef) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.select_exclusively();
        });
    }

    async fn clear_selection(&self, project: ProjectContext) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            for t in proj.tracks() {
                if t.is_selected() {
                    t.unselect();
                }
            }
        });
    }

    // =========================================================================
    // Batch Operations
    // =========================================================================

    async fn mute_all(&self, project: ProjectContext) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            for t in proj.tracks() {
                if !t.is_muted() {
                    t.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                }
            }
        });
    }

    async fn unmute_all(&self, project: ProjectContext) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            for t in proj.tracks() {
                if t.is_muted() {
                    t.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                }
            }
        });
    }

    // =========================================================================
    // Track Management
    // =========================================================================

    async fn add_track(
        &self,
        project: ProjectContext,
        name: String,
        at_index: Option<u32>,
    ) -> String {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let index = at_index.unwrap_or_else(|| proj.track_count());
            let new_track = proj.insert_track_at(index).ok()?;
            new_track.set_name(name.as_str());
            Some(new_track.guid().to_string_without_braces())
        })
        .await
        .flatten()
        .unwrap_or_default()
    }

    async fn remove_track(&self, project: ProjectContext, track: TrackRef) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            proj.remove_track(&t);
        });
    }

    async fn rename_track(&self, project: ProjectContext, track: TrackRef, name: String) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.set_name(name.as_str());
        });
    }

    async fn set_track_color(&self, project: ProjectContext, track: TrackRef, color: u32) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            if color == 0 {
                t.set_custom_color(None);
            } else {
                let r = ((color >> 16) & 0xFF) as u8;
                let g = ((color >> 8) & 0xFF) as u8;
                let b = (color & 0xFF) as u8;
                t.set_custom_color(Some(reaper_medium::RgbColor::rgb(r, g, b)));
            }
        });
    }

    async fn set_visible_in_tcp(&self, project: ProjectContext, track: TrackRef, visible: bool) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.set_shown(reaper_medium::TrackArea::Tcp, visible);
        });
    }

    async fn set_visible_in_mixer(&self, project: ProjectContext, track: TrackRef, visible: bool) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.set_shown(reaper_medium::TrackArea::Mcp, visible);
        });
    }

    async fn set_track_chunk(
        &self,
        project: ProjectContext,
        track: TrackRef,
        chunk: String,
    ) -> Result<(), String> {
        main_thread::query(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let t = resolve_track(&proj, &track).ok_or_else(|| "Track not found".to_string())?;
            let chunk_obj = reaper_high::Chunk::new(chunk);
            t.set_chunk(chunk_obj)
                .map_err(|e| format!("set_chunk failed: {e}"))
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    async fn get_track_chunk(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Result<String, String> {
        main_thread::query(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let t = resolve_track(&proj, &track).ok_or_else(|| "Track not found".to_string())?;
            let chunk = t
                .chunk(1_000_000, reaper_medium::ChunkCacheHint::NormalMode)
                .map_err(|e| format!("get_chunk failed: {e}"))?;
            let s: String = chunk
                .try_into()
                .map_err(|_| "chunk to string conversion failed".to_string())?;
            Ok(s)
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    async fn set_folder_depth(
        &self,
        project: ProjectContext,
        track: TrackRef,
        depth: i32,
    ) -> Result<(), String> {
        main_thread::query(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let t = resolve_track(&proj, &track).ok_or_else(|| "Track not found".to_string())?;
            let raw = t.raw().map_err(|e| format!("raw track failed: {e}"))?;
            routing_sw::set_media_track_info_value(
                Reaper::get().medium_reaper(),
                raw,
                reaper_medium::TrackAttributeKey::FolderDepth,
                depth as f64,
            );
            Ok(())
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    async fn set_num_channels(
        &self,
        project: ProjectContext,
        track: TrackRef,
        num_channels: u32,
    ) -> Result<(), String> {
        main_thread::query(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let t = resolve_track(&proj, &track).ok_or_else(|| "Track not found".to_string())?;
            let raw = t.raw().map_err(|e| format!("raw track failed: {e}"))?;
            routing_sw::set_media_track_info_value(
                Reaper::get().medium_reaper(),
                raw,
                reaper_medium::TrackAttributeKey::Nchan,
                num_channels as f64,
            );
            Ok(())
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    async fn remove_all_tracks(&self, project: ProjectContext) -> Result<(), String> {
        main_thread::query(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let count = proj.track_count();
            for i in (0..count).rev() {
                if let Some(t) = proj.track_by_index(i) {
                    proj.remove_track(&t);
                }
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    async fn move_track(
        &self,
        project: ProjectContext,
        track: TrackRef,
        new_index: u32,
    ) -> Result<(), String> {
        main_thread::query(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let t = resolve_track(&proj, &track).ok_or_else(|| "Track not found".to_string())?;

            let current_index = t.index().ok_or_else(|| "Track has no index".to_string())?;
            if current_index == new_index {
                return Ok(());
            }

            // Capture the full track state chunk
            let chunk = t
                .chunk(1_000_000, reaper_medium::ChunkCacheHint::NormalMode)
                .map_err(|e| format!("get_chunk failed: {e}"))?;
            let chunk_str: String = chunk
                .try_into()
                .map_err(|_| "chunk to string conversion failed".to_string())?;

            // Remove the track
            proj.remove_track(&t);

            // Insert at the new position
            let insert_idx = if new_index > current_index {
                // After removal, indices shift down
                new_index.saturating_sub(1)
            } else {
                new_index
            };
            proj.insert_track_at(insert_idx)
                .map_err(|e| format!("insert_track_at failed: {e}"))?;

            // Apply the saved chunk to the newly inserted track
            if let Some(new_track) = proj.track_by_index(insert_idx) {
                let chunk_obj = reaper_high::Chunk::new(chunk_str);
                new_track
                    .set_chunk(chunk_obj)
                    .map_err(|e| format!("set_chunk failed: {e}"))?;
            }

            Ok(())
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    // =========================================================================
    // Bulk Operations
    // =========================================================================

    async fn apply_hierarchy(
        &self,
        project: ProjectContext,
        hierarchy: daw_proto::TrackHierarchy,
    ) -> Result<(), String> {
        let t0 = std::time::Instant::now();
        let result = main_thread::query_undoable(project, "FTS: Apply hierarchy", move |proj| {
            apply_hierarchy_on_main_thread(&proj, &hierarchy)
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()));
        eprintln!(
            "[daw-bridge] apply_hierarchy RPC round-trip: {:?}",
            t0.elapsed()
        );
        result
    }

    // =========================================================================
    // Track ExtState (P_EXT)
    // =========================================================================

    async fn get_ext_state(
        &self,
        project: ProjectContext,
        track: TrackRef,
        request: TrackExtStateRequest,
    ) -> Option<String> {
        main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let t = resolve_track(&proj, &track)?;
            let raw = t.raw().ok()?;
            let low = Reaper::get().medium_reaper().low();
            let attr = std::ffi::CString::new(format!("P_EXT:{}:{}", request.section, request.key))
                .ok()?;
            let mut buf = vec![0u8; 65536];
            let ok = unsafe {
                low.GetSetMediaTrackInfo_String(
                    raw.as_ptr(),
                    attr.as_ptr(),
                    buf.as_mut_ptr() as *mut i8,
                    false,
                )
            };
            if !ok {
                return None;
            }
            let val = crate::safe_wrappers::buffer::string_from_buffer(&buf);
            if val.is_empty() { None } else { Some(val) }
        })
        .await
        .flatten()
    }

    async fn set_ext_state(
        &self,
        project: ProjectContext,
        track: TrackRef,
        request: TrackExtStateRequest,
    ) {
        main_thread::run(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            let Ok(raw) = t.raw() else { return };
            let low = Reaper::get().medium_reaper().low();
            let attr = match std::ffi::CString::new(format!(
                "P_EXT:{}:{}",
                request.section, request.key
            )) {
                Ok(s) => s,
                Err(_) => return,
            };
            let val = match std::ffi::CString::new(request.value) {
                Ok(s) => s,
                Err(_) => return,
            };
            unsafe {
                low.GetSetMediaTrackInfo_String(
                    raw.as_ptr(),
                    attr.as_ptr(),
                    val.as_ptr() as *mut i8,
                    true,
                );
            }
        });
    }

    async fn delete_ext_state(
        &self,
        project: ProjectContext,
        track: TrackRef,
        request: TrackExtStateRequest,
    ) {
        self.set_ext_state(
            project,
            track,
            TrackExtStateRequest {
                section: request.section,
                key: request.key,
                value: String::new(),
            },
        )
        .await;
    }

    // =========================================================================
    // Subscriptions
    // =========================================================================

    async fn subscribe_tracks(&self, _project: ProjectContext, tx: Tx<TrackEvent>) {
        tracing::info!("ReaperTrack: subscribe_tracks - subscribing to broadcast channel");

        let Some(mut rx) = track_receiver() else {
            tracing::info!("ReaperTrack: track broadcast channel not initialized");
            return;
        };

        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = tx.send(event).await {
                            tracing::debug!("ReaperTrack: subscribe_tracks stream closed: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        tracing::debug!(
                            "ReaperTrack: subscribe_tracks lagged by {} messages",
                            count
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::info!("ReaperTrack: track broadcast channel closed");
                        break;
                    }
                }
            }

            tracing::info!("ReaperTrack: subscribe_tracks stream ended");
        });
    }
}
