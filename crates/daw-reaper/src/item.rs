//! REAPER Item and Take Implementation
//!
//! Implements ItemService and TakeService by dispatching REAPER API calls to the main thread
//! via [`crate::main_thread`].

use crate::main_thread;
use crate::project_context::project_guid as project_guid_from;
use crate::safe_wrappers::item as item_sw;
use crate::track::{resolve_project, resolve_track};
use daw_proto::{
    BeatAttachMode, Duration, FadeShape, Item, ItemEvent, ItemRef, ItemService, PositionInSeconds,
    ProjectContext, SourceType, Take, TakeEvent, TakeRef, TakeService, TrackRef,
};
use reaper_high::{Project, Reaper};
use reaper_medium::{
    DurationInSeconds, ItemAttributeKey, MediaItem, MediaItemTake,
    ProjectContext as ReaperProjectContext, ProjectRef, Semitones, TakeAttributeKey,
    UiRefreshBehavior,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use tracing::{debug, info};
use vox::Tx;

// =============================================================================
// Helpers
// =============================================================================

/// Extract a REAPER item's GUID as a braced string (e.g., "{XXXXXXXX-XXXX-...}")
fn item_guid_string(medium: &reaper_medium::Reaper, item: MediaItem) -> String {
    let guid_ptr = unsafe {
        medium.low().GetSetMediaItemInfo(
            item.as_ptr(),
            b"GUID\0".as_ptr() as _,
            std::ptr::null_mut(),
        ) as *const reaper_low::raw::GUID
    };
    if !guid_ptr.is_null() {
        let g = unsafe { &*guid_ptr };
        format!(
            "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            g.Data1,
            g.Data2,
            g.Data3,
            g.Data4[0],
            g.Data4[1],
            g.Data4[2],
            g.Data4[3],
            g.Data4[4],
            g.Data4[5],
            g.Data4[6],
            g.Data4[7]
        )
    } else {
        format!("{:p}", item.as_ptr())
    }
}

// =============================================================================
// Broadcaster / Cache State
// =============================================================================

/// Global item event broadcaster
static ITEM_BROADCASTER: OnceLock<broadcast::Sender<ItemEvent>> = OnceLock::new();

/// Global take event broadcaster
static TAKE_BROADCASTER: OnceLock<broadcast::Sender<TakeEvent>> = OnceLock::new();

/// Per-project cached item states for change detection.
/// Key is project GUID, value is a vec of lightweight snapshots.
static ITEM_CACHE: OnceLock<Mutex<HashMap<String, Vec<CachedItemState>>>> = OnceLock::new();

/// Lightweight snapshot of item state used for fast diff comparisons.
#[derive(Clone, Debug)]
struct CachedItemState {
    guid: String,
    track_guid: String,
    position: f64,
    length: f64,
    muted: bool,
    selected: bool,
    volume: f64,
    active_take_index: u32,
}

/// Thresholds for floating-point change detection
const ITEM_POSITION_THRESHOLD: f64 = 0.001; // 1ms
const ITEM_LENGTH_THRESHOLD: f64 = 0.001; // 1ms
const ITEM_VOLUME_THRESHOLD: f64 = 0.0001;

/// Initialize item and take broadcasters.
/// Called by the extension during initialization.
pub fn init_item_broadcaster() {
    let (tx, _rx) = broadcast::channel::<ItemEvent>(1024);
    let _ = ITEM_BROADCASTER.set(tx);
    let (take_tx, _rx) = broadcast::channel::<TakeEvent>(512);
    let _ = TAKE_BROADCASTER.set(take_tx);
    let _ = ITEM_CACHE.set(Mutex::new(HashMap::new()));
}

/// Get a receiver for item events.
fn item_receiver() -> Option<broadcast::Receiver<ItemEvent>> {
    ITEM_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Get a receiver for take events.
fn take_receiver() -> Option<broadcast::Receiver<TakeEvent>> {
    TAKE_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Poll REAPER item state for ALL open projects and broadcast changes.
/// **MUST be called from the main thread** (e.g., from timer callback).
///
/// Uses a two-phase approach:
/// 1. Quick count check per project — if item count matches cached count, skip full diff
/// 2. Full per-item diff when count changes or any item state is stale
pub fn poll_and_broadcast_items() {
    let item_tx = ITEM_BROADCASTER.get();

    // Skip if no subscribers
    let has_item_subs = item_tx.map(|t| t.receiver_count() > 0).unwrap_or(false);
    if !has_item_subs {
        return;
    }

    let Some(cache) = ITEM_CACHE.get() else {
        return;
    };
    let mut cache_guard = cache.lock().unwrap();

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let low = medium.low();

    let mut seen_guids = Vec::new();

    // Iterate through all open projects
    for tab_index in 0..128u32 {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            break;
        };

        let project = Project::new(result.project);
        let project_guid = project_guid_from(&project);
        seen_guids.push(project_guid.clone());

        let reaper_ctx = ReaperProjectContext::Proj(result.project);

        // Phase 1: quick count check
        let current_count = medium.count_media_items(reaper_ctx);
        let cached_items = cache_guard.get(&project_guid);
        let cached_count = cached_items.map(|v| v.len() as u32).unwrap_or(u32::MAX);

        // Phase 2: build current snapshot and diff
        let mut current_states = Vec::with_capacity(current_count as usize);
        for i in 0..current_count {
            let Some(item) = medium.get_media_item(reaper_ctx, i) else {
                continue;
            };

            let guid = item_sw::get_item_state_chunk(low, item, 1024)
                .and_then(|chunk| extract_guid_from_chunk(&chunk))
                .unwrap_or_default();

            let track = item_sw::get_media_item_track(medium, item);
            let track_guid = track
                .map(|t| item_sw::get_track_guid(low, t))
                .unwrap_or_default();

            let position = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Position);
            let length = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Length);
            let muted = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Mute) != 0.0;
            let selected = item_sw::is_item_selected(low, item);
            let volume = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Vol);

            let active_take = item_sw::get_active_take(medium, item);
            let take_count = item_sw::count_takes(low, item) as u32;
            let active_take_index = if let Some(active) = active_take {
                let mut idx = 0u32;
                for ti in 0..take_count {
                    if let Some(t) = item_sw::get_take(low, item, ti as i32) {
                        if t == active {
                            idx = ti;
                            break;
                        }
                    }
                }
                idx
            } else {
                0
            };

            current_states.push(CachedItemState {
                guid,
                track_guid,
                position,
                length,
                muted,
                selected,
                volume,
                active_take_index,
            });
        }

        // If counts match and we have a cache, do per-item diff
        if current_count == cached_count {
            if let Some(prev_states) = cached_items {
                // Compare each item
                for (prev, curr) in prev_states.iter().zip(current_states.iter()) {
                    emit_item_diffs(item_tx, &project_guid, prev, curr);
                }
                // Update cache
                cache_guard.insert(project_guid.clone(), current_states);
                continue;
            }
        }

        // Count mismatch or no cache — detect creates/deletes and update everything
        if let Some(prev_states) = cached_items.cloned() {
            // Build lookup of previous GUIDs
            let prev_guids: HashMap<&str, &CachedItemState> =
                prev_states.iter().map(|s| (s.guid.as_str(), s)).collect();
            let curr_guids: HashMap<&str, &CachedItemState> = current_states
                .iter()
                .map(|s| (s.guid.as_str(), s))
                .collect();

            // Deleted items: in prev but not in curr
            for prev in &prev_states {
                if !prev.guid.is_empty() && !curr_guids.contains_key(prev.guid.as_str()) {
                    if let Some(tx) = item_tx {
                        let _ = tx.send(ItemEvent::Deleted {
                            project_guid: project_guid.clone(),
                            track_guid: prev.track_guid.clone(),
                            item_guid: prev.guid.clone(),
                        });
                    }
                }
            }

            // Created items: in curr but not in prev; changed items: in both
            for curr in &current_states {
                if curr.guid.is_empty() {
                    continue;
                }
                if let Some(prev) = prev_guids.get(curr.guid.as_str()) {
                    emit_item_diffs(item_tx, &project_guid, prev, curr);
                } else {
                    // New item — emit Created with a full Item snapshot
                    // We build a lightweight Item here; the subscriber can query for full details
                    if let Some(tx) = item_tx {
                        let _ = tx.send(ItemEvent::Created {
                            project_guid: project_guid.clone(),
                            track_guid: curr.track_guid.clone(),
                            item: Item {
                                guid: curr.guid.clone(),
                                track_guid: curr.track_guid.clone(),
                                index: 0,
                                position: daw_proto::PositionInSeconds::from_seconds(curr.position),
                                length: Duration::from_seconds(curr.length),
                                snap_offset: Duration::from_seconds(0.0),
                                muted: curr.muted,
                                selected: curr.selected,
                                locked: false,
                                volume: curr.volume,
                                fade_in_length: Duration::from_seconds(0.0),
                                fade_out_length: Duration::from_seconds(0.0),
                                fade_in_shape: FadeShape::Linear,
                                fade_out_shape: FadeShape::Linear,
                                beat_attach_mode: BeatAttachMode::Time,
                                loop_source: false,
                                auto_stretch: false,
                                color: None,
                                group_id: None,
                                take_count: 0,
                                active_take_index: curr.active_take_index,
                            },
                        });
                    }
                }
            }
        } else {
            // No previous cache at all — first poll, don't emit events
        }

        cache_guard.insert(project_guid.clone(), current_states);
    }

    // Clean up cache entries for projects that are no longer open
    cache_guard.retain(|guid, _| seen_guids.contains(guid));
}

/// Emit diff events for a single item by comparing previous and current cached states.
fn emit_item_diffs(
    item_tx: Option<&broadcast::Sender<ItemEvent>>,
    project_guid: &str,
    prev: &CachedItemState,
    curr: &CachedItemState,
) {
    let Some(tx) = item_tx else { return };

    if (prev.position - curr.position).abs() > ITEM_POSITION_THRESHOLD {
        let _ = tx.send(ItemEvent::PositionChanged {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            old_position: prev.position,
            new_position: curr.position,
        });
    }

    if (prev.length - curr.length).abs() > ITEM_LENGTH_THRESHOLD {
        let _ = tx.send(ItemEvent::LengthChanged {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            old_length: prev.length,
            new_length: curr.length,
        });
    }

    if prev.track_guid != curr.track_guid {
        let _ = tx.send(ItemEvent::MovedToTrack {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            old_track_guid: prev.track_guid.clone(),
            new_track_guid: curr.track_guid.clone(),
        });
    }

    if prev.muted != curr.muted {
        let _ = tx.send(ItemEvent::MuteChanged {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            muted: curr.muted,
        });
    }

    if prev.selected != curr.selected {
        let _ = tx.send(ItemEvent::SelectionChanged {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            selected: curr.selected,
        });
    }

    if (prev.volume - curr.volume).abs() > ITEM_VOLUME_THRESHOLD {
        let _ = tx.send(ItemEvent::VolumeChanged {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            volume: curr.volume,
        });
    }

    if prev.active_take_index != curr.active_take_index {
        let _ = tx.send(ItemEvent::ActiveTakeChanged {
            project_guid: project_guid.to_string(),
            item_guid: curr.guid.clone(),
            old_take_index: prev.active_take_index,
            new_take_index: curr.active_take_index,
        });
    }
}

// =============================================================================
// ReaperItem
// =============================================================================

/// REAPER item implementation.
///
/// All methods dispatch to the main thread via [`crate::main_thread`].
#[derive(Clone)]
pub struct ReaperItem;

impl ReaperItem {
    pub fn new() -> Self {
        Self
    }

    /// Resolve an ItemRef to a MediaItem pointer.
    ///
    /// Validates the pointer after resolution to guard against stale items.
    fn resolve_item(item_ref: &ItemRef, project: ReaperProjectContext) -> Option<MediaItem> {
        let reaper = Reaper::get();
        let medium = reaper.medium_reaper();

        let item = match item_ref {
            ItemRef::Guid(guid) => {
                // Iterate all items in the project and match GUID
                let item_count = medium.count_media_items(project);
                for i in 0..item_count {
                    if let Some(candidate) = medium.get_media_item(project, i) {
                        let guid_ptr = unsafe {
                            medium.low().GetSetMediaItemInfo(
                                candidate.as_ptr(),
                                b"GUID\0".as_ptr() as _,
                                std::ptr::null_mut(),
                            ) as *const reaper_low::raw::GUID
                        };
                        if !guid_ptr.is_null() {
                            let g = unsafe { &*guid_ptr };
                            let item_guid = format!(
                                "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
                                g.Data1,
                                g.Data2,
                                g.Data3,
                                g.Data4[0],
                                g.Data4[1],
                                g.Data4[2],
                                g.Data4[3],
                                g.Data4[4],
                                g.Data4[5],
                                g.Data4[6],
                                g.Data4[7]
                            );
                            if item_guid == *guid {
                                return Some(candidate);
                            }
                        }
                    }
                }
                return None;
            }
            ItemRef::Index(idx) => {
                let track = medium.get_track(project, 0)?;
                item_sw::get_track_media_item(medium, track, *idx)?
            }
            ItemRef::ProjectIndex(idx) => medium.get_media_item(project, *idx)?,
        };
        if !main_thread::is_item_valid(project, item) {
            return None;
        }
        Some(item)
    }

    /// Convert MediaItem to Item struct
    fn media_item_to_item(item: MediaItem) -> Option<Item> {
        let reaper = Reaper::get();
        let medium = reaper.medium_reaper();
        let low = medium.low();

        let guid = item_sw::get_item_state_chunk(low, item, 1024)
            .and_then(|chunk| extract_guid_from_chunk(&chunk))
            .unwrap_or_default();

        let track = item_sw::get_media_item_track(medium, item)?;
        let track_guid = item_sw::get_track_guid(low, track);

        let position = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Position);
        let length = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Length);
        let snap_offset = item_sw::get_item_info_value(medium, item, ItemAttributeKey::SnapOffset);

        let muted = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Mute) != 0.0;
        let selected = item_sw::is_item_selected(low, item);
        // Lock is not available in reaper_medium ItemAttributeKey
        let locked = false;

        let volume = item_sw::get_item_info_value(medium, item, ItemAttributeKey::Vol);
        let fade_in_length =
            item_sw::get_item_info_value(medium, item, ItemAttributeKey::FadeInLen);
        let fade_out_length =
            item_sw::get_item_info_value(medium, item, ItemAttributeKey::FadeOutLen);

        // Fade shapes - REAPER uses different numbering
        let fade_in_shape_raw =
            item_sw::get_item_info_value(medium, item, ItemAttributeKey::FadeInShape) as u8;
        let fade_out_shape_raw =
            item_sw::get_item_info_value(medium, item, ItemAttributeKey::FadeOutShape) as u8;

        let loop_source =
            item_sw::get_item_info_value(medium, item, ItemAttributeKey::LoopSrc) != 0.0;

        let color_raw =
            item_sw::get_item_info_value(medium, item, ItemAttributeKey::CustomColor) as i32;
        let color = if color_raw > 0 {
            Some(color_raw as u32)
        } else {
            None
        };

        let take_count = item_sw::count_takes(low, item) as u32;

        let active_take = item_sw::get_active_take(medium, item);
        // Find active take index by comparing pointers
        let active_take_index = if let Some(active) = active_take {
            let mut found_index = 0;
            for i in 0..take_count {
                if let Some(take) = item_sw::get_take(low, item, i as i32) {
                    if take == active {
                        found_index = i;
                        break;
                    }
                }
            }
            found_index
        } else {
            0
        };

        Some(Item {
            guid,
            track_guid,
            index: 0, // Will be set by caller if needed
            position: PositionInSeconds::from_seconds(position),
            length: Duration::from_seconds(length),
            snap_offset: Duration::from_seconds(snap_offset),
            muted,
            selected,
            locked,
            volume,
            fade_in_length: Duration::from_seconds(fade_in_length),
            fade_out_length: Duration::from_seconds(fade_out_length),
            fade_in_shape: reaper_fade_to_proto(fade_in_shape_raw),
            fade_out_shape: reaper_fade_to_proto(fade_out_shape_raw),
            beat_attach_mode: BeatAttachMode::Time, // TODO: Read from REAPER
            loop_source,
            auto_stretch: false, // TODO: Read from REAPER
            color,
            group_id: None, // TODO: Read from REAPER
            take_count,
            active_take_index,
        })
    }
}

impl Default for ReaperItem {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemService for ReaperItem {
    // =========================================================================
    // Queries
    // =========================================================================

    async fn get_items(&self, _project: ProjectContext, track: TrackRef) -> Vec<Item> {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let mut items = Vec::new();

            // Resolve track
            let track_ptr = match track {
                TrackRef::Master => {
                    Some(medium.get_master_track(ReaperProjectContext::CurrentProject))
                }
                TrackRef::Index(idx) => medium.get_track(ReaperProjectContext::CurrentProject, idx),
                TrackRef::Guid(_) => {
                    // GUID lookup not directly supported
                    None
                }
            };

            if let Some(track) = track_ptr {
                let count = item_sw::count_track_media_items(medium, track);
                for i in 0..count {
                    if let Some(item) = item_sw::get_track_media_item(medium, track, i)
                        && let Some(mut item_data) = Self::media_item_to_item(item)
                    {
                        item_data.index = i;
                        items.push(item_data);
                    }
                }
            }

            items
        })
        .await
        .unwrap_or_default()
    }

    async fn get_item(&self, _project: ProjectContext, item: ItemRef) -> Option<Item> {
        main_thread::query(move || {
            let item_ptr = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            Self::media_item_to_item(item_ptr)
        })
        .await
        .unwrap_or(None)
    }

    async fn get_all_items(&self, _project: ProjectContext) -> Vec<Item> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let mut items = Vec::new();

            let count = medium.count_media_items(ReaperProjectContext::CurrentProject);
            for i in 0..count {
                if let Some(item) = medium.get_media_item(ReaperProjectContext::CurrentProject, i)
                    && let Some(mut item_data) = Self::media_item_to_item(item)
                {
                    item_data.index = i;
                    items.push(item_data);
                }
            }

            items
        })
        .await
        .unwrap_or_default()
    }

    async fn get_selected_items(&self, _project: ProjectContext) -> Vec<Item> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let mut items = Vec::new();

            let count = medium.count_selected_media_items(ReaperProjectContext::CurrentProject);
            for i in 0..count {
                if let Some(item) =
                    medium.get_selected_media_item(ReaperProjectContext::CurrentProject, i)
                    && let Some(item_data) = Self::media_item_to_item(item)
                {
                    items.push(item_data);
                }
            }

            items
        })
        .await
        .unwrap_or_default()
    }

    async fn item_count(&self, project: ProjectContext, track: TrackRef) -> u32 {
        main_thread::query(move || {
            let proj =
                resolve_project(&project).or_else(|| Some(Reaper::get().current_project()))?;
            let reaper_track = resolve_track(&proj, &track)?;
            Some(reaper_track.item_count())
        })
        .await
        .flatten()
        .unwrap_or(0)
    }

    // =========================================================================
    // CRUD Operations
    // =========================================================================

    async fn add_item(
        &self,
        project: ProjectContext,
        track: TrackRef,
        position: PositionInSeconds,
        length: Duration,
    ) -> Option<String> {
        debug!(
            "ReaperItem: add_item {:?} at {} for {}",
            track,
            position.as_seconds(),
            length.as_seconds()
        );
        main_thread::query(move || {
            let proj =
                resolve_project(&project).or_else(|| Some(Reaper::get().current_project()))?;
            let reaper_track = resolve_track(&proj, &track)?;

            // Use CreateNewMIDIItemInProj via existing safe wrapper —
            // creates a properly initialized MIDI item with active take
            let low = Reaper::get().medium_reaper().low();
            let track_ptr = reaper_track.raw().ok()?;
            let start = position.as_seconds();
            let end = start + length.as_seconds();

            let item =
                crate::safe_wrappers::midi::create_new_midi_item(low, track_ptr, start, end)?;

            Reaper::get().medium_reaper().update_timeline();

            // Get the item GUID
            let guid = item_guid_string(Reaper::get().medium_reaper(), item);
            Some(guid)
        })
        .await
        .unwrap_or(None)
    }

    async fn delete_item(&self, _project: ProjectContext, item: ItemRef) {
        debug!("ReaperItem: delete_item");
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                if let Some(track) = item_sw::get_media_item_track(medium, item_ptr) {
                    item_sw::delete_track_media_item(medium, track, item_ptr);
                }
            }
        });
    }

    async fn duplicate_item(&self, _project: ProjectContext, item: ItemRef) -> Option<String> {
        debug!("ReaperItem: duplicate_item");
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let item_ptr = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)?;

            // First select only this item
            medium.select_all_media_items(ReaperProjectContext::CurrentProject, false);
            item_sw::set_media_item_selected(medium, item_ptr, true);

            // Duplicate using action
            item_sw::main_on_command_ex(
                medium,
                reaper_medium::CommandId::new(41295), // Item: Duplicate items
                0,
                ReaperProjectContext::CurrentProject,
            );

            // Get the newly duplicated item (should be the last selected)
            let count = medium.count_selected_media_items(ReaperProjectContext::CurrentProject);
            if count > 0 {
                let new_item = medium
                    .get_selected_media_item(ReaperProjectContext::CurrentProject, count - 1)?;
                // Use pointer as temporary ID
                Some(format!("{:p}", new_item.as_ptr()))
            } else {
                None
            }
        })
        .await
        .unwrap_or(None)
    }

    // =========================================================================
    // Position & Length
    // =========================================================================

    async fn set_position(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        position: PositionInSeconds,
    ) {
        debug!("ReaperItem: set_position to {}", position.as_seconds());
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
                && let Ok(pos) = reaper_medium::PositionInSeconds::new(position.as_seconds())
            {
                item_sw::set_media_item_position(medium, item_ptr, pos, UiRefreshBehavior::Refresh);
            }
        });
    }

    async fn set_length(&self, _project: ProjectContext, item: ItemRef, length: Duration) {
        debug!("ReaperItem: set_length to {}", length.as_seconds());
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
                && let Ok(len) = DurationInSeconds::new(length.as_seconds())
            {
                item_sw::set_media_item_length(medium, item_ptr, len, UiRefreshBehavior::Refresh);
            }
        });
    }

    async fn move_to_track(&self, project: ProjectContext, item: ItemRef, track: TrackRef) {
        debug!("ReaperItem: move_to_track");
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                let Some(proj) = resolve_project(&project) else {
                    return;
                };
                if let Some(resolved) = resolve_track(&proj, &track) {
                    if let Ok(raw) = resolved.raw() {
                        item_sw::move_item_to_track(medium.low(), item_ptr, raw);
                    }
                }
            }
        });
    }

    async fn set_snap_offset(&self, _project: ProjectContext, item: ItemRef, offset: Duration) {
        debug!("ReaperItem: set_snap_offset to {}", offset.as_seconds());
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::SnapOffset,
                    offset.as_seconds(),
                );
            }
        });
    }

    // =========================================================================
    // State
    // =========================================================================

    async fn set_muted(&self, _project: ProjectContext, item: ItemRef, muted: bool) {
        debug!("ReaperItem: set_muted to {}", muted);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::Mute,
                    if muted { 1.0 } else { 0.0 },
                );
            }
        });
    }

    async fn set_selected(&self, _project: ProjectContext, item: ItemRef, selected: bool) {
        debug!("ReaperItem: set_selected to {}", selected);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_media_item_selected(medium, item_ptr, selected);
            }
        });
    }

    async fn set_locked(&self, _project: ProjectContext, item: ItemRef, locked: bool) {
        debug!("ReaperItem: set_locked to {}", locked);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Lock attribute not available in reaper_medium
            // Would need chunk manipulation or low-level API
            let _ = (item, locked, medium, reaper);
        });
    }

    async fn select_all_items(&self, _project: ProjectContext, selected: bool) {
        debug!("ReaperItem: select_all_items to {}", selected);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            medium.select_all_media_items(ReaperProjectContext::CurrentProject, selected);
        });
    }

    // =========================================================================
    // Audio Properties
    // =========================================================================

    async fn set_volume(&self, _project: ProjectContext, item: ItemRef, volume: f64) {
        debug!("ReaperItem: set_volume to {}", volume);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(medium, item_ptr, ItemAttributeKey::Vol, volume);
            }
        });
    }

    async fn set_fade_in(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        length: Duration,
        shape: FadeShape,
    ) {
        debug!(
            "ReaperItem: set_fade_in length={}, shape={:?}",
            length.as_seconds(),
            shape
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::FadeInLen,
                    length.as_seconds(),
                );
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::FadeInShape,
                    proto_fade_to_reaper(shape) as f64,
                );
            }
        });
    }

    async fn set_fade_out(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        length: Duration,
        shape: FadeShape,
    ) {
        debug!(
            "ReaperItem: set_fade_out length={}, shape={:?}",
            length.as_seconds(),
            shape
        );
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::FadeOutLen,
                    length.as_seconds(),
                );
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::FadeOutShape,
                    proto_fade_to_reaper(shape) as f64,
                );
            }
        });
    }

    // =========================================================================
    // Timing Behavior
    // =========================================================================

    async fn set_loop_source(&self, _project: ProjectContext, item: ItemRef, loop_source: bool) {
        debug!("ReaperItem: set_loop_source to {}", loop_source);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::LoopSrc,
                    if loop_source { 1.0 } else { 0.0 },
                );
            }
        });
    }

    async fn set_beat_attach_mode(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        mode: BeatAttachMode,
    ) {
        debug!("ReaperItem: set_beat_attach_mode to {:?}", mode);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                // REAPER uses TimeBase attribute
                // 0 = time, 1 = beats (position, length, rate), 2 = beats (position only)
                let timebase = match mode {
                    BeatAttachMode::Time => 0.0,
                    BeatAttachMode::Beats => 1.0,
                    BeatAttachMode::BeatsPositionOnly => 2.0,
                };
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::BeatAttachMode,
                    timebase,
                );
            }
        });
    }

    async fn set_auto_stretch(&self, _project: ProjectContext, item: ItemRef, auto_stretch: bool) {
        debug!("ReaperItem: set_auto_stretch to {}", auto_stretch);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::AutoStretch,
                    if auto_stretch { 1.0 } else { 0.0 },
                );
            }
        });
    }

    // =========================================================================
    // Visual Properties
    // =========================================================================

    async fn set_color(&self, _project: ProjectContext, item: ItemRef, color: Option<u32>) {
        debug!("ReaperItem: set_color to {:?}", color);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                // Bit 24 (0x01000000) tells REAPER to use the custom color.
                // Without it, the item inherits its track's color.
                let color_value = color.map(|c| (c as i32) | 0x01000000).unwrap_or(0);
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::CustomColor,
                    color_value as f64,
                );
            }
        });
    }

    async fn set_group_id(&self, _project: ProjectContext, item: ItemRef, group_id: Option<u32>) {
        debug!("ReaperItem: set_group_id to {:?}", group_id);
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            if let Some(item_ptr) = Self::resolve_item(&item, ReaperProjectContext::CurrentProject)
            {
                let group_value = group_id.map(|g| g as i32).unwrap_or(0);
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::GroupId,
                    group_value as f64,
                );
            }
        });
    }

    // =========================================================================
    // Subscriptions
    // =========================================================================

    async fn subscribe_items(&self, _project: ProjectContext, tx: Tx<ItemEvent>) {
        info!("ReaperItem: subscribe_items - subscribing to broadcast channel");

        let Some(mut rx) = item_receiver() else {
            info!("ReaperItem: item broadcast channel not initialized");
            return;
        };

        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            debug!("ReaperItem: subscribe_items stream closed");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!("ReaperItem: subscribe_items lagged by {} messages", count);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("ReaperItem: item broadcast channel closed");
                        break;
                    }
                }
            }
            info!("ReaperItem: subscribe_items stream ended");
        });
    }
}

// =============================================================================
// TakeService Implementation
// =============================================================================

/// REAPER take implementation.
#[derive(Clone)]
pub struct ReaperTake;

impl ReaperTake {
    pub fn new() -> Self {
        Self
    }

    /// Stub for getting take GUID (take chunks not supported yet).
    fn _get_take_state_chunk(
        _take: MediaItemTake,
        _buffer_size: u32,
    ) -> Result<String, &'static str> {
        // TODO: REAPER doesn't have GetSetItemState2 for takes
        Err("take chunk reading not implemented yet")
    }

    /// Resolve a TakeRef within an item
    fn resolve_take(item: MediaItem, take_ref: &TakeRef) -> Option<MediaItemTake> {
        let reaper = Reaper::get();
        let medium = reaper.medium_reaper();
        let low = medium.low();

        match take_ref {
            TakeRef::Guid(guid) => {
                // Search takes by GUID using low-level API
                let count = item_sw::count_takes(low, item);

                for i in 0..count {
                    if let Some(take) = item_sw::get_take(low, item, i)
                        && let Ok(chunk) = Self::_get_take_state_chunk(take, 1024)
                        && let Some(take_guid) = extract_guid_from_chunk(&chunk)
                        && &take_guid == guid
                    {
                        return Some(take);
                    }
                }
                None
            }
            TakeRef::Index(idx) => item_sw::get_take(low, item, *idx as i32),
            TakeRef::Active => item_sw::get_active_take(medium, item),
        }
    }

    /// Convert MediaItemTake to Take struct
    fn media_take_to_take(item: MediaItem, take: MediaItemTake, index: u32) -> Option<Take> {
        let reaper = Reaper::get();
        let medium = reaper.medium_reaper();
        let low = medium.low();

        let guid = Self::_get_take_state_chunk(take, 1024)
            .ok()
            .and_then(|chunk| extract_guid_from_chunk(&chunk))
            .unwrap_or_default();

        let item_guid = item_sw::get_item_state_chunk(low, item, 1024)
            .and_then(|chunk| extract_guid_from_chunk(&chunk))
            .unwrap_or_default();

        let active_take = item_sw::get_active_take(medium, item);
        let is_active = active_take == Some(take);

        let name = item_sw::get_take_name(low, take);

        let volume = item_sw::get_take_info_value(medium, take, TakeAttributeKey::Vol);

        let play_rate = item_sw::get_take_info_value(medium, take, TakeAttributeKey::PlayRate);

        let pitch = item_sw::get_take_pitch(medium, take);

        let preserve_pitch_raw =
            item_sw::get_take_info_value(medium, take, TakeAttributeKey::PitchMode);
        let preserve_pitch = preserve_pitch_raw != 0.0;

        let start_offset = item_sw::get_take_info_value(medium, take, TakeAttributeKey::StartOffs);

        // Get source info
        // TODO: Implement proper source inspection using low-level API
        let source = item_sw::get_take_source(medium, take);
        let (source_type, source_length, source_sample_rate, source_channels, is_midi) =
            if source.is_some() {
                // For now, assume audio - proper implementation needs low-level API wrappers
                (SourceType::Audio, None, None, None, false)
            } else {
                (SourceType::Empty, None, None, None, false)
            };

        let midi_note_count = if is_midi {
            // TODO: Implement using MIDI_CountEvts low-level API
            Some(0)
        } else {
            None
        };

        Some(Take {
            guid,
            item_guid,
            index,
            is_active,
            name,
            color: None, // TODO: Read from chunk
            volume,
            play_rate,
            pitch,
            preserve_pitch,
            start_offset: Duration::from_seconds(start_offset),
            source_type,
            source_length,
            source_sample_rate,
            source_channels,
            is_midi,
            midi_note_count,
        })
    }
}

impl Default for ReaperTake {
    fn default() -> Self {
        Self::new()
    }
}

impl TakeService for ReaperTake {
    // =========================================================================
    // Queries
    // =========================================================================

    async fn get_takes(&self, _project: ProjectContext, item: ItemRef) -> Vec<Take> {
        main_thread::query(move || {
            let item_ptr = ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();
            let mut takes = Vec::new();

            let count = item_sw::count_takes(low, item_ptr);
            for i in 0..count {
                if let Some(take) = item_sw::get_take(low, item_ptr, i)
                    && let Some(take_data) = Self::media_take_to_take(item_ptr, take, i as u32)
                {
                    takes.push(take_data);
                }
            }

            Some(takes)
        })
        .await
        .unwrap_or(None)
        .unwrap_or_default()
    }

    async fn get_take(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> Option<Take> {
        main_thread::query(move || {
            let item_ptr = ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            let take_ptr = Self::resolve_take(item_ptr, &take)?;
            let index = match take {
                TakeRef::Index(idx) => idx,
                _ => {
                    // Find index by comparing pointers
                    let reaper = Reaper::get();
                    let low = reaper.medium_reaper().low();
                    let mut found_index = 0;
                    let count = item_sw::count_takes(low, item_ptr);
                    for i in 0..count {
                        if item_sw::get_take(low, item_ptr, i) == Some(take_ptr) {
                            found_index = i as u32;
                            break;
                        }
                    }
                    found_index
                }
            };
            Self::media_take_to_take(item_ptr, take_ptr, index)
        })
        .await
        .unwrap_or(None)
    }

    async fn get_active_take(&self, _project: ProjectContext, item: ItemRef) -> Option<Take> {
        main_thread::query(move || {
            let item_ptr = ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();

            let take_ptr = item_sw::get_active_take(medium, item_ptr)?;
            // Find index by comparing pointers
            let mut index = 0;
            let count = item_sw::count_takes(low, item_ptr);
            for i in 0..count {
                if item_sw::get_take(low, item_ptr, i) == Some(take_ptr) {
                    index = i as u32;
                    break;
                }
            }
            Self::media_take_to_take(item_ptr, take_ptr, index)
        })
        .await
        .unwrap_or(None)
    }

    async fn take_count(&self, _project: ProjectContext, item: ItemRef) -> u32 {
        main_thread::query(move || {
            let item_ptr = ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            let low = Reaper::get().medium_reaper().low();
            Some(item_sw::count_takes(low, item_ptr) as u32)
        })
        .await
        .unwrap_or(None)
        .unwrap_or(0)
    }

    // =========================================================================
    // CRUD Operations
    // =========================================================================

    async fn add_take(&self, _project: ProjectContext, item: ItemRef) -> Option<String> {
        debug!("ReaperTake: add_take");
        main_thread::query(move || {
            let item_ptr = ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            let medium = Reaper::get().medium_reaper();

            let take = item_sw::add_take_to_media_item(medium, item_ptr)?;
            let chunk = Self::_get_take_state_chunk(take, 1024).ok()?;
            extract_guid_from_chunk(&chunk)
        })
        .await
        .unwrap_or(None)
    }

    async fn delete_take(&self, _project: ProjectContext, item: ItemRef, take: TakeRef) {
        debug!("ReaperTake: delete_take");
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(_take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            // TODO: Implement take deletion
            // REAPER doesn't have DeleteTakeFromMediaItem in the API
        });
    }

    async fn set_active_take(&self, _project: ProjectContext, item: ItemRef, take: TakeRef) {
        debug!("ReaperTake: set_active_take");
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            let low = Reaper::get().medium_reaper().low();
            item_sw::set_active_take(low, take_ptr);
        });
    }

    // =========================================================================
    // Metadata
    // =========================================================================

    async fn set_name(&self, _project: ProjectContext, item: ItemRef, take: TakeRef, name: String) {
        debug!("ReaperTake: set_name to '{}'", name);
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            let low = Reaper::get().medium_reaper().low();
            if let Ok(cname) = std::ffi::CString::new(name) {
                item_sw::set_take_name(low, take_ptr, &cname);
            }
        });
    }

    async fn set_color(
        &self,
        _project: ProjectContext,
        _item: ItemRef,
        _take: TakeRef,
        color: Option<u32>,
    ) {
        debug!("ReaperTake: set_color to {:?}", color);
        // TODO: Implement using chunk manipulation or low-level API
    }

    // =========================================================================
    // Playback Properties
    // =========================================================================

    async fn set_volume(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        volume: f64,
    ) {
        debug!("ReaperTake: set_volume to {}", volume);
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            let medium = Reaper::get().medium_reaper();
            item_sw::set_take_info_value(medium, take_ptr, TakeAttributeKey::Vol, volume);
        });
    }

    async fn set_play_rate(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        rate: f64,
    ) {
        debug!("ReaperTake: set_play_rate to {}", rate);
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            let medium = Reaper::get().medium_reaper();
            item_sw::set_take_info_value(medium, take_ptr, TakeAttributeKey::PlayRate, rate);
        });
    }

    async fn set_pitch(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        semitones: f64,
    ) {
        debug!("ReaperTake: set_pitch to {} semitones", semitones);
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            let medium = Reaper::get().medium_reaper();
            if let Ok(pitch) = Semitones::new(semitones) {
                item_sw::set_take_pitch(medium, take_ptr, pitch);
            }
        });
    }

    async fn set_preserve_pitch(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        preserve: bool,
    ) {
        debug!("ReaperTake: set_preserve_pitch to {}", preserve);
        main_thread::run(move || {
            let Some(_item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(_take_ptr) = Self::resolve_take(_item_ptr, &take) else {
                return;
            };
            // TODO: Implement proper pitch mode setting
            let _preserve = preserve;
        });
    }

    async fn set_start_offset(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        offset: Duration,
    ) {
        debug!("ReaperTake: set_start_offset to {}", offset.as_seconds());
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            let medium = Reaper::get().medium_reaper();
            item_sw::set_take_info_value(
                medium,
                take_ptr,
                TakeAttributeKey::StartOffs,
                offset.as_seconds(),
            );
        });
    }

    // =========================================================================
    // Source Management
    // =========================================================================

    async fn set_source_file(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        path: String,
    ) {
        debug!("ReaperTake: set_source_file to '{}'", path);
        main_thread::run(move || {
            let Some(item_ptr) =
                ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)
            else {
                return;
            };
            let Some(_take_ptr) = Self::resolve_take(item_ptr, &take) else {
                return;
            };
            // TODO: Create a new PCM source from file
            let _path = path;
        });
    }

    async fn get_source_type(
        &self,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> SourceType {
        main_thread::query(move || {
            let item_ptr = ReaperItem::resolve_item(&item, ReaperProjectContext::CurrentProject)?;
            let take_ptr = Self::resolve_take(item_ptr, &take)?;
            let medium = Reaper::get().medium_reaper();

            // TODO: Implement source type detection using low-level API
            let _source = item_sw::get_take_source(medium, take_ptr)?;
            // For now, return Audio as default
            Some(SourceType::Audio)
        })
        .await
        .unwrap_or(None)
        .unwrap_or(SourceType::Unknown)
    }

    // =========================================================================
    // Subscriptions
    // =========================================================================

    async fn subscribe_takes(&self, _project: ProjectContext, tx: Tx<TakeEvent>) {
        info!("ReaperTake: subscribe_takes - subscribing to broadcast channel");

        let Some(mut rx) = take_receiver() else {
            info!("ReaperTake: take broadcast channel not initialized");
            return;
        };

        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            debug!("ReaperTake: subscribe_takes stream closed");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!("ReaperTake: subscribe_takes lagged by {} messages", count);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("ReaperTake: take broadcast channel closed");
                        break;
                    }
                }
            }
            info!("ReaperTake: subscribe_takes stream ended");
        });
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Extract GUID from a REAPER state chunk
fn extract_guid_from_chunk(chunk: &str) -> Option<String> {
    // Look for GUID line like: GUID {12345678-1234-1234-1234-123456789ABC}
    for line in chunk.lines() {
        if line.starts_with("GUID ")
            && let Some(guid_part) = line.strip_prefix("GUID ")
        {
            return Some(guid_part.trim().to_string());
        }
    }
    None
}

/// Convert proto FadeShape to REAPER fade shape index
fn proto_fade_to_reaper(shape: FadeShape) -> i32 {
    match shape {
        FadeShape::Linear => 0,
        FadeShape::FastStart => 1,
        FadeShape::FastEnd => 2,
        FadeShape::FastStartSteep => 3,
        FadeShape::FastEndSteep => 4,
        FadeShape::SlowStartEnd => 5,
        FadeShape::SlowStartEndSteep => 6,
    }
}

/// Convert REAPER fade shape index to proto FadeShape
fn reaper_fade_to_proto(shape: u8) -> FadeShape {
    match shape {
        0 => FadeShape::Linear,
        1 => FadeShape::FastStart,
        2 => FadeShape::FastEnd,
        3 => FadeShape::FastStartSteep,
        4 => FadeShape::FastEndSteep,
        5 => FadeShape::SlowStartEnd,
        6 => FadeShape::SlowStartEndSteep,
        _ => FadeShape::Linear,
    }
}
