//! REAPER Track Service Implementation
//!
//! Implements TrackService for REAPER by dispatching operations to the main thread
//! using TaskSupport from reaper-high. Follows the same pattern as ReaperFx and
//! ReaperTransport.

use daw_proto::{ProjectContext, Track, TrackRef, TrackService};
use reaper_high::{GroupingBehavior, Reaper};
use reaper_medium::GangBehavior;
use roam::Context;
use tracing::warn;

use crate::project_context::find_project_by_guid;
use crate::transport::task_support;

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

/// Resolve a TrackRef to a reaper-high Track within a project
fn resolve_track(
    project: &reaper_high::Project,
    track_ref: &TrackRef,
) -> Option<reaper_high::Track> {
    match track_ref {
        TrackRef::Guid(guid) => {
            // Linear scan to match GUID string
            for i in 0..project.track_count() {
                if let Some(track) = project.track_by_index(i) {
                    if track.guid().to_string_without_braces() == *guid {
                        return Some(track);
                    }
                }
            }
            None
        }
        TrackRef::Index(idx) => project.track_by_index(*idx),
        TrackRef::Master => project.master_track().ok(),
    }
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

    // Parent GUID: walk backwards to find the enclosing folder
    // This is expensive to compute for every track, so skip it for now.
    // The UI uses folder_depth for indentation instead.
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

// =============================================================================
// TrackService Implementation
// =============================================================================

impl TrackService for ReaperTrack {
    // =========================================================================
    // Query Methods
    // =========================================================================

    async fn get_tracks(&self, _cx: &Context, project: ProjectContext) -> Vec<Track> {
        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return vec![];
        };

        ts.main_thread_future(move || {
            let Some(proj) = resolve_project(&project) else {
                return vec![];
            };
            proj.tracks().map(|t| build_track_info(&t)).collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn get_track(
        &self,
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
    ) -> Option<Track> {
        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let t = resolve_track(&proj, &track)?;
            Some(build_track_info(&t))
        })
        .await
        .unwrap_or(None)
    }

    async fn track_count(&self, _cx: &Context, project: ProjectContext) -> u32 {
        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return 0;
        };

        ts.main_thread_future(move || {
            resolve_project(&project)
                .map(|p| p.track_count())
                .unwrap_or(0)
        })
        .await
        .unwrap_or(0)
    }

    async fn get_selected_tracks(&self, _cx: &Context, project: ProjectContext) -> Vec<Track> {
        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return vec![];
        };

        ts.main_thread_future(move || {
            let Some(proj) = resolve_project(&project) else {
                return vec![];
            };
            proj.tracks()
                .filter(|t| t.is_selected())
                .map(|t| build_track_info(&t))
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn get_master_track(&self, _cx: &Context, project: ProjectContext) -> Option<Track> {
        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let master = proj.master_track().ok()?;
            Some(build_track_info(&master))
        })
        .await
        .unwrap_or(None)
    }

    // =========================================================================
    // Mute/Solo/Arm
    // =========================================================================

    async fn set_muted(
        &self,
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        muted: bool,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        soloed: bool,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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

    async fn set_solo_exclusive(&self, _cx: &Context, project: ProjectContext, track: TrackRef) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            // Unsolo all first
            for t in proj.tracks() {
                if t.is_solo() {
                    t.unsolo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
                }
            }
            // Solo the target
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.solo(GangBehavior::DenyGang, GroupingBehavior::PreventGrouping);
        });
    }

    async fn clear_all_solo(&self, _cx: &Context, project: ProjectContext) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        armed: bool,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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

    // =========================================================================
    // Volume/Pan
    // =========================================================================

    async fn set_volume(
        &self,
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        volume: f64,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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

    async fn set_pan(&self, _cx: &Context, project: ProjectContext, track: TrackRef, pan: f64) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        selected: bool,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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

    async fn select_exclusive(&self, _cx: &Context, project: ProjectContext, track: TrackRef) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some(t) = resolve_track(&proj, &track) else {
                return;
            };
            t.select_exclusively();
        });
    }

    async fn clear_selection(&self, _cx: &Context, project: ProjectContext) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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

    async fn mute_all(&self, _cx: &Context, project: ProjectContext) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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

    async fn unmute_all(&self, _cx: &Context, project: ProjectContext) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        name: String,
        at_index: Option<u32>,
    ) -> String {
        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return String::new();
        };

        ts.main_thread_future(move || {
            let Some(proj) = resolve_project(&project) else {
                return String::new();
            };
            let index = at_index.unwrap_or_else(|| proj.track_count());
            let Ok(new_track) = proj.insert_track_at(index) else {
                return String::new();
            };
            new_track.set_name(name.as_str());
            new_track.guid().to_string_without_braces()
        })
        .await
        .unwrap_or_default()
    }

    async fn remove_track(&self, _cx: &Context, project: ProjectContext, track: TrackRef) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        name: String,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        color: u32,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        visible: bool,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        visible: bool,
    ) {
        let Some(ts) = task_support() else { return };

        let _ = ts.do_later_in_main_thread_asap(move || {
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
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        chunk: String,
    ) -> Result<(), String> {
        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".to_string());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            let t = resolve_track(&proj, &track).ok_or_else(|| "Track not found".to_string())?;
            let chunk_obj = reaper_high::Chunk::new(chunk);
            t.set_chunk(chunk_obj)
                .map_err(|e| format!("set_chunk failed: {e}"))
        })
        .await
        .unwrap_or_else(|_| Err("main_thread_future cancelled".to_string()))
    }

    async fn remove_all_tracks(
        &self,
        _cx: &Context,
        project: ProjectContext,
    ) -> Result<(), String> {
        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".to_string());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "Project not found".to_string())?;
            // Delete from highest index to lowest to avoid index shifting
            let count = proj.track_count();
            for i in (0..count).rev() {
                if let Some(t) = proj.track_by_index(i) {
                    proj.remove_track(&t);
                }
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main_thread_future cancelled".to_string()))
    }
}
