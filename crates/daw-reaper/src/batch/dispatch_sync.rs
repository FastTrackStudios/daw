//! Synchronous dispatch — runs directly on REAPER's main thread.
//!
//! Each function in this module calls REAPER APIs directly (via reaper_high)
//! rather than going through async service methods + main_thread::query().
//! This eliminates the ~33ms timer callback latency per operation.
//!
//! Called from `BatchExecutor::execute_sync()` inside a single
//! `main_thread::query()` closure.

use super::resolve::{resolve_fx_chain_arg, resolve_project_arg, resolve_track_arg};
use crate::fx::{
    build_fx_info, build_fx_parameter, read_config_i32, resolve_fx_chain, resolve_fx_index,
};
use crate::project::project_to_info;
use crate::safe_wrappers::fx as fx_sw;
use crate::track::{assign_parent_guids, build_track_info, resolve_project, resolve_track};
use daw_proto::batch::*;
use daw_proto::*;
use reaper_high::{GroupingBehavior, PlayRate, Reaper, Tempo as ReaperTempo};
use reaper_medium::{
    CommandId, DurationInSeconds, FxPresetRef, GangBehavior, ItemAttributeKey,
    MarkerOrRegionPosition, MeasureMode, PositionInSeconds, ProjectContext as ReaperProjectContext,
    Semitones, SetEditCurPosOptions, TakeAttributeKey, TrackFxLocation, UiRefreshBehavior,
    UndoBehavior,
};

/// Service instances needed by sync dispatch for stateful operations.
pub struct SyncServices<'a> {
    pub audio_accessor_svc: &'a crate::ReaperAudioAccessor,
}

/// Dispatch a single batch operation synchronously on the main thread.
pub fn dispatch_op_sync(
    op: &BatchOp,
    outputs: &[Option<StepOutput>],
    services: &SyncServices<'_>,
) -> Result<StepOutput, String> {
    match op {
        BatchOp::Project(op) => dispatch_project_sync(op, outputs),
        BatchOp::Transport(op) => dispatch_transport_sync(op, outputs),
        BatchOp::Track(op) => dispatch_track_sync(op, outputs),
        BatchOp::Marker(op) => dispatch_marker_sync(op, outputs),
        BatchOp::Region(op) => dispatch_region_sync(op, outputs),
        BatchOp::ExtState(op) => dispatch_ext_state_sync(op, outputs),
        BatchOp::Fx(op) => dispatch_fx_sync(op, outputs),
        BatchOp::Item(op) => dispatch_item_sync(op, outputs),
        BatchOp::Take(op) => dispatch_take_sync(op, outputs),
        BatchOp::Midi(op) => dispatch_midi_sync(op, outputs),
        BatchOp::TempoMap(op) => dispatch_tempo_map_sync(op, outputs),
        BatchOp::Routing(op) => dispatch_routing_sync(op, outputs),
        BatchOp::AudioEngine(op) => dispatch_audio_engine_sync(op),
        BatchOp::Resource(op) => dispatch_resource_sync(op),
        BatchOp::ActionRegistry(op) => dispatch_action_registry_sync(op),
        BatchOp::LiveMidi(op) => dispatch_live_midi_sync(op),
        BatchOp::Peak(op) => dispatch_peak_sync(op, outputs),
        BatchOp::PositionConversion(op) => dispatch_position_conversion_sync(op, outputs),
        BatchOp::Health(op) => dispatch_health_sync(op),
        BatchOp::AudioAccessor(op) => {
            dispatch_audio_accessor_sync(op, outputs, services.audio_accessor_svc)
        }
        BatchOp::MidiAnalysis(op) => dispatch_midi_analysis_sync(op),
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
            | BatchOp::Fx(_)
            | BatchOp::Item(_)
            | BatchOp::Take(_)
            | BatchOp::Midi(_)
            | BatchOp::TempoMap(_)
            | BatchOp::Marker(_)
            | BatchOp::Region(_)
            | BatchOp::ExtState(_)
            | BatchOp::Routing(_)
            | BatchOp::AudioEngine(_)
            | BatchOp::Resource(_)
            | BatchOp::ActionRegistry(_)
            | BatchOp::LiveMidi(_)
            | BatchOp::Peak(_)
            | BatchOp::PositionConversion(_)
            | BatchOp::Health(_)
            | BatchOp::AudioAccessor(_)
            | BatchOp::MidiAnalysis(_)
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

// =============================================================================
// FX dispatch
// =============================================================================

/// Helper: resolve project + FX chain.
fn resolve_chain(
    p: &ProjectArg,
    c: &FxChainArg,
    outputs: &[Option<StepOutput>],
) -> Result<
    (
        reaper_high::Project,
        reaper_high::Track,
        reaper_high::FxChain,
    ),
    String,
> {
    let project = proj(p, outputs)?;
    let chain_ctx = resolve_fx_chain_arg(c, outputs)?;
    let (track, chain) =
        resolve_fx_chain(&project, &chain_ctx).ok_or_else(|| "FX chain not found".to_string())?;
    Ok((project, track, chain))
}

/// Helper: resolve project + FX chain + single FX.
fn resolve_chain_and_fx(
    p: &ProjectArg,
    c: &FxChainArg,
    f: &FxRef,
    outputs: &[Option<StepOutput>],
) -> Result<(reaper_high::Track, reaper_high::FxChain, reaper_high::Fx), String> {
    let project = proj(p, outputs)?;
    let chain_ctx = resolve_fx_chain_arg(c, outputs)?;
    let (track, chain) =
        resolve_fx_chain(&project, &chain_ctx).ok_or_else(|| "FX chain not found".to_string())?;
    let idx = resolve_fx_index(&chain, f).ok_or_else(|| "FX not found".to_string())?;
    let fx = chain.fx_by_index_untracked(idx);
    Ok((track, chain, fx))
}

/// Helper: convert FX index + input-chain flag to TrackFxLocation.
fn fx_location(index: u32, is_input: bool) -> TrackFxLocation {
    if is_input {
        TrackFxLocation::InputFxChain(index)
    } else {
        TrackFxLocation::NormalFxChain(index)
    }
}

fn dispatch_fx_sync(op: &FxOp, outputs: &[Option<StepOutput>]) -> Result<StepOutput, String> {
    let reaper = Reaper::get();

    match op {
        // -----------------------------------------------------------------
        // Global queries
        // -----------------------------------------------------------------
        FxOp::ListInstalledFx => {
            let low = reaper.medium_reaper().low();
            let mut list = Vec::new();
            let mut i = 0i32;
            while let Some((name, ident)) = crate::safe_wrappers::fx::enum_installed_fx(low, i) {
                list.push(InstalledFx { name, ident });
                i += 1;
            }
            Ok(StepOutput::InstalledFxList(list))
        }
        FxOp::GetLastTouchedFx => {
            let result = reaper.medium_reaper().get_last_touched_fx();
            let info = result.and_then(|r| {
                use reaper_medium::GetLastTouchedFxResult::*;
                match r {
                    TrackFx {
                        track_location,
                        fx_location,
                        param_index,
                    } => {
                        let project = reaper.current_project();
                        let track = match track_location {
                            reaper_medium::TrackLocation::MasterTrack => {
                                project.master_track().ok()?
                            }
                            reaper_medium::TrackLocation::NormalTrack(idx) => {
                                project.track_by_index(idx)?
                            }
                        };
                        let track_guid = track.guid().to_string_without_braces();
                        let (fx_index, is_input_fx) = match fx_location {
                            TrackFxLocation::NormalFxChain(idx) => (idx, false),
                            TrackFxLocation::InputFxChain(idx) => (idx, true),
                            _ => return None,
                        };
                        Some(LastTouchedFx {
                            track_guid,
                            fx_index,
                            param_index,
                            is_input_fx,
                        })
                    }
                    _ => None,
                }
            });
            Ok(StepOutput::OptLastTouchedFx(info))
        }

        // -----------------------------------------------------------------
        // Chain-level queries
        // -----------------------------------------------------------------
        FxOp::GetFxList(p, c) => {
            let (_project, _track, chain) = resolve_chain(p, c, outputs)?;
            let list: Vec<Fx> = chain
                .index_based_fxs()
                .map(|fx| build_fx_info(&fx, Some(&chain)))
                .collect();
            Ok(StepOutput::FxList(list))
        }
        FxOp::FxCount(p, c) => {
            let (_project, _track, chain) = resolve_chain(p, c, outputs)?;
            Ok(StepOutput::U32(chain.fx_count()))
        }

        // -----------------------------------------------------------------
        // Single-FX queries
        // -----------------------------------------------------------------
        FxOp::GetFx(p, c, f) => {
            let (_track, chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            Ok(StepOutput::Fx(build_fx_info(&fx, Some(&chain))))
        }
        FxOp::SetFxEnabled(p, c, f, enabled) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            if *enabled {
                fx.enable().map_err(|e| format!("enable failed: {e}"))?;
            } else {
                fx.disable().map_err(|e| format!("disable failed: {e}"))?;
            }
            Ok(StepOutput::Unit)
        }
        FxOp::SetFxOffline(p, c, f, offline) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            fx.set_online(!offline)
                .map_err(|e| format!("set_online failed: {e}"))?;
            Ok(StepOutput::Unit)
        }
        FxOp::AddFx(p, c, name) => {
            let (_project, _track, chain) = resolve_chain(p, c, outputs)?;
            let fx = chain
                .add_fx_by_original_name(name.as_str())
                .ok_or_else(|| format!("failed to add FX '{}'", name))?;
            let guid = fx
                .get_or_query_guid()
                .map(|g| g.to_string_without_braces())
                .map_err(|e| format!("get guid failed: {e}"))?;
            Ok(StepOutput::OptStr(Some(guid)))
        }
        FxOp::RemoveFx(p, c, f) => {
            let (track, chain, _fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let idx = resolve_fx_index(&chain, f).ok_or_else(|| "FX not found".to_string())?;
            let raw_track = track.raw().map_err(|e| format!("raw track failed: {e}"))?;
            let location = fx_location(idx, chain.is_input_fx());
            fx_sw::track_fx_delete(reaper.medium_reaper(), raw_track, location)
                .map_err(|e| format!("track_fx_delete failed: {e}"))?;
            Ok(StepOutput::Unit)
        }

        // -----------------------------------------------------------------
        // Parameters
        // -----------------------------------------------------------------
        FxOp::GetParameters(p, c, f) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let params: Vec<FxParameter> = fx
                .parameters()
                .map(|param| build_fx_parameter(&param))
                .collect();
            Ok(StepOutput::FxParameterList(params))
        }
        FxOp::GetParameter(p, c, f, idx) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            if *idx >= fx.parameter_count() {
                return Ok(StepOutput::OptFxParameter(None));
            }
            let param = fx.parameter_by_index(*idx);
            Ok(StepOutput::OptFxParameter(Some(build_fx_parameter(&param))))
        }
        FxOp::SetParameter(p, req) => {
            let project = proj(p, outputs)?;
            let (_, chain) = resolve_fx_chain(&project, &req.target.context)
                .ok_or_else(|| "FX chain not found".to_string())?;
            let fx_idx = resolve_fx_index(&chain, &req.target.fx)
                .ok_or_else(|| "FX not found".to_string())?;
            let fx = chain
                .fx_by_index(fx_idx)
                .ok_or_else(|| format!("fx_by_index({}) returned None", fx_idx))?;
            let param = fx.parameter_by_index(req.index);
            let norm_val = reaper_medium::ReaperNormalizedFxParamValue::new(req.value);
            param
                .set_reaper_normalized_value(norm_val)
                .map_err(|e| format!("set_reaper_normalized_value failed: {e}"))?;
            Ok(StepOutput::Unit)
        }
        FxOp::GetParameterByName(p, c, f, name) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            for param in fx.parameters() {
                if let Ok(pname) = param.name() {
                    if pname.to_str() == *name {
                        return Ok(StepOutput::OptFxParameter(Some(build_fx_parameter(&param))));
                    }
                }
            }
            Ok(StepOutput::OptFxParameter(None))
        }
        FxOp::SetParameterByName(p, req) => {
            let project = proj(p, outputs)?;
            let (_, chain) = resolve_fx_chain(&project, &req.target.context)
                .ok_or_else(|| "FX chain not found".to_string())?;
            let fx_idx = resolve_fx_index(&chain, &req.target.fx)
                .ok_or_else(|| "FX not found".to_string())?;
            let fx = chain
                .fx_by_index(fx_idx)
                .ok_or_else(|| format!("fx_by_index({}) returned None", fx_idx))?;
            for param in fx.parameters() {
                if let Ok(pname) = param.name() {
                    if pname.to_str() == req.name {
                        let norm_val = reaper_medium::ReaperNormalizedFxParamValue::new(req.value);
                        param
                            .set_reaper_normalized_value(norm_val)
                            .map_err(|e| format!("set_reaper_normalized_value failed: {e}"))?;
                        return Ok(StepOutput::Unit);
                    }
                }
            }
            Err(format!("parameter '{}' not found", req.name))
        }

        // -----------------------------------------------------------------
        // Presets
        // -----------------------------------------------------------------
        FxOp::GetPresetIndex(p, c, f) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fx.preset_index_and_count()
            }))
            .ok();
            let info = result.map(|r| {
                let name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fx.preset_name()
                        .map(|rs| rs.to_str().to_string())
                        .filter(|s| !s.is_empty())
                }))
                .unwrap_or(None);
                FxPresetIndex {
                    index: r.index,
                    count: r.count,
                    name,
                }
            });
            Ok(StepOutput::OptFxPresetIndex(info))
        }
        FxOp::NextPreset(p, c, f) => {
            let (track, chain, _fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let idx = resolve_fx_index(&chain, f).ok_or_else(|| "FX not found".to_string())?;
            let raw_track = track
                .raw()
                .map_err(|_| "raw track not available".to_string())?;
            let location = fx_location(idx, chain.is_input_fx());
            fx_sw::track_fx_navigate_presets(reaper.medium_reaper(), raw_track, location, 1);
            Ok(StepOutput::Unit)
        }
        FxOp::PrevPreset(p, c, f) => {
            let (track, chain, _fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let idx = resolve_fx_index(&chain, f).ok_or_else(|| "FX not found".to_string())?;
            let raw_track = track
                .raw()
                .map_err(|_| "raw track not available".to_string())?;
            let location = fx_location(idx, chain.is_input_fx());
            fx_sw::track_fx_navigate_presets(reaper.medium_reaper(), raw_track, location, -1);
            Ok(StepOutput::Unit)
        }
        FxOp::SetPreset(p, c, f, preset_idx) => {
            let (track, chain, _fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let idx = resolve_fx_index(&chain, f).ok_or_else(|| "FX not found".to_string())?;
            let raw_track = track.raw().map_err(|e| format!("raw track failed: {e}"))?;
            let location = fx_location(idx, chain.is_input_fx());
            fx_sw::track_fx_set_preset_by_index(
                reaper.medium_reaper(),
                raw_track,
                location,
                FxPresetRef::Preset(*preset_idx),
            );
            Ok(StepOutput::Unit)
        }

        // -----------------------------------------------------------------
        // UI
        // -----------------------------------------------------------------
        FxOp::OpenFxUi(p, c, f) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let _ = fx.show_in_floating_window();
            Ok(StepOutput::Unit)
        }
        FxOp::CloseFxUi(p, c, f) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let _ = fx.hide_floating_window();
            Ok(StepOutput::Unit)
        }
        FxOp::ToggleFxUi(p, c, f) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            if fx.window_is_open() {
                let _ = fx.hide_floating_window();
            } else {
                let _ = fx.show_in_floating_window();
            }
            Ok(StepOutput::Unit)
        }

        // -----------------------------------------------------------------
        // Named config / latency
        // -----------------------------------------------------------------
        FxOp::GetNamedConfig(p, c, f, key) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let value = fx
                .get_named_config_param(&**key, 4096)
                .ok()
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string());
            Ok(StepOutput::OptStr(value))
        }
        FxOp::SetNamedConfig(p, req) => {
            let project = proj(p, outputs)?;
            let (_, chain) = resolve_fx_chain(&project, &req.target.context)
                .ok_or_else(|| "FX chain not found".to_string())?;
            let idx = resolve_fx_index(&chain, &req.target.fx)
                .ok_or_else(|| "FX not found".to_string())?;
            let fx = chain.fx_by_index_untracked(idx);
            fx_sw::fx_set_named_config_param(&fx, &req.key, &req.value)?;
            Ok(StepOutput::Unit)
        }
        FxOp::GetFxLatency(p, c, f) => {
            let (_track, _chain, fx) = resolve_chain_and_fx(p, c, f, outputs)?;
            let pdc_samples = read_config_i32(&fx, "pdc");
            let chain_pdc_actual = read_config_i32(&fx, "chain_pdc_actual");
            let chain_pdc_reporting = read_config_i32(&fx, "chain_pdc_reporting");
            Ok(StepOutput::OptFxLatency(Some(FxLatency {
                pdc_samples,
                chain_pdc_actual,
                chain_pdc_reporting,
            })))
        }

        // -----------------------------------------------------------------
        // Complex operations — fall back to async dispatch
        // -----------------------------------------------------------------
        _ => Err("sync dispatch not implemented for this FxOp".to_string()),
    }
}

// =============================================================================
// Routing dispatch
// =============================================================================

fn dispatch_routing_sync(
    op: &RoutingOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    use crate::routing::convert_track_route;
    use crate::safe_wrappers::routing as routing_sw;
    use reaper_high::SendPartnerType;
    use reaper_medium::{
        EditMode, ReaperVolumeValue, SendTarget, TrackSendAttributeKey, TrackSendCategory,
    };

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    /// Resolve a RouteLocation's track within a project.
    fn resolve_route_track(
        project: &reaper_high::Project,
        location: &RouteLocation,
    ) -> Option<reaper_high::Track> {
        crate::track::resolve_track(project, &location.track)
    }

    /// Get a route's index from a RouteRef, returning Err for ByDestination.
    fn route_index(route: &RouteRef) -> Result<u32, String> {
        match route {
            RouteRef::Index(idx) => Ok(*idx),
            RouteRef::ByDestination(_) => {
                Err("ByDestination lookup not supported in sync dispatch".to_string())
            }
        }
    }

    /// Resolve a route by RouteLocation, returning the reaper_high::TrackRoute.
    fn resolve_reaper_route(
        track: &reaper_high::Track,
        location: &RouteLocation,
    ) -> Result<reaper_high::TrackRoute, String> {
        let idx = route_index(&location.route)?;
        let route = match location.route_type {
            RouteType::Send => {
                let hw_count = track.typed_send_count(SendPartnerType::HardwareOutput);
                track.send_by_index(hw_count + idx)
            }
            RouteType::Receive => track.receive_by_index(idx),
            RouteType::HardwareOutput => {
                track.typed_send_by_index(SendPartnerType::HardwareOutput, idx)
            }
        };
        route.ok_or_else(|| format!("route not found at index {}", idx))
    }

    match op {
        RoutingOp::GetSends(p, t) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let routes: Vec<_> = track
                .typed_sends(SendPartnerType::Track)
                .enumerate()
                .map(|(i, r)| convert_track_route(&r, RouteType::Send, i as u32))
                .collect();
            Ok(StepOutput::RouteList(routes))
        }
        RoutingOp::GetReceives(p, t) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let routes: Vec<_> = track
                .receives()
                .enumerate()
                .map(|(i, r)| convert_track_route(&r, RouteType::Receive, i as u32))
                .collect();
            Ok(StepOutput::RouteList(routes))
        }
        RoutingOp::GetHardwareOutputs(p, t) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let routes: Vec<_> = track
                .typed_sends(SendPartnerType::HardwareOutput)
                .enumerate()
                .map(|(i, r)| convert_track_route(&r, RouteType::HardwareOutput, i as u32))
                .collect();
            Ok(StepOutput::RouteList(routes))
        }
        RoutingOp::GetRoute(p, loc) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let reaper_route = resolve_reaper_route(&track, loc)?;
            let idx = route_index(&loc.route)?;
            Ok(StepOutput::OptRoute(Some(convert_track_route(
                &reaper_route,
                loc.route_type,
                idx,
            ))))
        }
        RoutingOp::AddSend(p, src, dst) => {
            let project = proj(p, outputs)?;
            let src_ref = resolve_track_arg(src, outputs)?;
            let dst_ref = resolve_track_arg(dst, outputs)?;
            let source_track = crate::track::resolve_track(&project, &src_ref)
                .ok_or_else(|| "source track not found".to_string())?;
            let dest_track = crate::track::resolve_track(&project, &dst_ref)
                .ok_or_else(|| "dest track not found".to_string())?;
            let route = source_track.add_send_to(&dest_track);
            Ok(StepOutput::OptU32(route.track_route_index()))
        }
        RoutingOp::RemoveRoute(p, loc) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let idx = route_index(&loc.route)?;
            let (category, actual_index) = match loc.route_type {
                RouteType::Send => {
                    let hw_count = track.typed_send_count(SendPartnerType::HardwareOutput);
                    (TrackSendCategory::Send, idx - hw_count.min(idx))
                }
                RouteType::Receive => (TrackSendCategory::Receive, idx),
                RouteType::HardwareOutput => (TrackSendCategory::HardwareOutput, idx),
            };
            routing_sw::remove_track_send(medium, raw_track, category, actual_index);
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetVolume(p, loc, v) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let reaper_route = resolve_reaper_route(&track, loc)?;
            if let Ok(vol) = ReaperVolumeValue::new(*v) {
                let _ = reaper_route.set_volume(vol, EditMode::NormalTweak);
            }
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetPan(p, loc, v) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let reaper_route = resolve_reaper_route(&track, loc)?;
            let pan_obj = reaper_high::Pan::from_normalized_value(*v);
            let _ = reaper_route.set_pan(pan_obj, EditMode::NormalTweak);
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetMuted(p, loc, v) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let reaper_route = resolve_reaper_route(&track, loc)?;
            if *v {
                let _ = reaper_route.mute();
            } else {
                let _ = reaper_route.unmute();
            }
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetMono(p, loc, v) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let reaper_route = resolve_reaper_route(&track, loc)?;
            let _ = reaper_route.set_mono(*v);
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetPhase(p, loc, v) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let reaper_route = resolve_reaper_route(&track, loc)?;
            let _ = reaper_route.set_phase_inverted(*v);
            Ok(StepOutput::Unit)
        }
        RoutingOp::AddHardwareOutput(p, t, hw) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            match routing_sw::create_track_send(medium, raw_track, SendTarget::HardwareOutput) {
                Ok(index) => {
                    let dst_chan = (*hw * 2) as f64;
                    routing_sw::set_track_send_info_value(
                        medium,
                        raw_track,
                        TrackSendCategory::HardwareOutput,
                        index,
                        TrackSendAttributeKey::DstChan,
                        dst_chan,
                    );
                    Ok(StepOutput::OptU32(Some(index)))
                }
                Err(e) => Err(format!("create hardware output: {:?}", e)),
            }
        }
        RoutingOp::SetSendMode(p, t, r, mode) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let idx = route_index(r)?;
            let hw_count = track.typed_send_count(SendPartnerType::HardwareOutput);
            let (category, actual_index) = if idx < hw_count {
                (TrackSendCategory::HardwareOutput, idx)
            } else {
                (TrackSendCategory::Send, idx - hw_count)
            };
            let raw_mode = match mode {
                SendMode::PostFader => 0,
                SendMode::PreFx => 1,
                SendMode::PostFx => 3,
            };
            routing_sw::set_track_send_info_value(
                medium,
                raw_track,
                category,
                actual_index,
                TrackSendAttributeKey::SendMode,
                raw_mode as f64,
            );
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetSourceChannels(p, loc, start, _num) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let idx = route_index(&loc.route)?;
            let (category, actual_index) = match loc.route_type {
                RouteType::Send => (TrackSendCategory::Send, idx),
                RouteType::Receive => (TrackSendCategory::Receive, idx),
                RouteType::HardwareOutput => (TrackSendCategory::HardwareOutput, idx),
            };
            routing_sw::set_track_send_info_value(
                medium,
                raw_track,
                category,
                actual_index,
                TrackSendAttributeKey::SrcChan,
                *start as f64,
            );
            Ok(StepOutput::Unit)
        }
        RoutingOp::SetDestChannels(p, loc, start, _num) => {
            let project = proj(p, outputs)?;
            let track =
                resolve_route_track(&project, loc).ok_or_else(|| "track not found".to_string())?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let idx = route_index(&loc.route)?;
            let (category, actual_index) = match loc.route_type {
                RouteType::Send => (TrackSendCategory::Send, idx),
                RouteType::Receive => (TrackSendCategory::Receive, idx),
                RouteType::HardwareOutput => (TrackSendCategory::HardwareOutput, idx),
            };
            routing_sw::set_track_send_info_value(
                medium,
                raw_track,
                category,
                actual_index,
                TrackSendAttributeKey::DstChan,
                *start as f64,
            );
            Ok(StepOutput::Unit)
        }
        RoutingOp::GetParentSendEnabled(p, t) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let value = routing_sw::get_media_track_info_value(
                medium,
                raw_track,
                reaper_medium::TrackAttributeKey::MainSend,
            );
            Ok(StepOutput::Bool(value > 0.0))
        }
        RoutingOp::SetParentSendEnabled(p, t, v) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let raw_track = track.raw().map_err(|e| format!("raw track: {e}"))?;
            routing_sw::set_media_track_info_value(
                medium,
                raw_track,
                reaper_medium::TrackAttributeKey::MainSend,
                if *v { 1.0 } else { 0.0 },
            );
            Ok(StepOutput::Unit)
        }
    }
}

// =============================================================================
// Audio engine dispatch
// =============================================================================

fn dispatch_audio_engine_sync(op: &AudioEngineOp) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        AudioEngineOp::GetState => {
            let is_running = medium.audio_is_running();
            let is_prebuffer = medium.low().Audio_IsPreBuffer() != 0;
            let latency = crate::audio_engine::get_audio_latency_internal(medium);
            Ok(StepOutput::AudioEngineState(AudioEngineState {
                is_running,
                is_prebuffer,
                latency,
            }))
        }
        AudioEngineOp::GetLatency => {
            let latency = crate::audio_engine::get_audio_latency_internal(medium);
            Ok(StepOutput::AudioLatency(latency))
        }
        AudioEngineOp::GetOutputLatencySeconds => {
            if !medium.audio_is_running() {
                return Ok(StepOutput::F64(0.0));
            }
            Ok(StepOutput::F64(medium.low().GetOutputLatency()))
        }
        AudioEngineOp::IsRunning => Ok(StepOutput::Bool(medium.audio_is_running())),
        AudioEngineOp::GetAudioInputs => {
            let low = medium.low();
            let device_name =
                crate::safe_wrappers::audio::get_audio_device_info(low, c"IDENT_IN", 256)
                    .unwrap_or_default();
            let num_inputs = low.GetNumAudioInputs() as u32;
            let channels: Vec<AudioInputChannel> = (0..num_inputs)
                .map(|i| {
                    let name = medium.get_input_channel_name(i, |cstr| {
                        cstr.map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| format!("Input {}", i + 1))
                    });
                    AudioInputChannel { index: i, name }
                })
                .collect();
            Ok(StepOutput::AudioInputInfo(AudioInputInfo {
                device_name,
                channels,
            }))
        }
    }
}

// =============================================================================
// Resource dispatch
// =============================================================================

fn dispatch_resource_sync(op: &ResourceOp) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        ResourceOp::GetResourcePath => {
            let utf8_path = medium.get_resource_path(|p| p.to_path_buf());
            Ok(StepOutput::Path(utf8_path.into_std_path_buf()))
        }
        ResourceOp::GetIniFilePath => {
            let utf8_path = medium.get_ini_file(|p| p.to_path_buf());
            Ok(StepOutput::Path(utf8_path.into_std_path_buf()))
        }
        ResourceOp::GetColorThemePath => {
            let low = medium.low();
            let ptr = low.GetLastColorThemeFile();
            let path = crate::safe_wrappers::cstring::read_cstr(ptr).map(std::path::PathBuf::from);
            Ok(StepOutput::OptPath(path))
        }
    }
}

// =============================================================================
// Action registry dispatch
// =============================================================================

fn dispatch_action_registry_sync(op: &ActionRegistryOp) -> Result<StepOutput, String> {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        ActionRegistryOp::RegisterAction(..) => {
            Err("RegisterAction requires async dispatch (leaks closures)".to_string())
        }
        ActionRegistryOp::UnregisterAction(name) => {
            let removed = crate::action_registry::registered_actions()
                .lock()
                .unwrap()
                .remove(name)
                .is_some();
            Ok(StepOutput::Bool(removed))
        }
        ActionRegistryOp::IsRegistered(name) => {
            let found = medium.named_command_lookup(format!("_{name}")).is_some();
            Ok(StepOutput::Bool(found))
        }
        ActionRegistryOp::LookupCommandId(name) => {
            let id = medium
                .named_command_lookup(format!("_{name}"))
                .map(|id| id.get());
            Ok(StepOutput::OptU32(id))
        }
        ActionRegistryOp::IsInActionList(_name) => {
            Err("IsInActionList requires async dispatch (action list enumeration)".to_string())
        }
        ActionRegistryOp::ExecuteCommand(id) => {
            medium.main_on_command_ex(CommandId::new(*id), 0, ReaperProjectContext::CurrentProject);
            Ok(StepOutput::Unit)
        }
        ActionRegistryOp::ExecuteNamedAction(name) => {
            let lookup = format!("_{name}");
            if let Some(cmd_id) = medium.named_command_lookup(lookup) {
                medium.main_on_command_ex(cmd_id, 0, ReaperProjectContext::CurrentProject);
                Ok(StepOutput::Bool(true))
            } else {
                Ok(StepOutput::Bool(false))
            }
        }
        ActionRegistryOp::SetToggleState(name, v) => {
            let mut states = crate::action_registry::toggle_states().lock().unwrap();
            if states.contains_key(name) {
                states.insert(name.clone(), *v);
            }
            Ok(StepOutput::Unit)
        }
        ActionRegistryOp::GetToggleState(name) => {
            let state = crate::action_registry::toggle_states()
                .lock()
                .unwrap()
                .get(name)
                .copied();
            Ok(StepOutput::OptBool(state))
        }
    }
}

// =============================================================================
// Live MIDI dispatch
// =============================================================================

fn dispatch_live_midi_sync(op: &LiveMidiOp) -> Result<StepOutput, String> {
    match op {
        LiveMidiOp::GetInputDevices => Ok(StepOutput::MidiInputDeviceList(vec![])),
        LiveMidiOp::GetOutputDevices => Ok(StepOutput::MidiOutputDeviceList(vec![])),
        LiveMidiOp::GetInputDevice(_id) => Ok(StepOutput::OptMidiInputDevice(None)),
        LiveMidiOp::GetOutputDevice(_id) => Ok(StepOutput::OptMidiOutputDevice(None)),
        LiveMidiOp::OpenInputDevice(_id) => Ok(StepOutput::Bool(false)),
        LiveMidiOp::OpenOutputDevice(_id) => Ok(StepOutput::Bool(false)),
        LiveMidiOp::CloseInputDevice(_id) => Ok(StepOutput::Unit),
        LiveMidiOp::CloseOutputDevice(_id) => Ok(StepOutput::Unit),
        LiveMidiOp::SendMidi(_dev, _msg, _timing) => Ok(StepOutput::Unit),
        LiveMidiOp::SendMidiBatch(_dev, _events) => Ok(StepOutput::Unit),
        LiveMidiOp::StuffMidiMessage(target, msg) => {
            let Some((status, data1, data2)) = msg.to_raw_bytes() else {
                return Ok(StepOutput::Unit);
            };
            let mode = match target {
                StuffMidiTarget::VirtualMidiKeyboard => 0,
                StuffMidiTarget::ControlInput => 1,
                StuffMidiTarget::VirtualMidiKeyboardCurrentChannel => 2,
            };
            Reaper::get().medium_reaper().low().StuffMIDIMessage(
                mode,
                status as i32,
                data1 as i32,
                data2 as i32,
            );
            Ok(StepOutput::Unit)
        }
    }
}

// =============================================================================
// Peak dispatch
// =============================================================================

fn dispatch_peak_sync(op: &PeakOp, outputs: &[Option<StepOutput>]) -> Result<StepOutput, String> {
    match op {
        PeakOp::GetTrackPeak(p, t, ch) => {
            let (_, track) = proj_track(p, t, outputs)?;
            let raw = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let low = Reaper::get().medium_reaper().low();

            let peak_linear = crate::safe_wrappers::peak::track_get_peak_info(low, raw, *ch as i32);
            let peak_hold_db =
                crate::safe_wrappers::peak::track_get_peak_hold_db(low, raw, *ch as i32, false);
            let peak_db = if peak_linear > 0.0 {
                20.0 * peak_linear.log10()
            } else {
                -150.0
            };
            Ok(StepOutput::TrackPeak(TrackPeak {
                peak_db,
                peak_hold_db,
            }))
        }
        PeakOp::GetTakePeaks(..) => {
            Err("GetTakePeaks requires async dispatch (complex take/PCM resolution)".to_string())
        }
    }
}

// =============================================================================
// Item dispatch
// =============================================================================

fn dispatch_item_sync(op: &ItemOp, outputs: &[Option<StepOutput>]) -> Result<StepOutput, String> {
    use crate::item::{ReaperItem, item_guid_string};
    use crate::safe_wrappers::item as item_sw;

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        ItemOp::GetItems(p, t) => {
            let project = proj(p, outputs)?;
            let tref = resolve_track_arg(t, outputs)?;
            let track =
                resolve_track(&project, &tref).ok_or_else(|| "track not found".to_string())?;
            let track_ptr = track.raw().map_err(|e| format!("raw track: {e}"))?;

            let count = item_sw::count_track_media_items(medium, track_ptr);
            let mut items = Vec::with_capacity(count as usize);
            for i in 0..count {
                if let Some(item) = item_sw::get_track_media_item(medium, track_ptr, i)
                    && let Some(mut item_data) = ReaperItem::media_item_to_item(item)
                {
                    item_data.index = i;
                    items.push(item_data);
                }
            }
            Ok(StepOutput::ItemList(items))
        }
        ItemOp::GetItem(p, item_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item = ReaperItem::resolve_item(item_ref, proj_ctx)
                .and_then(ReaperItem::media_item_to_item);
            Ok(StepOutput::OptItem(item))
        }
        ItemOp::GetAllItems(p) => {
            let _project = proj(p, outputs)?;
            let count = medium.count_media_items(ReaperProjectContext::CurrentProject);
            let mut items = Vec::with_capacity(count as usize);
            for i in 0..count {
                if let Some(item) = medium.get_media_item(ReaperProjectContext::CurrentProject, i)
                    && let Some(mut item_data) = ReaperItem::media_item_to_item(item)
                {
                    item_data.index = i;
                    items.push(item_data);
                }
            }
            Ok(StepOutput::ItemList(items))
        }
        ItemOp::GetSelectedItems(p) => {
            let _project = proj(p, outputs)?;
            let count = medium.count_selected_media_items(ReaperProjectContext::CurrentProject);
            let mut items = Vec::with_capacity(count as usize);
            for i in 0..count {
                if let Some(item) =
                    medium.get_selected_media_item(ReaperProjectContext::CurrentProject, i)
                    && let Some(item_data) = ReaperItem::media_item_to_item(item)
                {
                    items.push(item_data);
                }
            }
            Ok(StepOutput::ItemList(items))
        }
        ItemOp::ItemCount(p, t) => {
            let project = proj(p, outputs)?;
            let tref = resolve_track_arg(t, outputs)?;
            let track =
                resolve_track(&project, &tref).ok_or_else(|| "track not found".to_string())?;
            Ok(StepOutput::U32(track.item_count()))
        }
        ItemOp::AddItem(p, t, position, length) => {
            let project = proj(p, outputs)?;
            let tref = resolve_track_arg(t, outputs)?;
            let track =
                resolve_track(&project, &tref).ok_or_else(|| "track not found".to_string())?;
            let track_ptr = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let low = medium.low();
            let start = position.as_seconds();
            let end = start + length.as_seconds();
            let item = crate::safe_wrappers::midi::create_new_midi_item(low, track_ptr, start, end)
                .ok_or_else(|| "failed to create MIDI item".to_string())?;
            medium.update_timeline();
            let guid = item_guid_string(medium, item);
            Ok(StepOutput::OptStr(Some(guid)))
        }
        ItemOp::DeleteItem(p, item_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                if let Some(track) = item_sw::get_media_item_track(medium, item_ptr) {
                    item_sw::delete_track_media_item(medium, track, item_ptr);
                }
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetPosition(p, item_ref, position) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx)
                && let Ok(pos) = PositionInSeconds::new(position.as_seconds())
            {
                item_sw::set_media_item_position(medium, item_ptr, pos, UiRefreshBehavior::Refresh);
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetLength(p, item_ref, length) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx)
                && let Ok(len) = DurationInSeconds::new(length.as_seconds())
            {
                item_sw::set_media_item_length(medium, item_ptr, len, UiRefreshBehavior::Refresh);
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetSnapOffset(p, item_ref, offset) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::SnapOffset,
                    offset.as_seconds(),
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetMuted(p, item_ref, muted) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::Mute,
                    if *muted { 1.0 } else { 0.0 },
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetSelected(p, item_ref, selected) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                item_sw::set_media_item_selected(medium, item_ptr, *selected);
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetLocked(_p, _item_ref, _locked) => {
            // Lock attribute not available in reaper_medium
            Ok(StepOutput::Unit)
        }
        ItemOp::SelectAllItems(p, selected) => {
            let _project = proj(p, outputs)?;
            medium.select_all_media_items(ReaperProjectContext::CurrentProject, *selected);
            Ok(StepOutput::Unit)
        }
        ItemOp::SetVolume(p, item_ref, volume) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                item_sw::set_item_info_value(medium, item_ptr, ItemAttributeKey::Vol, *volume);
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetFadeIn(p, item_ref, length, shape) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
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
                    proto_fade_to_reaper(*shape) as f64,
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetFadeOut(p, item_ref, length, shape) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
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
                    proto_fade_to_reaper(*shape) as f64,
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetLoopSource(p, item_ref, loop_source) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::LoopSrc,
                    if *loop_source { 1.0 } else { 0.0 },
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetBeatAttachMode(p, item_ref, mode) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
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
            Ok(StepOutput::Unit)
        }
        ItemOp::SetAutoStretch(p, item_ref, auto_stretch) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::AutoStretch,
                    if *auto_stretch { 1.0 } else { 0.0 },
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetColor(p, item_ref, color) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                let color_value = color.map(|c| (c as i32) | 0x01000000).unwrap_or(0);
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::CustomColor,
                    color_value as f64,
                );
            }
            Ok(StepOutput::Unit)
        }
        ItemOp::SetGroupId(p, item_ref, group_id) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            if let Some(item_ptr) = ReaperItem::resolve_item(item_ref, proj_ctx) {
                let group_value = group_id.map(|g| g as i32).unwrap_or(0);
                item_sw::set_item_info_value(
                    medium,
                    item_ptr,
                    ItemAttributeKey::GroupId,
                    group_value as f64,
                );
            }
            Ok(StepOutput::Unit)
        }
        // Complex operations — fall back to async dispatch
        _ => Err("sync dispatch not implemented for this ItemOp".to_string()),
    }
}

/// Convert a FadeShape proto value to REAPER's integer representation.
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

// =============================================================================
// Take dispatch
// =============================================================================

fn dispatch_take_sync(op: &TakeOp, outputs: &[Option<StepOutput>]) -> Result<StepOutput, String> {
    use crate::item::{ReaperItem, ReaperTake as ReaperTakeImpl};
    use crate::safe_wrappers::item as item_sw;

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let low = medium.low();

    match op {
        TakeOp::GetTakes(p, item_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            let count = item_sw::count_takes(low, item_ptr);
            let mut takes = Vec::with_capacity(count.max(0) as usize);
            for i in 0..count {
                if let Some(take) = item_sw::get_take(low, item_ptr, i)
                    && let Some(take_data) =
                        ReaperTakeImpl::media_take_to_take(item_ptr, take, i as u32)
                {
                    takes.push(take_data);
                }
            }
            Ok(StepOutput::TakeList(takes))
        }
        TakeOp::GetTake(p, item_ref, take_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            let take = ReaperTakeImpl::resolve_take(item_ptr, take_ref)
                .ok_or_else(|| "take not found".to_string())?;
            // Find the take index
            let count = item_sw::count_takes(low, item_ptr);
            let mut index = 0u32;
            for i in 0..count {
                if let Some(t) = item_sw::get_take(low, item_ptr, i) {
                    if t == take {
                        index = i as u32;
                        break;
                    }
                }
            }
            let take_data = ReaperTakeImpl::media_take_to_take(item_ptr, take, index);
            Ok(StepOutput::OptTake(take_data))
        }
        TakeOp::GetActiveTake(p, item_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            let active = item_sw::get_active_take(medium, item_ptr);
            let take_data = active.and_then(|take| {
                // Find active take index
                let count = item_sw::count_takes(low, item_ptr);
                let mut index = 0u32;
                for i in 0..count {
                    if let Some(t) = item_sw::get_take(low, item_ptr, i) {
                        if t == take {
                            index = i as u32;
                            break;
                        }
                    }
                }
                ReaperTakeImpl::media_take_to_take(item_ptr, take, index)
            });
            Ok(StepOutput::OptTake(take_data))
        }
        TakeOp::TakeCount(p, item_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            Ok(StepOutput::U32(item_sw::count_takes(low, item_ptr) as u32))
        }
        TakeOp::AddTake(p, item_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            let _new_take = item_sw::add_take_to_media_item(medium, item_ptr);
            Ok(StepOutput::Unit)
        }
        TakeOp::SetActiveTake(p, item_ref, take_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            if let Some(take) = ReaperTakeImpl::resolve_take(item_ptr, take_ref) {
                item_sw::set_active_take(low, take);
            }
            Ok(StepOutput::Unit)
        }
        TakeOp::SetName(p, item_ref, take_ref, name) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            if let Some(take) = ReaperTakeImpl::resolve_take(item_ptr, take_ref) {
                let c_name =
                    std::ffi::CString::new(name.as_str()).map_err(|e| format!("CString: {e}"))?;
                item_sw::set_take_name(low, take, &c_name);
            }
            Ok(StepOutput::Unit)
        }
        TakeOp::SetVolume(p, item_ref, take_ref, volume) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            if let Some(take) = ReaperTakeImpl::resolve_take(item_ptr, take_ref) {
                item_sw::set_take_info_value(medium, take, TakeAttributeKey::Vol, *volume);
            }
            Ok(StepOutput::Unit)
        }
        TakeOp::SetPlayRate(p, item_ref, take_ref, rate) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            if let Some(take) = ReaperTakeImpl::resolve_take(item_ptr, take_ref) {
                item_sw::set_take_info_value(medium, take, TakeAttributeKey::PlayRate, *rate);
            }
            Ok(StepOutput::Unit)
        }
        TakeOp::SetPitch(p, item_ref, take_ref, semitones) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            if let Some(take) = ReaperTakeImpl::resolve_take(item_ptr, take_ref) {
                if let Ok(s) = Semitones::new(*semitones) {
                    item_sw::set_take_pitch(medium, take, s);
                }
            }
            Ok(StepOutput::Unit)
        }
        TakeOp::SetStartOffset(p, item_ref, take_ref, offset) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let proj_ctx = crate::project_context::resolve_project_context(&ctx);
            let item_ptr = ReaperItem::resolve_item(item_ref, proj_ctx)
                .ok_or_else(|| "item not found".to_string())?;
            if let Some(take) = ReaperTakeImpl::resolve_take(item_ptr, take_ref) {
                item_sw::set_take_info_value(
                    medium,
                    take,
                    TakeAttributeKey::StartOffs,
                    offset.as_seconds(),
                );
            }
            Ok(StepOutput::Unit)
        }
        // Stubs for operations not fully implemented
        TakeOp::GetSourceType(_p, _item_ref, _take_ref) => {
            Ok(StepOutput::SourceType(SourceType::Audio))
        }
        _ => Err("sync dispatch not implemented for this TakeOp".to_string()),
    }
}

// =============================================================================
// MIDI dispatch
// =============================================================================

fn dispatch_midi_sync(op: &MidiOp, outputs: &[Option<StepOutput>]) -> Result<StepOutput, String> {
    use crate::midi::ReaperMidi;
    use crate::safe_wrappers::item as item_sw;

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    match op {
        MidiOp::GetNotes(location) => {
            let take = ReaperMidi::resolve_take_for_location(medium, location)
                .ok_or_else(|| "MIDI take not found".to_string())?;
            Ok(StepOutput::MidiNoteList(ReaperMidi::read_notes(
                medium, take,
            )))
        }
        MidiOp::GetNotesInRange(location, range) => {
            let take = ReaperMidi::resolve_take_for_location(medium, location)
                .ok_or_else(|| "MIDI take not found".to_string())?;
            let notes: Vec<MidiNote> = ReaperMidi::read_notes(medium, take)
                .into_iter()
                .filter(|note| note.overlaps(range.start, range.end))
                .collect();
            Ok(StepOutput::MidiNoteList(notes))
        }
        MidiOp::GetSelectedNotes(location) => {
            let take = ReaperMidi::resolve_take_for_location(medium, location)
                .ok_or_else(|| "MIDI take not found".to_string())?;
            let notes: Vec<MidiNote> = ReaperMidi::read_notes(medium, take)
                .into_iter()
                .filter(|note| note.selected)
                .collect();
            Ok(StepOutput::MidiNoteList(notes))
        }
        MidiOp::NoteCount(location) => {
            let take = ReaperMidi::resolve_take_for_location(medium, location)
                .ok_or_else(|| "MIDI take not found".to_string())?;
            let count = ReaperMidi::read_notes(medium, take).len();
            Ok(StepOutput::U32(count as u32))
        }
        MidiOp::CreateMidiItem(p, t, start, end) => {
            let project = proj(p, outputs)?;
            let tref = resolve_track_arg(t, outputs)?;
            let track =
                resolve_track(&project, &tref).ok_or_else(|| "track not found".to_string())?;
            let track_ptr = track.raw().map_err(|e| format!("raw track: {e}"))?;
            let take = crate::midi::create_midi_item_on_main_thread(track_ptr, *start, *end);

            let location = take.and_then(|take| {
                let low = medium.low();
                let item = item_sw::get_take_item(low, take)?;
                let item_guid = crate::item::item_guid_string(medium, item);
                if item_guid.is_empty() {
                    return None;
                }
                let ctx = resolve_project_arg(p, outputs).ok()?;
                Some(MidiTakeLocation::active(ctx, ItemRef::Guid(item_guid)))
            });
            Ok(StepOutput::OptMidiTakeLocation(location))
        }
        MidiOp::AddNote(location, note) => {
            let take = ReaperMidi::resolve_take_for_location(medium, location)
                .ok_or_else(|| "MIDI take not found".to_string())?;
            let count_before = ReaperMidi::read_notes(medium, take).len() as u32;
            crate::midi::add_notes_to_take_on_main_thread(take, &[note.clone()]);
            Ok(StepOutput::U32(count_before))
        }
        MidiOp::AddNotes(location, notes) => {
            let take = ReaperMidi::resolve_take_for_location(medium, location)
                .ok_or_else(|| "MIDI take not found".to_string())?;
            let count_before = ReaperMidi::read_notes(medium, take).len() as u32;
            crate::midi::add_notes_to_take_on_main_thread(take, notes);
            let indices: Vec<u32> = (count_before..count_before + notes.len() as u32).collect();
            Ok(StepOutput::U32List(indices))
        }
        // Stubs for mutation operations (matching async behavior)
        MidiOp::DeleteNote(..)
        | MidiOp::DeleteNotes(..)
        | MidiOp::DeleteSelectedNotes(..)
        | MidiOp::SetNotePitch(..)
        | MidiOp::SetNoteVelocity(..)
        | MidiOp::SetNotePosition(..)
        | MidiOp::SetNoteLength(..)
        | MidiOp::SetNoteChannel(..)
        | MidiOp::SetNoteSelected(..)
        | MidiOp::SetNoteMuted(..)
        | MidiOp::SelectAllNotes(..)
        | MidiOp::TransposeNotes(..)
        | MidiOp::QuantizeNotes(..)
        | MidiOp::HumanizeNotes(..)
        | MidiOp::AddCc(..)
        | MidiOp::DeleteCc(..)
        | MidiOp::SetCcValue(..)
        | MidiOp::AddPitchBend(..) => Ok(StepOutput::Unit),
        // Stubs for CC/PitchBend/ProgramChange/Sysex queries (matching async stubs)
        MidiOp::GetCcs(..) => Ok(StepOutput::MidiCCList(Vec::new())),
        MidiOp::GetPitchBends(..) => Ok(StepOutput::MidiPitchBendList(Vec::new())),
        MidiOp::GetProgramChanges(..) => Ok(StepOutput::MidiProgramChangeList(Vec::new())),
        MidiOp::GetSysex(..) => Ok(StepOutput::MidiSysExList(Vec::new())),
    }
}

// =============================================================================
// TempoMap dispatch
// =============================================================================

fn dispatch_tempo_map_sync(
    op: &TempoMapOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    use crate::safe_wrappers::tempo as tempo_sw;
    use crate::safe_wrappers::time_map as tm_sw;

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let low = medium.low();

    /// Convert a tempo marker to a TempoPoint.
    fn marker_to_point(m: &crate::safe_wrappers::tempo::TempoMarkerRaw) -> TempoPoint {
        let time_sig = if m.timesig_num > 0 && m.timesig_denom > 0 {
            Some(TimeSignature::new(
                m.timesig_num as u32,
                m.timesig_denom as u32,
            ))
        } else {
            None
        };
        TempoPoint {
            position: Position::from_time(TimePosition::from_seconds(m.timepos)),
            bpm: m.bpm,
            time_signature: time_sig,
            shape: None,
            bezier_tension: None,
            selected: None,
            linear: Some(m.lineartempo),
        }
    }

    match op {
        TempoMapOp::GetTempoPoints(p) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            let count = medium.count_tempo_time_sig_markers(rctx);
            let mut points = Vec::with_capacity(count as usize);
            for i in 0..count {
                if let Some(m) = tempo_sw::get_tempo_marker(low, rctx, i as i32) {
                    points.push(marker_to_point(&m));
                }
            }
            Ok(StepOutput::TempoPointList(points))
        }
        TempoMapOp::GetTempoPoint(p, index) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            let point =
                tempo_sw::get_tempo_marker(low, rctx, *index as i32).map(|m| marker_to_point(&m));
            Ok(StepOutput::OptTempoPoint(point))
        }
        TempoMapOp::TempoPointCount(p) => {
            let _project = proj(p, outputs)?;
            let count = medium.count_tempo_time_sig_markers(ReaperProjectContext::CurrentProject);
            Ok(StepOutput::U32(count))
        }
        TempoMapOp::GetTempoAt(p, seconds) => {
            let _project = proj(p, outputs)?;
            let bpm = if let Ok(pos) = PositionInSeconds::new(*seconds) {
                medium
                    .time_map_2_get_divided_bpm_at_time(ReaperProjectContext::CurrentProject, pos)
                    .get()
            } else {
                reaper.current_project().tempo().bpm().get()
            };
            Ok(StepOutput::F64(bpm))
        }
        TempoMapOp::GetTimeSignatureAt(p, seconds) => {
            let _project = proj(p, outputs)?;
            if let Ok(pos) = PositionInSeconds::new(*seconds) {
                let beat_info =
                    medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
                Ok(StepOutput::I32Pair(
                    beat_info.time_signature.numerator.get() as i32,
                    beat_info.time_signature.denominator.get() as i32,
                ))
            } else {
                Ok(StepOutput::I32Pair(4, 4))
            }
        }
        TempoMapOp::TimeToQn(p, seconds) => {
            let _project = proj(p, outputs)?;
            let qn = tm_sw::time_to_qn(low, ReaperProjectContext::CurrentProject, *seconds);
            Ok(StepOutput::F64(qn))
        }
        TempoMapOp::QnToTime(p, qn) => {
            let _project = proj(p, outputs)?;
            let time = tm_sw::qn_to_time(low, ReaperProjectContext::CurrentProject, *qn);
            Ok(StepOutput::F64(time))
        }
        TempoMapOp::TimeToMusical(p, seconds) => {
            let _project = proj(p, outputs)?;
            if let Ok(pos) = PositionInSeconds::new(*seconds) {
                let result =
                    medium.time_map_2_time_to_beats(ReaperProjectContext::CurrentProject, pos);
                let measure = result.measure_index + 1;
                let beats_since = result.beats_since_measure.get();
                let beat_in_measure = beats_since.floor() as i32 + 1;
                let fraction = beats_since.fract();
                Ok(StepOutput::MusicalTime(measure, beat_in_measure, fraction))
            } else {
                Ok(StepOutput::MusicalTime(1, 1, 0.0))
            }
        }
        TempoMapOp::MusicalToTime(p, measure, beat, fraction) => {
            let _project = proj(p, outputs)?;
            let measure_0based = (*measure - 1).max(0);
            let beat_0based = (*beat - 1).max(0) as f64 + *fraction;
            if let Ok(beats) = reaper_medium::PositionInBeats::new(beat_0based) {
                let result = medium.time_map_2_beats_to_time(
                    ReaperProjectContext::CurrentProject,
                    MeasureMode::FromMeasureAtIndex(measure_0based),
                    beats,
                );
                Ok(StepOutput::F64(result.get()))
            } else {
                Ok(StepOutput::F64(0.0))
            }
        }
        TempoMapOp::AddTempoPoint(p, seconds, bpm) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            let result = tempo_sw::set_tempo_marker(
                low, rctx, -1, // add new
                *seconds, -1,   // measurepos (auto)
                -1.0, // beatpos (auto)
                *bpm, 0,     // timesig_num (don't change)
                0,     // timesig_denom (don't change)
                false, // lineartempo
            );
            if result {
                let count = medium.count_tempo_time_sig_markers(rctx);
                Ok(StepOutput::U32(count.saturating_sub(1)))
            } else {
                Ok(StepOutput::U32(0))
            }
        }
        TempoMapOp::RemoveTempoPoint(p, index) => {
            let _project = proj(p, outputs)?;
            tempo_sw::delete_tempo_marker(low, ReaperProjectContext::CurrentProject, *index as i32);
            Ok(StepOutput::Unit)
        }
        TempoMapOp::SetTempoAtPoint(p, index, bpm) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            if let Some(m) = tempo_sw::get_tempo_marker(low, rctx, *index as i32) {
                tempo_sw::set_tempo_marker(
                    low,
                    rctx,
                    *index as i32,
                    m.timepos,
                    m.measurepos,
                    m.beatpos,
                    *bpm,
                    m.timesig_num,
                    m.timesig_denom,
                    m.lineartempo,
                );
            }
            Ok(StepOutput::Unit)
        }
        TempoMapOp::SetTimeSignatureAtPoint(p, index, numerator, denominator) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            if let Some(m) = tempo_sw::get_tempo_marker(low, rctx, *index as i32) {
                tempo_sw::set_tempo_marker(
                    low,
                    rctx,
                    *index as i32,
                    m.timepos,
                    m.measurepos,
                    m.beatpos,
                    m.bpm,
                    *numerator,
                    *denominator,
                    m.lineartempo,
                );
            }
            Ok(StepOutput::Unit)
        }
        TempoMapOp::MoveTempoPoint(p, index, seconds) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            if let Some(m) = tempo_sw::get_tempo_marker(low, rctx, *index as i32) {
                tempo_sw::set_tempo_marker(
                    low,
                    rctx,
                    *index as i32,
                    *seconds,
                    -1,   // auto measure
                    -1.0, // auto beat
                    m.bpm,
                    m.timesig_num,
                    m.timesig_denom,
                    m.lineartempo,
                );
            }
            Ok(StepOutput::Unit)
        }
        TempoMapOp::GetDefaultTempo(p) => {
            let _project = proj(p, outputs)?;
            Ok(StepOutput::F64(
                reaper.current_project().tempo().bpm().get(),
            ))
        }
        TempoMapOp::SetDefaultTempo(p, bpm) => {
            let _project = proj(p, outputs)?;
            if let Ok(bpm_value) = reaper_medium::Bpm::new(*bpm) {
                let tempo = ReaperTempo::from_bpm(bpm_value);
                let _ = reaper
                    .current_project()
                    .set_tempo(tempo, UndoBehavior::OmitUndoPoint);
            }
            Ok(StepOutput::Unit)
        }
        TempoMapOp::GetDefaultTimeSignature(p) => {
            let _project = proj(p, outputs)?;
            let measure_info =
                medium.time_map_get_measure_info(ReaperProjectContext::CurrentProject, 0);
            Ok(StepOutput::I32Pair(
                measure_info.time_signature.numerator.get() as i32,
                measure_info.time_signature.denominator.get() as i32,
            ))
        }
        TempoMapOp::SetDefaultTimeSignature(p, numerator, denominator) => {
            let _project = proj(p, outputs)?;
            let rctx = ReaperProjectContext::CurrentProject;
            let bpm = reaper.current_project().tempo().bpm().get();
            let count = medium.count_tempo_time_sig_markers(rctx);
            let mut found_at_zero = false;
            for i in 0..count {
                if let Some(m) = tempo_sw::get_tempo_marker(low, rctx, i as i32) {
                    if m.timepos < 0.001 {
                        tempo_sw::set_tempo_marker(
                            low,
                            rctx,
                            i as i32,
                            0.0,
                            0,
                            0.0,
                            m.bpm,
                            *numerator,
                            *denominator,
                            m.lineartempo,
                        );
                        found_at_zero = true;
                        break;
                    }
                }
            }
            if !found_at_zero {
                tempo_sw::set_tempo_marker(
                    low,
                    rctx,
                    -1,
                    0.0,
                    0,
                    0.0,
                    bpm,
                    *numerator,
                    *denominator,
                    false,
                );
            }
            Ok(StepOutput::Unit)
        }
    }
}

// =============================================================================
// Position Conversion dispatch
// =============================================================================

fn dispatch_position_conversion_sync(
    op: &PositionConversionOp,
    outputs: &[Option<StepOutput>],
) -> Result<StepOutput, String> {
    use crate::safe_wrappers::time_map as sw;
    use daw_proto::MeasureMode as ProtoMeasureMode;
    use daw_proto::PositionInSeconds as ProtoPositionInSeconds;

    match op {
        PositionConversionOp::TimeToBeats(p, pos, mode) => {
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = project.context();
            let time = pos.as_seconds();

            let result = sw::time_to_beats(low, proj_ctx, time);

            let final_measure = match mode {
                ProtoMeasureMode::IgnoreMeasure => 0,
                ProtoMeasureMode::FromMeasureAtIndex(idx) => idx - result.measure_index,
            };

            let ts = sw::get_time_sig_at_time(low, proj_ctx, time);

            Ok(StepOutput::TimeToBeatsResult(TimeToBeatsResult {
                full_beats: PositionInBeats::from_beats(result.full_beats),
                measure_index: final_measure,
                beats_since_measure: PositionInBeats::from_beats(result.beats_frac),
                time_signature: TimeSignature::new(ts.num as u32, ts.denom as u32),
            }))
        }
        PositionConversionOp::BeatsToTime(p, pos, mode) => {
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = project.context();
            let full_beats = pos.as_beats();

            let adjusted_beats = match mode {
                ProtoMeasureMode::IgnoreMeasure => full_beats,
                ProtoMeasureMode::FromMeasureAtIndex(measure_idx) => {
                    let measure_start_time = sw::get_measure_info(low, proj_ctx, *measure_idx);
                    let tb = sw::time_to_beats(low, proj_ctx, measure_start_time);
                    tb.full_beats + full_beats
                }
            };

            let time = sw::beats_to_time(low, proj_ctx, adjusted_beats, None);
            Ok(StepOutput::PositionInSeconds(
                ProtoPositionInSeconds::from_seconds(time),
            ))
        }
        PositionConversionOp::TimeToQuarterNotes(p, pos) => {
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = project.context();
            let time = pos.as_seconds();

            let qn_position = sw::time_to_qn(low, proj_ctx, time);
            let minfo = sw::qn_to_measures(low, proj_ctx, qn_position);
            let qn_since_measure = qn_position - minfo.qn_start;
            let ts = sw::get_time_sig_at_time(low, proj_ctx, time);

            Ok(StepOutput::TimeToQuarterNotesResult(
                TimeToQuarterNotesResult {
                    quarter_notes: PositionInQuarterNotes::from_quarter_notes(qn_position),
                    measure_index: minfo.measure_index,
                    quarter_notes_since_measure: PositionInQuarterNotes::from_quarter_notes(
                        qn_since_measure,
                    ),
                    time_signature: TimeSignature::new(ts.num as u32, ts.denom as u32),
                },
            ))
        }
        PositionConversionOp::QuarterNotesToTime(p, pos) => {
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let time = sw::qn_to_time(low, project.context(), pos.as_quarter_notes());
            Ok(StepOutput::PositionInSeconds(
                ProtoPositionInSeconds::from_seconds(time),
            ))
        }
        PositionConversionOp::QuarterNotesToMeasure(p, pos) => {
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = project.context();
            let qn = pos.as_quarter_notes();

            let minfo = sw::qn_to_measures(low, proj_ctx, qn);
            let time = sw::qn_to_time(low, proj_ctx, qn);
            let ts = sw::get_time_sig_at_time(low, proj_ctx, time);

            Ok(StepOutput::QuarterNotesToMeasureResult(
                QuarterNotesToMeasureResult {
                    measure_index: minfo.measure_index,
                    start: PositionInQuarterNotes::from_quarter_notes(minfo.qn_start),
                    end: PositionInQuarterNotes::from_quarter_notes(minfo.qn_end),
                    time_signature: TimeSignature::new(ts.num as u32, ts.denom as u32),
                },
            ))
        }
        PositionConversionOp::BeatsToQuarterNotes(p, pos) => {
            // beats -> time -> quarter notes
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = project.context();
            let time = sw::beats_to_time(low, proj_ctx, pos.as_beats(), None);
            let qn = sw::time_to_qn(low, proj_ctx, time);
            Ok(StepOutput::PositionInQuarterNotes(
                PositionInQuarterNotes::from_quarter_notes(qn),
            ))
        }
        PositionConversionOp::QuarterNotesToBeats(p, pos) => {
            // quarter notes -> time -> beats
            let project = proj(p, outputs)?;
            let low = Reaper::get().medium_reaper().low();
            let proj_ctx = project.context();
            let time = sw::qn_to_time(low, proj_ctx, pos.as_quarter_notes());
            let result = sw::time_to_beats(low, proj_ctx, time);
            Ok(StepOutput::PositionInBeats(PositionInBeats::from_beats(
                result.full_beats,
            )))
        }
    }
}

// =============================================================================
// Audio Accessor dispatch
// =============================================================================

fn dispatch_audio_accessor_sync(
    op: &AudioAccessorOp,
    outputs: &[Option<StepOutput>],
    svc: &crate::ReaperAudioAccessor,
) -> Result<StepOutput, String> {
    use crate::safe_wrappers::audio_accessor as aa_sw;
    use crate::track::resolve_track_pub;

    match op {
        AudioAccessorOp::CreateTrackAccessor(p, t) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let tr = resolve_track_arg(t, outputs)?;
            let project = crate::track::resolve_project(&ctx)
                .ok_or_else(|| "project not found".to_string())?;
            let track =
                resolve_track_pub(&project, &tr).ok_or_else(|| "track not found".to_string())?;
            let raw = track.raw().map_err(|e| format!("track raw error: {}", e))?;
            let low = Reaper::get().medium_reaper().low();
            let accessor = aa_sw::create_track_audio_accessor(low, raw);
            let ptr = aa_sw::SendableAccessorPtr::new(accessor);
            Ok(StepOutput::OptStr(svc.store(ptr)))
        }
        AudioAccessorOp::CreateTakeAccessor(p, item_ref, take_ref) => {
            let ctx = resolve_project_arg(p, outputs)?;
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let reaper_project_ctx = match &ctx {
                ProjectContext::Current => reaper_medium::ProjectContext::CurrentProject,
                ProjectContext::Project(guid) => {
                    let proj = crate::project_context::find_project_by_guid(guid)
                        .ok_or_else(|| "project not found".to_string())?;
                    reaper_medium::ProjectContext::Proj(proj.raw())
                }
            };
            let midi_item =
                crate::midi::ReaperMidi::resolve_item(medium, reaper_project_ctx, item_ref)
                    .ok_or_else(|| "item not found".to_string())?;
            let midi_take = crate::midi::ReaperMidi::resolve_take(medium, midi_item, take_ref)
                .ok_or_else(|| "take not found".to_string())?;
            let low = medium.low();
            let accessor = aa_sw::create_take_audio_accessor(low, midi_take);
            let ptr = aa_sw::SendableAccessorPtr::new(accessor);
            Ok(StepOutput::OptStr(svc.store(ptr)))
        }
        AudioAccessorOp::HasStateChanged(id) => {
            let ptr = svc
                .get_ptr(id)
                .ok_or_else(|| format!("unknown accessor ID '{}'", id))?;
            let low = Reaper::get().medium_reaper().low();
            Ok(StepOutput::Bool(aa_sw::audio_accessor_state_changed(
                low,
                ptr.get(),
            )))
        }
        AudioAccessorOp::GetSamples(req) => {
            let ptr = svc
                .get_ptr(&req.accessor_id)
                .ok_or_else(|| format!("unknown accessor ID '{}'", req.accessor_id))?;
            let low = Reaper::get().medium_reaper().low();
            let buf_size = (req.num_channels * req.num_samples) as usize;
            let mut buf = vec![0.0f64; buf_size];
            let result = aa_sw::get_audio_accessor_samples(
                low,
                ptr.get(),
                req.sample_rate as i32,
                req.num_channels as i32,
                req.start_time,
                req.num_samples as i32,
                &mut buf,
            );
            if result <= 0 {
                return Ok(StepOutput::AudioSampleData(AudioSampleData::default()));
            }
            let actual_samples = result as u32;
            let actual_size = (req.num_channels * actual_samples) as usize;
            buf.truncate(actual_size);
            Ok(StepOutput::AudioSampleData(AudioSampleData {
                samples: buf,
                sample_rate: req.sample_rate,
                num_channels: req.num_channels,
                num_samples: actual_samples,
            }))
        }
        AudioAccessorOp::DestroyAccessor(id) => {
            let ptr = svc
                .remove_ptr(id)
                .ok_or_else(|| format!("unknown accessor ID '{}'", id))?;
            let low = Reaper::get().medium_reaper().low();
            aa_sw::destroy_audio_accessor(low, ptr.get());
            Ok(StepOutput::Unit)
        }
    }
}

// =============================================================================
// MIDI Analysis dispatch
// =============================================================================

fn dispatch_midi_analysis_sync(op: &MidiAnalysisOp) -> Result<StepOutput, String> {
    use crate::ReaperMidiAnalysis;

    match op {
        MidiAnalysisOp::SourceFingerprint(req) => {
            let project = ReaperMidiAnalysis::resolve_project(&req.project)
                .ok_or_else(|| "Project not found".to_string())?;
            let track = ReaperMidiAnalysis::find_track_by_tag(&project, req.track_tag.as_deref())
                .ok_or_else(|| {
                let tag = req.track_tag.as_deref().unwrap_or("<none>");
                format!("No track matched tag '{}'", tag)
            })?;
            let track_name = track
                .name()
                .map(|name| name.to_str().to_string())
                .unwrap_or_else(|| "Unnamed Track".to_string());
            let (take, item_start_time) = ReaperMidiAnalysis::get_first_midi_take(&track)
                .ok_or_else(|| format!("Track '{}' has no MIDI take", track_name))?;
            let notes = ReaperMidiAnalysis::read_keyflow_notes(take);
            if notes.is_empty() {
                return Err("No MIDI notes found".to_string());
            }
            let item_start_tick = ReaperMidiAnalysis::time_to_tick(project, item_start_time);
            let import_notes = ReaperMidiAnalysis::import_notes(&notes, item_start_tick);
            let markers = ReaperMidiAnalysis::gather_markers(project);
            Ok(StepOutput::ResultString(Ok(
                ReaperMidiAnalysis::make_source_fingerprint(&track_name, &import_notes, &markers),
            )))
        }
        MidiAnalysisOp::GenerateChartData(req) => {
            let project = ReaperMidiAnalysis::resolve_project(&req.project)
                .ok_or_else(|| "Project not found".to_string())?;
            let track = ReaperMidiAnalysis::find_track_by_tag(&project, req.track_tag.as_deref())
                .ok_or_else(|| {
                let tag = req.track_tag.as_deref().unwrap_or("<none>");
                format!("No track matched tag '{}'", tag)
            })?;
            let track_name = track
                .name()
                .map(|name| name.to_str().to_string())
                .unwrap_or_else(|| "Unnamed Track".to_string());
            let (take, item_start_time) = ReaperMidiAnalysis::get_first_midi_take(&track)
                .ok_or_else(|| format!("Track '{}' has no MIDI take", track_name))?;
            let notes = ReaperMidiAnalysis::read_keyflow_notes(take);
            Ok(StepOutput::ResultMidiChartData(
                ReaperMidiAnalysis::build_chart_data(project, track_name, notes, item_start_time),
            ))
        }
    }
}
