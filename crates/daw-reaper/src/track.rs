//! REAPER Track Service Implementation
//!
//! Implements TrackService for REAPER by dispatching operations to the main thread
//! using TaskSupport from reaper-high. Follows the same pattern as ReaperFx and
//! ReaperTransport.

use daw_proto::{InputMonitoringMode, ProjectContext, RecordInput, Track, TrackExtStateRequest, TrackRef, TrackService};
use reaper_high::{GroupingBehavior, Reaper};
use reaper_medium::GangBehavior;

use crate::main_thread;
use crate::project_context::find_project_by_guid;
use crate::safe_wrappers::routing as routing_sw;

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
fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
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

fn resolve_track(
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

    async fn get_track(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Option<Track> {
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

    async fn set_muted(
        &self,
        project: ProjectContext,
        track: TrackRef,
        muted: bool,
    ) {
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

    async fn set_soloed(
        &self,
        project: ProjectContext,
        track: TrackRef,
        soloed: bool,
    ) {
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

    async fn set_armed(
        &self,
        project: ProjectContext,
        track: TrackRef,
        armed: bool,
    ) {
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

    async fn set_record_input(
        &self,
        project: ProjectContext,
        track: TrackRef,
        input: RecordInput,
    ) {
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

    async fn set_volume(
        &self,
        project: ProjectContext,
        track: TrackRef,
        volume: f64,
    ) {
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

    async fn set_selected(
        &self,
        project: ProjectContext,
        track: TrackRef,
        selected: bool,
    ) {
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

    async fn rename_track(
        &self,
        project: ProjectContext,
        track: TrackRef,
        name: String,
    ) {
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

    async fn set_track_color(
        &self,
        project: ProjectContext,
        track: TrackRef,
        color: u32,
    ) {
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

    async fn set_visible_in_tcp(
        &self,
        project: ProjectContext,
        track: TrackRef,
        visible: bool,
    ) {
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

    async fn set_visible_in_mixer(
        &self,
        project: ProjectContext,
        track: TrackRef,
        visible: bool,
    ) {
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

    async fn remove_all_tracks(
        &self,
        project: ProjectContext,
    ) -> Result<(), String> {
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
            let attr = std::ffi::CString::new(format!(
                "P_EXT:{}:{}", request.section, request.key
            )).ok()?;
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
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
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
                "P_EXT:{}:{}", request.section, request.key
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
        self.set_ext_state(project, track, TrackExtStateRequest {
            section: request.section,
            key: request.key,
            value: String::new(),
        }).await;
    }
}
