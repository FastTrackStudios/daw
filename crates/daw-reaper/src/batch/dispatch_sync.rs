//! Synchronous dispatch — runs directly on REAPER's main thread.
//!
//! Each function in this module calls REAPER APIs directly (via reaper_high)
//! rather than going through async service methods + main_thread::query().
//! This eliminates the ~33ms timer callback latency per operation.
//!
//! Called from `BatchExecutor::execute_sync()` inside a single
//! `main_thread::query()` closure.

use super::resolve::{resolve_project_arg, resolve_track_arg};
use crate::project::project_to_info;
use crate::track::{assign_parent_guids, build_track_info, resolve_project, resolve_track};
use daw_proto::batch::*;
use daw_proto::*;
use reaper_high::{GroupingBehavior, PlayRate, Reaper, Tempo as ReaperTempo};
use reaper_medium::{
    CommandId, GangBehavior, MarkerOrRegionPosition, PositionInSeconds,
    ProjectContext as ReaperProjectContext, SetEditCurPosOptions, UndoBehavior,
};

/// Dispatch a single batch operation synchronously on the main thread.
pub fn dispatch_op_sync(
    op: &BatchOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    match op {
        BatchOp::Project(op) => dispatch_project_sync(op, outputs),
        BatchOp::Transport(op) => dispatch_transport_sync(op, outputs),
        BatchOp::Track(op) => dispatch_track_sync(op, outputs),
        BatchOp::Marker(op) => dispatch_marker_sync(op, outputs),
        BatchOp::Region(op) => dispatch_region_sync(op, outputs),
        BatchOp::ExtState(op) => dispatch_ext_state_sync(op, outputs),
        BatchOp::Health(op) => dispatch_health_sync(op),
        // Services not yet sync-optimized — return error so caller falls back
        _ => Err(format!(
            "sync dispatch not implemented for {:?}",
            std::mem::discriminant(op)
        )),
    }
}

/// Returns true if the given op is supported by sync dispatch.
pub fn is_sync_supported(op: &BatchOp) -> bool {
    matches!(
        op,
        BatchOp::Project(_)
            | BatchOp::Transport(_)
            | BatchOp::Track(_)
            | BatchOp::Marker(_)
            | BatchOp::Region(_)
            | BatchOp::ExtState(_)
            | BatchOp::Health(_)
    )
}

// =============================================================================
// Helpers
// =============================================================================

/// Resolve project arg and get the reaper_high::Project.
fn proj(p: &ProjectArg, outputs: &[Option<StepOutput>]) -> Result<reaper_high::Project, String> {
    let ctx = resolve_project_arg(p, outputs)?;
    resolve_project(&ctx).ok_or_else(|| "project not found".to_string())
}

/// Resolve project + track args.
fn proj_track(
    p: &ProjectArg,
    t: &TrackArg,
    outputs: &[Option<StepOutput>],
) -> Result<(reaper_high::Project, reaper_high::Track), String> {
    let project = proj(p, outputs)?;
    let tref = resolve_track_arg(t, outputs)?;
    let track = resolve_track(&project, &tref).ok_or_else(|| "track not found".to_string())?;
    Ok((project, track))
}

// =============================================================================
// Project dispatch
// =============================================================================

fn dispatch_project_sync(
    op: &ProjectOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        ProjectOp::GetCurrent => {
            let project = reaper.current_project();
            Ok(StepOutput::OptProjectInfo(Some(project_to_info(&project))))
        }
        ProjectOp::Get(id) => {
            let project = crate::project_context::find_project_by_guid(id);
            Ok(StepOutput::OptProjectInfo(
                project.as_ref().map(project_to_info),
            ))
        }
        ProjectOp::List => {
            use crate::project_context::MAX_PROJECT_TABS;
            let mut list = Vec::new();
            for tab in 0..MAX_PROJECT_TABS {
                if let Some(result) = medium.enum_projects(reaper_medium::ProjectRef::Tab(tab), 0) {
                    let p = reaper_high::Project::new(result.project);
                    list.push(project_to_info(&p));
                } else {
                    break;
                }
            }
            Ok(StepOutput::ProjectInfoList(list))
        }
        ProjectOp::Create => {
            // Action 41929 = "New project tab"
            medium.main_on_command_ex(
                CommandId::new(41929),
                0,
                ReaperProjectContext::CurrentProject,
            );
            let project = reaper.current_project();
            Ok(StepOutput::OptProjectInfo(Some(project_to_info(&project))))
        }
        ProjectOp::BeginUndoBlock(p, _label) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let rctx = crate::project_context::resolve_project_context(&ctx);
            medium.undo_begin_block_2(rctx);
            Ok(StepOutput::Unit)
        }
        ProjectOp::EndUndoBlock(p, label, scope) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let rctx = crate::project_context::resolve_project_context(&ctx);
            let reaper_scope = scope
                .as_ref()
                .map(|s| crate::project::convert_undo_scope(s))
                .unwrap_or(reaper_medium::UndoScope::All);
            medium.undo_end_block_2(rctx, label.as_str(), reaper_scope);
            Ok(StepOutput::Unit)
        }
        ProjectOp::RunCommand(p, cmd) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let raw_proj = resolve_project(&ctx).map(|p| p.raw());
            let rctx = raw_proj
                .map(ReaperProjectContext::Proj)
                .unwrap_or(ReaperProjectContext::CurrentProject);
            // Try numeric first, then named
            if let Ok(id) = cmd.parse::<u32>() {
                medium.main_on_command_ex(CommandId::new(id), 0, rctx);
                Ok(StepOutput::Bool(true))
            } else if let Some(cmd_id) = medium.named_command_lookup(cmd.as_str()) {
                medium.main_on_command_ex(cmd_id, 0, rctx);
                Ok(StepOutput::Bool(true))
            } else {
                Ok(StepOutput::Bool(false))
            }
        }
        ProjectOp::Save(p) => {
            let _project = proj(p, outputs)?;
            // Action 40026 = "Save project"
            medium.main_on_command_ex(
                CommandId::new(40026),
                0,
                ReaperProjectContext::CurrentProject,
            );
            Ok(StepOutput::Unit)
        }
        ProjectOp::SaveAll => {
            medium.main_on_command_ex(
                CommandId::new(40108),
                0,
                ReaperProjectContext::CurrentProject,
            );
            Ok(StepOutput::Unit)
        }
        ProjectOp::Undo(p) => {
            let project = proj(p, outputs)?;
            Ok(StepOutput::Bool(project.undo()))
        }
        ProjectOp::Redo(p) => {
            let project = proj(p, outputs)?;
            Ok(StepOutput::Bool(project.redo()))
        }
        _ => Err("sync dispatch not implemented for this ProjectOp".to_string()),
    }
}

// =============================================================================
// Transport dispatch
// =============================================================================

fn dispatch_transport_sync(
    op: &TransportOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    /// Helper: fire a REAPER action command by numeric ID.
    fn fire_cmd(medium: &reaper_medium::Reaper, id: u32) {
        medium.main_on_command_ex(CommandId::new(id), 0, ReaperProjectContext::CurrentProject);
    }

    match op {
        TransportOp::Play(_p) => {
            fire_cmd(medium, 1007);
            Ok(StepOutput::Unit)
        }
        TransportOp::Pause(_p) => {
            fire_cmd(medium, 1008);
            Ok(StepOutput::Unit)
        }
        TransportOp::Stop(_p) => {
            fire_cmd(medium, 1016);
            Ok(StepOutput::Unit)
        }
        TransportOp::PlayPause(_p) => {
            fire_cmd(medium, 40073);
            Ok(StepOutput::Unit)
        }
        TransportOp::PlayStop(_p) => {
            fire_cmd(medium, 40044);
            Ok(StepOutput::Unit)
        }
        TransportOp::PlayFromLastStartPosition(_p) => {
            fire_cmd(medium, 1007); // same as Play
            Ok(StepOutput::Unit)
        }
        TransportOp::Record(_p) => {
            fire_cmd(medium, 1013);
            Ok(StepOutput::Unit)
        }
        TransportOp::StopRecording(_p) => {
            fire_cmd(medium, 1016);
            Ok(StepOutput::Unit)
        }
        TransportOp::ToggleRecording(_p) => {
            fire_cmd(medium, 1013);
            Ok(StepOutput::Unit)
        }
        TransportOp::SetPosition(_p, secs) => {
            if let Ok(pos) = PositionInSeconds::new(*secs) {
                reaper.current_project().set_edit_cursor_position(
                    pos,
                    SetEditCurPosOptions {
                        move_view: false,
                        seek_play: true,
                    },
                );
            }
            Ok(StepOutput::Unit)
        }
        TransportOp::GetPosition(_p) => {
            let pos = reaper
                .current_project()
                .play_or_edit_cursor_position()
                .map(|p| p.get())
                .unwrap_or(0.0);
            Ok(StepOutput::F64(pos))
        }
        TransportOp::GotoStart(_p) => {
            fire_cmd(medium, 40042);
            Ok(StepOutput::Unit)
        }
        TransportOp::GotoEnd(_p) => {
            fire_cmd(medium, 40043);
            Ok(StepOutput::Unit)
        }
        TransportOp::GetState(p) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let project = resolve_project(&ctx).ok_or_else(|| "project not found".to_string())?;
            let rctx = match &ctx {
                ProjectContext::Current => ReaperProjectContext::CurrentProject,
                ProjectContext::Project(_) => ReaperProjectContext::Proj(project.raw()),
            };
            Ok(StepOutput::Transport(
                crate::transport::read_transport_state_for_project(&project, rctx, medium),
            ))
        }
        TransportOp::GetPlayState(_p) => {
            let state = medium.get_play_state_ex(ReaperProjectContext::CurrentProject);
            let play_state = if state.is_recording {
                PlayState::Recording
            } else if state.is_paused {
                PlayState::Paused
            } else if state.is_playing {
                PlayState::Playing
            } else {
                PlayState::Stopped
            };
            Ok(StepOutput::PlayState(play_state))
        }
        TransportOp::IsPlaying(_p) => {
            let state = medium.get_play_state_ex(ReaperProjectContext::CurrentProject);
            Ok(StepOutput::Bool(state.is_playing || state.is_recording))
        }
        TransportOp::IsRecording(_p) => {
            let state = medium.get_play_state_ex(ReaperProjectContext::CurrentProject);
            Ok(StepOutput::Bool(state.is_recording))
        }
        TransportOp::GetTempo(_p) => Ok(StepOutput::F64(
            reaper.current_project().tempo().bpm().get(),
        )),
        TransportOp::SetTempo(_p, bpm) => {
            if let Ok(bpm_value) = reaper_medium::Bpm::new(*bpm) {
                let tempo = ReaperTempo::from_bpm(bpm_value);
                let _ = reaper
                    .current_project()
                    .set_tempo(tempo, UndoBehavior::OmitUndoPoint);
            }
            Ok(StepOutput::Unit)
        }
        TransportOp::ToggleLoop(_p) => {
            fire_cmd(medium, 1068);
            Ok(StepOutput::Unit)
        }
        TransportOp::IsLooping(_p) => Ok(StepOutput::Bool(
            medium.get_set_repeat_ex_get(ReaperProjectContext::CurrentProject),
        )),
        TransportOp::SetLoop(_p, enabled) => {
            medium.get_set_repeat_ex_set(ReaperProjectContext::CurrentProject, *enabled);
            Ok(StepOutput::Unit)
        }
        TransportOp::GetPlayrate(_p) => Ok(StepOutput::F64(
            reaper
                .current_project()
                .play_rate()
                .playback_speed_factor()
                .get(),
        )),
        TransportOp::SetPlayrate(_p, rate) => {
            let clamped = rate.clamp(0.25, 4.0);
            let factor = reaper_medium::PlaybackSpeedFactor::new(clamped);
            let play_rate = PlayRate::from_playback_speed_factor(factor);
            reaper.current_project().set_play_rate(play_rate);
            Ok(StepOutput::Unit)
        }
        TransportOp::GetTimeSignature(_p) => {
            let pos = medium.get_play_position_ex(ReaperProjectContext::CurrentProject);
            let ts = medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
            Ok(StepOutput::TimeSignature(TimeSignature::new(
                ts.time_signature.numerator.get() as u32,
                ts.time_signature.denominator.get() as u32,
            )))
        }
        TransportOp::SetPositionMusical(_p, measure, beat, subdivision) => {
            let beats_within_measure = *beat as f64 + *subdivision as f64 / 1000.0;
            if let Ok(beats) = reaper_medium::PositionInBeats::new(beats_within_measure) {
                let time_seconds = medium.time_map_2_beats_to_time(
                    ReaperProjectContext::CurrentProject,
                    reaper_medium::MeasureMode::FromMeasureAtIndex(*measure),
                    beats,
                );
                if let Ok(pos) = PositionInSeconds::new(time_seconds.get()) {
                    reaper.current_project().set_edit_cursor_position(
                        pos,
                        SetEditCurPosOptions {
                            move_view: false,
                            seek_play: true,
                        },
                    );
                }
            }
            Ok(StepOutput::Unit)
        }
        TransportOp::GotoMeasure(_p, measure) => {
            let beats = reaper_medium::PositionInBeats::new(0.0).unwrap();
            let time_seconds = medium.time_map_2_beats_to_time(
                ReaperProjectContext::CurrentProject,
                reaper_medium::MeasureMode::FromMeasureAtIndex(*measure),
                beats,
            );
            if let Ok(pos) = PositionInSeconds::new(time_seconds.get()) {
                reaper.current_project().set_edit_cursor_position(
                    pos,
                    SetEditCurPosOptions {
                        move_view: false,
                        seek_play: true,
                    },
                );
            }
            Ok(StepOutput::Unit)
        }
    }
}

// =============================================================================
// Track dispatch
// =============================================================================

fn dispatch_track_sync(op: &TrackOp, outputs: &[Option<StepOutput>]) -> Result<StepOutput, String> {
    match op {
        TrackOp::GetTracks(p) => {
            let project = proj(p, outputs)?;
            let mut tracks: Vec<Track> = (0..project.track_count())
                .filter_map(|i| project.track_by_index(i))
                .map(|t| build_track_info(&t))
                .collect();
            assign_parent_guids(&mut tracks);
            Ok(StepOutput::TrackList(tracks))
        }
        TrackOp::GetTrack(p, t) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let tref = resolve_track_arg(t, outputs)?;
            let track = resolve_project(&ctx)
                .and_then(|pr| resolve_track(&pr, &tref))
                .map(|t| build_track_info(&t));
            Ok(StepOutput::OptTrack(track))
        }
        TrackOp::TrackCount(p) => {
            let project = proj(p, outputs)?;
            Ok(StepOutput::U32(project.track_count()))
        }
        TrackOp::GetSelectedTracks(p) => {
            let project = proj(p, outputs)?;
            let mut tracks: Vec<Track> = (0..project.track_count())
                .filter_map(|i| project.track_by_index(i))
                .filter(|t| t.is_selected())
                .map(|t| build_track_info(&t))
                .collect();
            assign_parent_guids(&mut tracks);
            Ok(StepOutput::TrackList(tracks))
        }
        TrackOp::GetMasterTrack(p) => {
            let project = proj(p, outputs)?;
            let master = project.master_track().ok().map(|t| build_track_info(&t));
            Ok(StepOutput::OptTrack(master))
        }
        TrackOp::AddTrack(p, name, at) => {
            let project = proj(p, outputs)?;
            let index = at.unwrap_or_else(|| project.track_count());
            let new_track = project
                .insert_track_at(index)
                .map_err(|e| format!("insert_track_at: {e}"))?;
            new_track.set_name(name.as_str());
            Ok(StepOutput::Str(new_track.guid().to_string_without_braces()))
        }
        TrackOp::RemoveTrack(p, t) => {
            let (project, track) = proj_track(p, t, outputs)?;
            project.remove_track(&track);
            Ok(StepOutput::Unit)
        }
        TrackOp::RenameTrack(p, t, name) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            track.set_name(name.as_str());
            Ok(StepOutput::Unit)
        }
        TrackOp::SetTrackColor(p, t, color) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            if *color == 0 {
                track.set_custom_color(None);
            } else {
                let r = ((color >> 16) & 0xFF) as u8;
                let g = ((color >> 8) & 0xFF) as u8;
                let b = (color & 0xFF) as u8;
                track.set_custom_color(Some(reaper_medium::RgbColor::rgb(r, g, b)));
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetMuted(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            if *v {
                track.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            } else {
                track.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetSoloed(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            if *v {
                track.solo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            } else {
                track.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetArmed(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            if *v {
                track.arm(
                    false,
                    GangBehavior::DenyGang,
                    GroupingBehavior::PreventGrouping,
                );
            } else {
                track.disarm(
                    false,
                    GangBehavior::DenyGang,
                    GroupingBehavior::PreventGrouping,
                );
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetVolume(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            if let Ok(val) = reaper_medium::ReaperVolumeValue::new(*v) {
                let _ = track.set_volume_smart(val, Default::default());
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetPan(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            let val = reaper_medium::ReaperPanValue::new_panic(v.clamp(-1.0, 1.0));
            let _ = track.set_pan_smart(val, Default::default());
            Ok(StepOutput::Unit)
        }
        TrackOp::SetSelected(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            if *v {
                track.select();
            } else {
                track.unselect();
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::ClearSelection(p) => {
            let project = proj(p, outputs)?;
            for i in 0..project.track_count() {
                if let Some(t) = project.track_by_index(i) {
                    if t.is_selected() {
                        t.unselect();
                    }
                }
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::MuteAll(p) => {
            let project = proj(p, outputs)?;
            for i in 0..project.track_count() {
                if let Some(t) = project.track_by_index(i) {
                    if !t.is_muted() {
                        t.mute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                    }
                }
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::UnmuteAll(p) => {
            let project = proj(p, outputs)?;
            for i in 0..project.track_count() {
                if let Some(t) = project.track_by_index(i) {
                    if t.is_muted() {
                        t.unmute(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                    }
                }
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetVisibleInTcp(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            track.set_shown(reaper_medium::TrackArea::Tcp, *v);
            Ok(StepOutput::Unit)
        }
        TrackOp::SetVisibleInMixer(p, t, v) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            track.set_shown(reaper_medium::TrackArea::Mcp, *v);
            Ok(StepOutput::Unit)
        }
        TrackOp::RemoveAllTracks(p) => {
            let project = proj(p, outputs)?;
            let count = project.track_count();
            for i in (0..count).rev() {
                if let Some(t) = project.track_by_index(i) {
                    project.remove_track(&t);
                }
            }
            Ok(StepOutput::Unit)
        }
        TrackOp::SetExtState(p, t, req) => {
            let (_project, track) = proj_track(p, t, outputs)?;
            let raw = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let c_key = std::ffi::CString::new(format!("P_EXT:{}/{}", req.section, req.key))
                .map_err(|e| format!("CString: {e}"))?;
            let c_val =
                std::ffi::CString::new(req.value.as_str()).map_err(|e| format!("CString: {e}"))?;
            unsafe {
                Reaper::get()
                    .medium_reaper()
                    .low()
                    .GetSetMediaTrackInfo_String(
                        raw.as_ptr(),
                        c_key.as_ptr(),
                        c_val.as_ptr() as *mut _,
                        true,
                    );
            }
            Ok(StepOutput::Unit)
        }
        _ => Err("sync dispatch not implemented for this TrackOp".to_string()),
    }
}

// =============================================================================
// Marker dispatch
// =============================================================================

fn dispatch_marker_sync(
    op: &MarkerOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        MarkerOp::AddMarker(p, pos, name) => {
            let _project = proj(p, outputs)?;
            if let Ok(pos) = PositionInSeconds::new(*pos) {
                let idx = medium
                    .add_project_marker_2(
                        ReaperProjectContext::CurrentProject,
                        MarkerOrRegionPosition::Marker(pos),
                        name.as_str(),
                        None,
                        None,
                    )
                    .unwrap_or(0);
                Ok(StepOutput::U32(idx as u32))
            } else {
                Ok(StepOutput::U32(0))
            }
        }
        MarkerOp::RemoveMarker(p, id) => {
            let _project = proj(p, outputs)?;
            let low = medium.low();
            crate::safe_wrappers::markers::delete_project_marker(
                low,
                ReaperProjectContext::CurrentProject,
                *id as i32,
                false,
            );
            Ok(StepOutput::Unit)
        }
        MarkerOp::MoveMarker(p, id, position) => {
            let _project = proj(p, outputs)?;
            let low = medium.low();
            crate::safe_wrappers::markers::set_project_marker(
                low, *id as i32, false, *position, 0.0, None,
            );
            Ok(StepOutput::Unit)
        }
        MarkerOp::RenameMarker(p, id, name) => {
            let _project = proj(p, outputs)?;
            let low = medium.low();
            let c_name =
                std::ffi::CString::new(name.as_str()).map_err(|e| format!("CString: {e}"))?;
            crate::safe_wrappers::markers::set_project_marker(
                low,
                *id as i32,
                false,
                -1.0, // keep position
                0.0,
                Some(&c_name),
            );
            Ok(StepOutput::Unit)
        }
        _ => Err("sync dispatch not implemented for this MarkerOp".to_string()),
    }
}

// =============================================================================
// Region dispatch
// =============================================================================

fn dispatch_region_sync(
    op: &RegionOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        RegionOp::AddRegion(p, start, end, name) => {
            let _project = proj(p, outputs)?;
            if let (Ok(start_pos), Ok(end_pos)) =
                (PositionInSeconds::new(*start), PositionInSeconds::new(*end))
            {
                let idx = medium
                    .add_project_marker_2(
                        ReaperProjectContext::CurrentProject,
                        MarkerOrRegionPosition::Region(start_pos, end_pos),
                        name.as_str(),
                        None,
                        None,
                    )
                    .unwrap_or(0);
                Ok(StepOutput::U32(idx as u32))
            } else {
                Ok(StepOutput::U32(0))
            }
        }
        RegionOp::RemoveRegion(p, id) => {
            let _project = proj(p, outputs)?;
            let low = medium.low();
            crate::safe_wrappers::markers::delete_project_marker(
                low,
                ReaperProjectContext::CurrentProject,
                *id as i32,
                true,
            );
            Ok(StepOutput::Unit)
        }
        _ => Err("sync dispatch not implemented for this RegionOp".to_string()),
    }
}

// =============================================================================
// ExtState dispatch
// =============================================================================

fn dispatch_ext_state_sync(
    op: &ExtStateOp,
    _outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    use crate::safe_wrappers::ext_state as sw;
    use std::ffi::CString;

    let low = Reaper::get().medium_reaper().low();

    match op {
        ExtStateOp::GetExtState(section, key) => {
            let section_c = CString::new(section.as_str()).map_err(|e| format!("CString: {e}"))?;
            let key_c = CString::new(key.as_str()).map_err(|e| format!("CString: {e}"))?;
            Ok(StepOutput::OptStr(sw::get_ext_state(
                low, &section_c, &key_c,
            )))
        }
        ExtStateOp::SetExtState(section, key, value, persist) => {
            let section_c = CString::new(section.as_str()).map_err(|e| format!("CString: {e}"))?;
            let key_c = CString::new(key.as_str()).map_err(|e| format!("CString: {e}"))?;
            let value_c = CString::new(value.as_str()).map_err(|e| format!("CString: {e}"))?;
            sw::set_ext_state(low, &section_c, &key_c, &value_c, *persist);
            Ok(StepOutput::Unit)
        }
        ExtStateOp::DeleteExtState(section, key, persist) => {
            let section_c = CString::new(section.as_str()).map_err(|e| format!("CString: {e}"))?;
            let key_c = CString::new(key.as_str()).map_err(|e| format!("CString: {e}"))?;
            sw::delete_ext_state(low, &section_c, &key_c, *persist);
            Ok(StepOutput::Unit)
        }
        ExtStateOp::HasExtState(section, key) => {
            let section_c = CString::new(section.as_str()).map_err(|e| format!("CString: {e}"))?;
            let key_c = CString::new(key.as_str()).map_err(|e| format!("CString: {e}"))?;
            Ok(StepOutput::Bool(sw::has_ext_state(low, &section_c, &key_c)))
        }
        _ => Err("sync dispatch not implemented for this ExtStateOp".to_string()),
    }
}

// =============================================================================
// Health dispatch
// =============================================================================

fn dispatch_health_sync(op: &HealthOp) -> Result<StepOutput, String> {
    match op {
        HealthOp::Ping => Ok(StepOutput::Bool(true)),
        HealthOp::ShowConsoleMsg(msg) => {
            Reaper::get().show_console_msg(msg.as_str());
            Ok(StepOutput::Unit)
        }
    }
}
