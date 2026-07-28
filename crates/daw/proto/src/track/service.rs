//! Tracks service trait.
//!
//! Stateless singleton backends — `ProjectContext` flows through every
//! call. `#[architect::rpc]` derives the async vox client + `serve`
//! function; backends impl `Tracks` directly. See
//! `daw-proto/src/marker/service.rs` for the same pattern.
//!
//! Scope was trimmed from the previous async `TrackService` to the
//! verbs the daw-control facade actually drives. Broader surfaces
//! (track chunks, hierarchy apply, ext state, input monitoring,
//! subscribe) land on follow-on sibling traits if/when a real consumer
//! needs them.

use super::event::TrackStreamEvent;
use super::{RecordInput, ReorderTracksBehavior, Track, TrackRef};
use crate::batch::{ProjectArg, TrackArg};
use crate::{DawResult, ProjectContext};
use facet::Facet;

/// Track-scoped ext state payload — groups section + key + value into
/// a single Facet struct. Kept here so batch op definitions can name
/// it without dragging in the full ext-state surface.
#[derive(Clone, Debug, Facet)]
pub struct TrackExtStateRequest {
    pub section: String,
    pub key: String,
    pub value: String,
}

#[architect::rpc(ops(ProjectContext as ProjectArg, TrackRef as TrackArg), scopes(project: ProjectContext, track: TrackRef))]
pub trait Tracks {
    // ── Queries ─────────────────────────────────────────────────────

    /// Every track in the project, in mixer order.
    fn all(&self, project: ProjectContext) -> Vec<Track>;

    /// One track by reference (guid or index), if it still exists.
    fn get(&self, project: ProjectContext, track: TrackRef) -> Option<Track>;

    /// Total number of tracks (master excluded).
    fn count(&self, project: ProjectContext) -> u32;

    /// All currently selected tracks.
    fn selected(&self, project: ProjectContext) -> Vec<Track>;

    /// The master track.
    fn master(&self, project: ProjectContext) -> Option<Track>;

    // ── Mute / solo / arm ───────────────────────────────────────────

    fn set_muted(&self, project: ProjectContext, track: TrackRef, muted: bool) -> DawResult<()>;

    fn set_soloed(&self, project: ProjectContext, track: TrackRef, soloed: bool) -> DawResult<()>;

    fn set_solo_exclusive(&self, project: ProjectContext, track: TrackRef) -> DawResult<()>;

    fn clear_all_solo(&self, project: ProjectContext) -> DawResult<()>;

    fn set_armed(&self, project: ProjectContext, track: TrackRef, armed: bool) -> DawResult<()>;

    // ── Volume / pan ────────────────────────────────────────────────

    fn set_volume(&self, project: ProjectContext, track: TrackRef, volume: f64) -> DawResult<()>;

    fn set_pan(&self, project: ProjectContext, track: TrackRef, pan: f64) -> DawResult<()>;

    /// Set polarity/phase inversion (flip the signal's sign).
    /// Set the track automation mode (trim/read/touch/write/latch).
    fn set_automation_mode(
        &self,
        project: ProjectContext,
        track: TrackRef,
        mode: crate::primitives::AutomationMode,
    ) -> DawResult<()>;

    /// Set record-input monitoring (off / on / tape-auto).
    fn set_input_monitor(
        &self,
        project: ProjectContext,
        track: TrackRef,
        monitor: super::InputMonitoringMode,
    ) -> DawResult<()>;

    fn set_phase_inverted(
        &self,
        project: ProjectContext,
        track: TrackRef,
        inverted: bool,
    ) -> DawResult<()>;

    // ── Track groups ────────────────────────────────────────────────
    //
    // The DAW's fixed set of track-group slots (REAPER: 128, addressed
    // 1-based). A slot bundles the flag families (volume/mute/solo/…) so
    // members move together.

    /// Set the display name of track-group `slot` (1-based).
    fn set_group_name(&self, project: ProjectContext, slot: u32, name: &str) -> DawResult<()>;

    /// First slot in `[band_start, band_end]` (inclusive, 1-based) that no
    /// track belongs to, or `None` if the band is full.
    fn first_free_group_slot(
        &self,
        project: ProjectContext,
        band_start: u32,
        band_end: u32,
    ) -> Option<u32>;

    /// Add or remove `track` from track-group `slot` as a *mutual* member —
    /// every flag family, both lead and follow — so any member's
    /// mute/solo/volume/etc. moves the whole group equally.
    fn set_group_membership(
        &self,
        project: ProjectContext,
        track: TrackRef,
        slot: u32,
        member: bool,
    ) -> DawResult<()>;

    // ── Selection ───────────────────────────────────────────────────

    fn set_selected(
        &self,
        project: ProjectContext,
        track: TrackRef,
        selected: bool,
    ) -> DawResult<()>;

    fn select_exclusive(&self, project: ProjectContext, track: TrackRef) -> DawResult<()>;

    fn clear_selection(&self, project: ProjectContext) -> DawResult<()>;

    // ── Bulk mute ───────────────────────────────────────────────────

    fn mute_all(&self, project: ProjectContext) -> DawResult<()>;

    fn unmute_all(&self, project: ProjectContext) -> DawResult<()>;

    // ── Mutation ────────────────────────────────────────────────────

    /// Insert a track. `at_index = None` appends at the end. Returns
    /// the new track's guid.
    fn add(&self, project: ProjectContext, name: &str, at_index: Option<u32>) -> DawResult<String>;

    fn remove(&self, project: ProjectContext, track: TrackRef) -> DawResult<()>;

    fn remove_all(&self, project: ProjectContext) -> DawResult<()>;

    fn rename(&self, project: ProjectContext, track: TrackRef, name: &str) -> DawResult<()>;

    fn set_color(&self, project: ProjectContext, track: TrackRef, color: u32) -> DawResult<()>;

    /// Set REAPER folder-depth change for a track.
    fn set_folder_depth(
        &self,
        project: ProjectContext,
        track: TrackRef,
        folder_depth: i32,
    ) -> DawResult<()>;

    /// Set the track channel count.
    fn set_num_channels(
        &self,
        project: ProjectContext,
        track: TrackRef,
        num_channels: u32,
    ) -> DawResult<()>;

    /// Set the track record input source.
    fn set_record_input(
        &self,
        project: ProjectContext,
        track: TrackRef,
        input: RecordInput,
    ) -> DawResult<()>;

    /// Move all currently selected tracks to `index`.
    fn reorder_selected(
        &self,
        project: ProjectContext,
        index: u32,
        behavior: ReorderTracksBehavior,
    ) -> DawResult<()>;

    /// Set TCP/MCP visibility for a track.
    fn set_visibility(
        &self,
        project: ProjectContext,
        track: TrackRef,
        visible_in_tcp: bool,
        visible_in_mixer: bool,
    ) -> DawResult<()>;

    /// Set the TCP height override for a track. `height_pixels = 0` clears the
    /// override and lets REAPER choose the default height.
    fn set_tcp_height(
        &self,
        project: ProjectContext,
        track: TrackRef,
        height_pixels: u32,
    ) -> DawResult<()>;

    // ── Streaming ───────────────────────────────────────────────────

    /// Track add/remove/modify events across all open projects, as
    /// they happen. Served from the backend's `TracksStreamSource`
    /// hub; subscribers filter by `project_guid` on the envelope.
    #[subscribe]
    fn events(&self) -> TrackStreamEvent;
}
