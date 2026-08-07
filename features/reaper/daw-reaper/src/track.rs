//! `impl Tracks for Reaper` — sync trait + REAPER C API.
//!
//! Mounting goes through `daw_proto::track::serve(Reaper)`. The
//! dispatcher (REAPER's main thread queue) is pulled off the backend
//! via `HasDispatcher` on `Reaper`. Each method body assumes it's
//! running on the main thread — that contract is enforced by the
//! bridge before the call lands here.
//!
//! Helpers (`resolve_project`, `resolve_track`, `build_track_info`,
//! `assign_parent_guids`) are kept `pub(crate)` so other modules
//! (midi, batch, etc.) can reuse them.

use std::cell::RefCell;

use daw_proto::Tracks;
use daw_proto::{
    DawError, DawResult, ProjectContext, RecordInput,
    ReorderTracksBehavior as ProtoReorderTracksBehavior, Track, TrackRef,
};
use reaper_high::{GroupingBehavior, Project, Reaper as ReaperHigh};
use reaper_medium::{
    ChunkCacheHint, GangBehavior, ReorderTracksBehavior, TrackAttributeKey, TrackPolarity,
};

use crate::main_thread;
use crate::project_context::{find_project_by_guid, project_guid};

// ── Per-thread project cache ───────────────────────────────────────────
//
// `resolve_project(ProjectContext::Project(guid))` is hot — most calls
// resolve to the same project the previous call did. Cache the
// resolved `Project` per thread to avoid the FFI loop over tabs.

thread_local! {
    static CURRENT_PROJECT_CACHE: RefCell<Option<(String, reaper_high::Project)>> =
        const { RefCell::new(None) };
}

/// Cache the current project's guid for the duration of a batch.
/// Must be called from the main thread. Call `clear_project_cache()`
/// when done.
pub(crate) fn set_project_cache(guid: String, project: reaper_high::Project) {
    CURRENT_PROJECT_CACHE.with(|c| {
        *c.borrow_mut() = Some((guid, project));
    });
}

/// Clear the project cache after a batch completes.
pub(crate) fn clear_project_cache() {
    CURRENT_PROJECT_CACHE.with(|c| {
        *c.borrow_mut() = None;
    });
}

// ── Helpers ────────────────────────────────────────────────────────────

pub(crate) fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(ReaperHigh::get().current_project()),
        ProjectContext::Project(guid) => {
            let cached = CURRENT_PROJECT_CACHE.with(|c| {
                c.borrow()
                    .as_ref()
                    .filter(|(cached_guid, _)| cached_guid == guid)
                    .map(|(_, proj)| *proj)
            });
            if let Some(proj) = cached {
                return Some(proj);
            }
            let current = ReaperHigh::get().current_project();
            if project_guid(&current) == *guid {
                return Some(current);
            }
            find_project_by_guid(guid)
        }
    }
}

/// Public alias used by sibling modules (midi.rs etc).
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
    if !main_thread::is_track_valid(project, &track) {
        return None;
    }
    Some(track)
}

pub(crate) fn build_track_info(track: &reaper_high::Track) -> Track {
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
        phase_inverted: track.phase_is_inverted(),
        automation_mode: {
            use daw_proto::primitives::AutomationMode as P;
            use reaper_medium::AutomationMode as R;
            match track.automation_mode() {
                R::TrimRead => P::TrimRead,
                R::Read => P::Read,
                R::Touch => P::Touch,
                R::Write => P::Write,
                R::Latch => P::Latch,
                R::LatchPreview => P::LatchPreview,
                R::Unknown(_) => P::TrimRead,
            }
        },
        input_monitor: {
            use daw_proto::track::InputMonitoringMode as P;
            use reaper_medium::InputMonitoringMode as R;
            match track.input_monitoring_mode() {
                R::Off => P::Off,
                R::Normal => P::Normal,
                R::NotWhenPlaying => P::NotWhenPlaying,
                _ => P::Off,
            }
        },
        parent_guid: None,
        folder_depth,
        is_folder,
        // Fixed lanes via the live API (I_NUMFIXEDLANES /
        // C_LANEPLAYS) — not yet wired through reaper-rs.
        lane_count: 0,
        lane_play_mask: 0,
        lane_names: Vec::new(),
        lane_display: daw_proto::track::LaneDisplay::default(),
        grouping: daw_proto::track::TrackGrouping::default(),
        visible_in_tcp,
        visible_in_mixer,
        fx_count,
        input_fx_count,
    }
}

pub(crate) fn assign_parent_guids(tracks: &mut [Track]) {
    let mut folder_stack: Vec<String> = Vec::new();
    for track in tracks.iter_mut() {
        track.parent_guid = folder_stack.last().cloned();
        let depth = track.folder_depth;
        if depth > 0 {
            folder_stack.push(track.guid.clone());
        } else if depth < 0 {
            for _ in 0..depth.unsigned_abs() {
                folder_stack.pop();
            }
        }
    }
}

/// Insert a track in the current project, returning its guid. Used by
/// places that already hold a main-thread proof (no need to go through
/// the singleton trait).
pub fn add_track_on_main_thread(name: &str, at_index: Option<u32>) -> Option<String> {
    let proj = ReaperHigh::get().current_project();
    let index = at_index.unwrap_or_else(|| proj.track_count());
    let new_track = proj.insert_track_at(index).ok()?;
    new_track.set_name(name);
    Some(new_track.guid().to_string_without_braces())
}

/// Set REAPER's `I_FOLDERDEPTH` for a track in the current project.
///
/// This is for callers that already run on REAPER's main thread and need to
/// build folder structures immediately after inserting tracks.
pub fn set_folder_depth_on_main_thread(guid: &str, depth: i32) -> DawResult<()> {
    let proj = ReaperHigh::get().current_project();
    let track =
        resolve_track(&proj, &TrackRef::Guid(guid.to_string())).ok_or_else(not_found_track)?;
    let raw = track.raw().map_err(|_| not_found_track())?;
    unsafe {
        ReaperHigh::get()
            .medium_reaper()
            .set_media_track_info_value(raw, TrackAttributeKey::FolderDepth, depth as f64)
            .map_err(|err| DawError::operation_failed(format!("set folder depth failed: {err}")))?;
    }
    Ok(())
}

/// Max chunk size requested when reading a track's state chunk for a surgical
/// `BUSCOMP` edit. Track chunks (sans large embedded FX state) are small; 4 MiB
/// is generous headroom.
const FOLDER_CHUNK_MAX: u32 = 4 * 1024 * 1024;

/// Set folder-collapse ("compact") state independently for the arrange view and
/// the mixer.
///
/// - `arrange`: REAPER's `I_FOLDERCOMPACT` (0 = open, 1 = small, 2 = collapsed),
///   applied via the track-info API.
/// - `mixer`: the **second** field of the track's `BUSCOMP` chunk line
///   (`BUSCOMP <arrange> <mixer> <wiring> <x> <y>`), which the track-info API
///   does not expose — edited surgically in the state chunk so no other track
///   state is disturbed.
///
/// Either argument may be `None` to leave that surface untouched. Only
/// folder-parent tracks carry a `BUSCOMP` line, so the mixer edit is a logged
/// no-op on non-folder tracks.
pub fn set_folder_compact_on_main_thread(
    guid: &str,
    arrange: Option<i32>,
    mixer: Option<i32>,
) -> DawResult<()> {
    let proj = ReaperHigh::get().current_project();
    let track =
        resolve_track(&proj, &TrackRef::Guid(guid.to_string())).ok_or_else(not_found_track)?;
    let raw = track.raw().map_err(|_| not_found_track())?;

    if let Some(arrange) = arrange {
        unsafe {
            ReaperHigh::get()
                .medium_reaper()
                .set_media_track_info_value(raw, TrackAttributeKey::FolderCompact, arrange as f64)
                .map_err(|err| {
                    DawError::operation_failed(format!(
                        "set folder compact (arrange) failed: {err}"
                    ))
                })?;
        }
    }

    if let Some(mixer) = mixer {
        let chunk = track
            .chunk(FOLDER_CHUNK_MAX, ChunkCacheHint::NormalMode)
            .map_err(|e| DawError::operation_failed(format!("get track chunk: {e}")))?;
        let chunk_str = chunk.to_string();
        match rewrite_buscomp_mixer(&chunk_str, mixer) {
            Some(new_str) => {
                let new_chunk = reaper_high::Chunk::new(new_str);
                track
                    .set_chunk(new_chunk)
                    .map_err(|e| DawError::operation_failed(format!("set track chunk: {e}")))?;
            }
            None => {
                tracing::debug!(
                    guid,
                    "no BUSCOMP line in track chunk; skipped mixer folder-compact"
                );
            }
        }
    }

    Ok(())
}

/// Return a copy of `chunk_str` with the `BUSCOMP` line's mixer-collapse field
/// (2nd token) set to `mixer`, preserving the line's indentation and the other
/// fields. Returns `None` if the chunk has no `BUSCOMP` line.
fn rewrite_buscomp_mixer(chunk_str: &str, mixer: i32) -> Option<String> {
    let line = chunk_str
        .lines()
        .find(|l| l.trim_start().starts_with("BUSCOMP "))?;
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let rest = trimmed.strip_prefix("BUSCOMP ")?;
    let new_tokens: Vec<String> = rest
        .split_whitespace()
        .enumerate()
        .map(|(i, tok)| {
            if i == 1 {
                mixer.to_string()
            } else {
                tok.to_string()
            }
        })
        .collect();
    if new_tokens.len() < 2 {
        return None;
    }
    let new_line = format!("{indent}BUSCOMP {}", new_tokens.join(" "));
    Some(chunk_str.replacen(line, &new_line, 1))
}

/// Set track visibility in the current project.
///
/// This keeps local extension helpers working while the public track facade does
/// not expose visibility mutations.
pub fn set_visibility_on_main_thread(
    guid: &str,
    visible_in_tcp: bool,
    visible_in_mixer: bool,
) -> DawResult<()> {
    let proj = ReaperHigh::get().current_project();
    let track =
        resolve_track(&proj, &TrackRef::Guid(guid.to_string())).ok_or_else(not_found_track)?;
    let raw = track.raw().map_err(|_| not_found_track())?;
    let reaper = ReaperHigh::get();
    let medium = reaper.medium_reaper();
    unsafe {
        medium
            .set_media_track_info_value(
                raw,
                TrackAttributeKey::ShowInTcp,
                f64::from(visible_in_tcp),
            )
            .map_err(|err| {
                DawError::operation_failed(format!("set TCP visibility failed: {err}"))
            })?;
        medium
            .set_media_track_info_value(
                raw,
                TrackAttributeKey::ShowInMixer,
                f64::from(visible_in_mixer),
            )
            .map_err(|err| {
                DawError::operation_failed(format!("set mixer visibility failed: {err}"))
            })?;
    }
    Ok(())
}

/// Set a track's arrange-view height override in the current project.
///
/// REAPER uses `I_HEIGHTOVERRIDE = 0` to return the track to automatic height.
pub fn set_tcp_height_on_main_thread(guid: &str, height_pixels: u32) -> DawResult<()> {
    let proj = ReaperHigh::get().current_project();
    let track =
        resolve_track(&proj, &TrackRef::Guid(guid.to_string())).ok_or_else(not_found_track)?;
    let raw = track.raw().map_err(|_| not_found_track())?;
    let reaper = ReaperHigh::get();
    let medium = reaper.medium_reaper();
    unsafe {
        medium
            .set_media_track_info_value(
                raw,
                TrackAttributeKey::HeightOverride,
                height_pixels as f64,
            )
            .map_err(|err| DawError::operation_failed(format!("set TCP height failed: {err}")))?;
    }
    medium.track_list_adjust_windows_minor();
    Ok(())
}

fn reorder_behavior_to_reaper(behavior: ProtoReorderTracksBehavior) -> ReorderTracksBehavior {
    match behavior {
        ProtoReorderTracksBehavior::Normal => ReorderTracksBehavior::Normal,
        ProtoReorderTracksBehavior::MakeChildOfPreviousTrack => {
            ReorderTracksBehavior::MakeChildOfPreviousTrack
        }
        ProtoReorderTracksBehavior::ExtendFolder => ReorderTracksBehavior::ExtendFolder,
    }
}

fn record_input_to_raw(input: RecordInput) -> i32 {
    match input {
        RecordInput::None => -1,
        RecordInput::Midi { device_id, channel } => {
            const ALL_MIDI_DEVICES_ID: u32 = 63;
            let device_high = device_id.map(u32::from).unwrap_or(ALL_MIDI_DEVICES_ID);
            let channel_low = channel.map(|ch| u32::from(ch) + 1).unwrap_or(0);
            (4096 + (device_high * 32 + channel_low)) as i32
        }
        // REAPER `I_RECINPUT`: a mono hardware input on channel N is
        // encoded as N (0-based). Stereo / MIDI use higher ranges.
        RecordInput::Audio { channel } => channel as i32,
        RecordInput::Raw(value) => value,
    }
}

// ── Tracks impl ────────────────────────────────────────────────────────

fn not_found_proj() -> DawError {
    DawError::not_found("Project", "context")
}

fn not_found_track() -> DawError {
    DawError::not_found("Track", "")
}

/// REAPER track-group flag families. A "mutual" group member sets every
/// family's `_LEAD` and `_FOLLOW` bit so the whole group moves together.
const GROUP_FLAG_FAMILIES: &[&str] = &[
    "MEDIA_EDIT",
    "VOLUME",
    "VOLUME_VCA",
    "PAN",
    "WIDTH",
    "MUTE",
    "SOLO",
    "RECARM",
    "POLARITY",
    "AUTOMODE",
];

/// `GetSetTrackGroupMembershipEx` window offset + bit for a 1-based slot.
/// REAPER addresses the 128 slots as four 32-bit windows.
fn group_slot_window(slot: u32) -> (i32, u32) {
    let idx = slot - 1;
    (((idx / 32) * 32) as i32, 1u32 << (idx % 32))
}

impl Tracks for crate::Reaper {
    fn all(&self, project: ProjectContext) -> Vec<Track> {
        let Some(proj) = resolve_project(&project) else {
            return Vec::new();
        };
        let mut tracks: Vec<Track> = proj.tracks().map(|t| build_track_info(&t)).collect();
        assign_parent_guids(&mut tracks);
        tracks
    }

    fn get(&self, project: ProjectContext, track: TrackRef) -> Option<Track> {
        let proj = resolve_project(&project)?;
        let t = resolve_track(&proj, &track)?;
        Some(build_track_info(&t))
    }

    fn count(&self, project: ProjectContext) -> u32 {
        resolve_project(&project)
            .map(|p| p.track_count())
            .unwrap_or(0)
    }

    fn selected(&self, project: ProjectContext) -> Vec<Track> {
        let Some(proj) = resolve_project(&project) else {
            return Vec::new();
        };
        proj.tracks()
            .filter(|t| t.is_selected())
            .map(|t| build_track_info(&t))
            .collect()
    }

    fn master(&self, project: ProjectContext) -> Option<Track> {
        let proj = resolve_project(&project)?;
        proj.master_track().ok().as_ref().map(build_track_info)
    }

    fn set_automation_mode(
        &self,
        project: ProjectContext,
        track: TrackRef,
        mode: daw_proto::primitives::AutomationMode,
    ) -> DawResult<()> {
        use daw_proto::primitives::AutomationMode as P;
        use reaper_medium::AutomationMode as R;
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let mode = match mode {
            P::Off | P::TrimRead => R::TrimRead,
            P::Read => R::Read,
            P::Touch => R::Touch,
            P::Write => R::Write,
            P::Latch => R::Latch,
            P::LatchPreview => R::LatchPreview,
        };
        t.set_automation_mode(mode);
        Ok(())
    }

    fn set_input_monitor(
        &self,
        project: ProjectContext,
        track: TrackRef,
        monitor: daw_proto::track::InputMonitoringMode,
    ) -> DawResult<()> {
        use daw_proto::track::InputMonitoringMode as P;
        use reaper_medium::InputMonitoringMode as R;
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let mode = match monitor {
            P::Off => R::Off,
            P::Normal => R::Normal,
            P::NotWhenPlaying => R::NotWhenPlaying,
        };
        t.set_input_monitoring_mode(
            mode,
            GangBehavior::DenyGang,
            GroupingBehavior::PreventGrouping,
        );
        Ok(())
    }

    fn set_phase_inverted(
        &self,
        project: ProjectContext,
        track: TrackRef,
        inverted: bool,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        t.set_phase_inverted(
            if inverted {
                TrackPolarity::Inverted
            } else {
                TrackPolarity::Normal
            },
            GangBehavior::DenyGang,
            GroupingBehavior::PreventGrouping,
        );
        Ok(())
    }

    fn set_muted(&self, project: ProjectContext, track: TrackRef, muted: bool) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        if muted {
            t.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        } else {
            t.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        }
        Ok(())
    }

    fn set_soloed(&self, project: ProjectContext, track: TrackRef, soloed: bool) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        if soloed {
            t.solo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        } else {
            t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        }
        Ok(())
    }

    fn set_solo_exclusive(&self, project: ProjectContext, track: TrackRef) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        for t in proj.tracks() {
            if t.is_solo() {
                t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
        }
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        t.solo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        Ok(())
    }

    fn clear_all_solo(&self, project: ProjectContext) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        for t in proj.tracks() {
            if t.is_solo() {
                t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
        }
        Ok(())
    }

    fn set_armed(&self, project: ProjectContext, track: TrackRef, armed: bool) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
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
        Ok(())
    }

    fn set_volume(&self, project: ProjectContext, track: TrackRef, volume: f64) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let val = reaper_medium::ReaperVolumeValue::new(volume)
            .map_err(|e| DawError::operation_failed(format!("invalid volume: {e:?}")))?;
        let _ = t.set_volume_smart(val, Default::default());
        Ok(())
    }

    fn set_pan(&self, project: ProjectContext, track: TrackRef, pan: f64) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let val = reaper_medium::ReaperPanValue::new_panic(pan.clamp(-1.0, 1.0));
        let _ = t.set_pan_smart(val, Default::default());
        Ok(())
    }

    fn set_group_name(&self, project: ProjectContext, slot: u32, name: &str) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let low = ReaperHigh::get().medium_reaper().low();
        let desc = std::ffi::CString::new(format!("TRACK_GROUP_NAME:{slot}"))
            .map_err(|e| DawError::operation_failed(format!("bad slot desc: {e}")))?;
        let value = std::ffi::CString::new(name)
            .map_err(|e| DawError::operation_failed(format!("bad group name: {e}")))?;
        let mut buf = value.into_bytes_with_nul();
        let ok = unsafe {
            low.GetSetProjectInfo_String(
                proj.raw().as_ptr(),
                desc.as_ptr(),
                buf.as_mut_ptr() as *mut std::os::raw::c_char,
                true, // is_set
            )
        };
        if ok {
            Ok(())
        } else {
            Err(DawError::operation_failed(format!(
                "set TRACK_GROUP_NAME:{slot} failed"
            )))
        }
    }

    fn first_free_group_slot(
        &self,
        project: ProjectContext,
        band_start: u32,
        band_end: u32,
    ) -> Option<u32> {
        let proj = resolve_project(&project)?;
        let low = ReaperHigh::get().medium_reaper().low();
        // A slot is "in use" if any track carries its VCA-lead bit.
        let probe = std::ffi::CString::new("VOLUME_VCA_LEAD").ok()?;
        let tracks: Vec<_> = proj.tracks().filter_map(|t| t.raw().ok()).collect();
        'slots: for slot in band_start..=band_end {
            let (offset, mask) = group_slot_window(slot);
            for t in &tracks {
                let state = unsafe {
                    low.GetSetTrackGroupMembershipEx(t.as_ptr(), probe.as_ptr(), offset, 0, 0)
                };
                if state & mask != 0 {
                    continue 'slots;
                }
            }
            return Some(slot);
        }
        None
    }

    fn set_group_membership(
        &self,
        project: ProjectContext,
        track: TrackRef,
        slot: u32,
        member: bool,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let raw = t.raw().map_err(|_| not_found_track())?;
        let low = ReaperHigh::get().medium_reaper().low();
        let (offset, mask) = group_slot_window(slot);
        let value = if member { mask } else { 0 };
        for fam in GROUP_FLAG_FAMILIES {
            for suffix in ["_LEAD", "_FOLLOW"] {
                if let Ok(name) = std::ffi::CString::new(format!("{fam}{suffix}")) {
                    unsafe {
                        low.GetSetTrackGroupMembershipEx(
                            raw.as_ptr(),
                            name.as_ptr(),
                            offset,
                            mask,
                            value,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn set_selected(
        &self,
        project: ProjectContext,
        track: TrackRef,
        selected: bool,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        if selected {
            t.select();
        } else {
            t.unselect();
        }
        Ok(())
    }

    fn select_exclusive(&self, project: ProjectContext, track: TrackRef) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        t.select_exclusively();
        Ok(())
    }

    fn clear_selection(&self, project: ProjectContext) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        for t in proj.tracks() {
            if t.is_selected() {
                t.unselect();
            }
        }
        Ok(())
    }

    fn mute_all(&self, project: ProjectContext) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        for t in proj.tracks() {
            if !t.is_muted() {
                t.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
        }
        Ok(())
    }

    fn unmute_all(&self, project: ProjectContext) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        for t in proj.tracks() {
            if t.is_muted() {
                t.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
        }
        Ok(())
    }

    fn add(&self, project: ProjectContext, name: &str, at_index: Option<u32>) -> DawResult<String> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let index = at_index.unwrap_or_else(|| proj.track_count());
        let new_track = proj
            .insert_track_at(index)
            .map_err(|e| DawError::operation_failed(format!("insert_track_at failed: {e:?}")))?;
        new_track.set_name(name);
        Ok(new_track.guid().to_string_without_braces())
    }

    fn remove(&self, project: ProjectContext, track: TrackRef) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        proj.remove_track(&t);
        Ok(())
    }

    fn remove_all(&self, project: ProjectContext) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let count = proj.track_count();
        for i in (0..count).rev() {
            if let Some(t) = proj.track_by_index(i) {
                proj.remove_track(&t);
            }
        }
        Ok(())
    }

    fn rename(&self, project: ProjectContext, track: TrackRef, name: &str) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        t.set_name(name);
        Ok(())
    }

    fn set_color(&self, project: ProjectContext, track: TrackRef, color: u32) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        if color == 0 {
            t.set_custom_color(None);
        } else {
            let r = ((color >> 16) & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let b = (color & 0xFF) as u8;
            t.set_custom_color(Some(reaper_medium::RgbColor::rgb(r, g, b)));
        }
        Ok(())
    }

    fn set_folder_depth(
        &self,
        project: ProjectContext,
        track: TrackRef,
        folder_depth: i32,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let raw = t.raw().map_err(|_| not_found_track())?;
        unsafe {
            ReaperHigh::get()
                .medium_reaper()
                .set_media_track_info_value(
                    raw,
                    TrackAttributeKey::FolderDepth,
                    folder_depth as f64,
                )
                .map_err(|err| {
                    DawError::operation_failed(format!("set folder depth failed: {err}"))
                })?;
        }
        Ok(())
    }

    fn set_num_channels(
        &self,
        project: ProjectContext,
        track: TrackRef,
        num_channels: u32,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let raw = t.raw().map_err(|_| not_found_track())?;
        let num_channels = num_channels.max(2);
        unsafe {
            ReaperHigh::get()
                .medium_reaper()
                .set_media_track_info_value(raw, TrackAttributeKey::Nchan, num_channels as f64)
                .map_err(|err| {
                    DawError::operation_failed(format!("set track channel count failed: {err}"))
                })?;
        }
        Ok(())
    }

    fn set_record_input(
        &self,
        project: ProjectContext,
        track: TrackRef,
        input: RecordInput,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let raw = t.raw().map_err(|_| not_found_track())?;
        unsafe {
            ReaperHigh::get()
                .medium_reaper()
                .set_media_track_info_value(
                    raw,
                    TrackAttributeKey::RecInput,
                    record_input_to_raw(input) as f64,
                )
                .map_err(|err| {
                    DawError::operation_failed(format!("set record input failed: {err}"))
                })?;
        }
        Ok(())
    }

    fn reorder_selected(
        &self,
        project: ProjectContext,
        index: u32,
        behavior: ProtoReorderTracksBehavior,
    ) -> DawResult<()> {
        let _proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        ReaperHigh::get()
            .medium_reaper()
            .reorder_selected_tracks(index, reorder_behavior_to_reaper(behavior))
            .map_err(|err| DawError::operation_failed(format!("reorder selected failed: {err}")))?;
        ReaperHigh::get()
            .medium_reaper()
            .track_list_adjust_windows_minor();
        Ok(())
    }

    fn set_visibility(
        &self,
        project: ProjectContext,
        track: TrackRef,
        visible_in_tcp: bool,
        visible_in_mixer: bool,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let raw = t.raw().map_err(|_| not_found_track())?;
        let medium = ReaperHigh::get().medium_reaper();
        unsafe {
            medium
                .set_media_track_info_value(
                    raw,
                    TrackAttributeKey::ShowInTcp,
                    f64::from(visible_in_tcp),
                )
                .map_err(|err| {
                    DawError::operation_failed(format!("set TCP visibility failed: {err}"))
                })?;
            medium
                .set_media_track_info_value(
                    raw,
                    TrackAttributeKey::ShowInMixer,
                    f64::from(visible_in_mixer),
                )
                .map_err(|err| {
                    DawError::operation_failed(format!("set mixer visibility failed: {err}"))
                })?;
        }
        Ok(())
    }

    fn set_tcp_height(
        &self,
        project: ProjectContext,
        track: TrackRef,
        height_pixels: u32,
    ) -> DawResult<()> {
        let proj = resolve_project(&project).ok_or_else(not_found_proj)?;
        let t = resolve_track(&proj, &track).ok_or_else(not_found_track)?;
        let raw = t.raw().map_err(|_| not_found_track())?;
        let medium = ReaperHigh::get().medium_reaper();
        unsafe {
            medium
                .set_media_track_info_value(
                    raw,
                    TrackAttributeKey::HeightOverride,
                    height_pixels as f64,
                )
                .map_err(|err| {
                    DawError::operation_failed(format!("set TCP height failed: {err}"))
                })?;
        }
        medium.track_list_adjust_windows_minor();
        Ok(())
    }
}

impl daw_proto::track::TracksStreamSource for crate::Reaper {
    fn events_hub(&self) -> &architect::PubSub<TrackStreamEvent> {
        crate::event_hub::hub().tracks_hub()
    }
}

// `Project` is used only for the `Project` re-export visibility check
// inside `resolve_project`; quiet the unused-import lint when no
// methods reference the bare name.
#[allow(dead_code)]
fn _force_project_in_scope(_: &Project) {}

// ── Streaming: poll + broadcast tracks ────────────────────────────────
//
// Coarse for Phase 2: emit Added / Removed only. Per-field change
// events (MuteChanged, VolumeChanged, …) need per-field diff logic
// that we'll wire alongside the synchronization engine when we know
// exactly which fields the engine cares about.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use daw_proto::TrackEvent;
use daw_proto::track::TrackStreamEvent;
use reaper_medium::ProjectRef;

use crate::project_context::MAX_PROJECT_TABS;

static TRACK_CACHE: OnceLock<Mutex<HashMap<String, HashMap<String, Track>>>> = OnceLock::new();

/// Shared per-project track cache used by `poll_and_broadcast_tracks`.
/// Exposed to `control_surface.rs` so push-based callbacks can update
/// the cached field before publishing, preventing the next poll tick
/// from re-emitting the same diff. Main-thread access only.
pub(crate) fn track_cache() -> &'static Mutex<HashMap<String, HashMap<String, Track>>> {
    TRACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Poll REAPER track state for every open project. **Main thread only.**
pub fn poll_and_broadcast_tracks() {
    let hub = crate::event_hub::hub();
    if hub.tracks_subscriber_count() == 0 {
        return;
    }

    let reaper = ReaperHigh::get();
    let medium = reaper.medium_reaper();
    let mut cache = track_cache().lock().expect("track cache mutex poisoned");

    let mut seen_projects: Vec<String> = Vec::new();

    for tab_index in 0..MAX_PROJECT_TABS {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            break;
        };
        let project = Project::new(result.project);
        let project_guid_str = project_guid(&project);
        seen_projects.push(project_guid_str.clone());

        let project_ctx = ProjectContext::Project(project_guid_str.clone());
        let fresh: Vec<Track> = Tracks::all(&crate::Reaper, project_ctx);
        let fresh_by_guid: HashMap<String, Track> =
            fresh.into_iter().map(|t| (t.guid.clone(), t)).collect();

        let prev = cache.entry(project_guid_str.clone()).or_default();

        let publish = |event: TrackEvent| {
            hub.publish_track(TrackStreamEvent {
                project_guid: project_guid_str.clone(),
                event,
            });
        };

        for (guid, track) in &fresh_by_guid {
            match prev.get(guid) {
                None => publish(TrackEvent::Added(track.clone())),
                Some(p) => {
                    if p.name != track.name {
                        publish(TrackEvent::Renamed {
                            guid: guid.clone(),
                            name: track.name.clone(),
                        });
                    }
                    if p.muted != track.muted {
                        publish(TrackEvent::MuteChanged {
                            guid: guid.clone(),
                            muted: track.muted,
                        });
                    }
                    if p.soloed != track.soloed {
                        publish(TrackEvent::SoloChanged {
                            guid: guid.clone(),
                            soloed: track.soloed,
                        });
                    }
                    if p.armed != track.armed {
                        publish(TrackEvent::ArmChanged {
                            guid: guid.clone(),
                            armed: track.armed,
                        });
                    }
                    if p.selected != track.selected {
                        publish(TrackEvent::SelectionChanged {
                            guid: guid.clone(),
                            selected: track.selected,
                        });
                    }
                    if (p.volume - track.volume).abs() > f64::EPSILON {
                        publish(TrackEvent::VolumeChanged {
                            guid: guid.clone(),
                            volume: track.volume,
                        });
                    }
                    if (p.pan - track.pan).abs() > f64::EPSILON {
                        publish(TrackEvent::PanChanged {
                            guid: guid.clone(),
                            pan: track.pan,
                        });
                    }
                    if p.color != track.color {
                        publish(TrackEvent::ColorChanged {
                            guid: guid.clone(),
                            color: track.color,
                        });
                    }
                    if p.index != track.index {
                        publish(TrackEvent::Moved {
                            guid: guid.clone(),
                            old_index: p.index,
                            new_index: track.index,
                        });
                    }
                }
            }
        }
        for guid in prev.keys() {
            if !fresh_by_guid.contains_key(guid) {
                publish(TrackEvent::Removed(guid.clone()));
            }
        }

        *prev = fresh_by_guid;
    }

    cache.retain(|guid, _| seen_projects.iter().any(|seen| seen == guid));
}

// ── Tracks::subscribe impl ─────────────────────────────────────────────
