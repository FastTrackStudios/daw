//! Track service trait
//!
//! Defines the RPC interface for track operations.

use super::{InputMonitoringMode, RecordInput, Track, TrackEvent, TrackRef};
use crate::ProjectContext;
use facet::Facet;
use roam::{Tx, service};

/// Request for track-scoped extended state operations.
///
/// Groups section + key (+ optional value) into a single Facet struct
/// to stay within the 4-element tuple limit for service method params.
#[derive(Clone, Debug, Facet)]
pub struct TrackExtStateRequest {
    pub section: String,
    pub key: String,
    pub value: String,
}

/// Service for managing tracks in a DAW project
///
/// Tracks are the fundamental organizational unit in a DAW mixer,
/// representing audio or MIDI channels with associated FX chains,
/// routing, and automation.
#[service]
pub trait TrackService {
    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all tracks in the project
    async fn get_tracks(&self, project: ProjectContext) -> Vec<Track>;

    /// Get a specific track by reference
    async fn get_track(&self, project: ProjectContext, track: TrackRef) -> Option<Track>;

    /// Get the total number of tracks (excluding master)
    async fn track_count(&self, project: ProjectContext) -> u32;

    /// Get all currently selected tracks
    async fn get_selected_tracks(&self, project: ProjectContext) -> Vec<Track>;

    /// Get the master track
    async fn get_master_track(&self, project: ProjectContext) -> Option<Track>;

    // =========================================================================
    // Mute/Solo/Arm
    // =========================================================================

    /// Set the mute state of a track
    async fn set_muted(&self, project: ProjectContext, track: TrackRef, muted: bool);

    /// Set the solo state of a track
    async fn set_soloed(&self, project: ProjectContext, track: TrackRef, soloed: bool);

    /// Solo a track exclusively (unsolo all others)
    async fn set_solo_exclusive(&self, project: ProjectContext, track: TrackRef);

    /// Clear solo from all tracks
    async fn clear_all_solo(&self, project: ProjectContext);

    /// Set the arm (record-ready) state of a track
    async fn set_armed(&self, project: ProjectContext, track: TrackRef, armed: bool);

    /// Set the input monitoring mode for a track.
    ///
    /// Controls whether input signal passes through to FX and output:
    /// - `Off` — no monitoring
    /// - `Normal` — always monitor (needed for MIDI VKB → FX flow)
    /// - `NotWhenPlaying` — tape-style auto-monitoring
    async fn set_input_monitoring(
        &self,
        project: ProjectContext,
        track: TrackRef,
        mode: InputMonitoringMode,
    );

    /// Set the record input source for a track.
    ///
    /// Use `RecordInput::midi_virtual_keyboard()` to receive from REAPER's
    /// virtual MIDI keyboard queue (pairs with `StuffMIDIMessage`).
    async fn set_record_input(&self, project: ProjectContext, track: TrackRef, input: RecordInput);

    // =========================================================================
    // Volume/Pan
    // =========================================================================

    /// Set track volume (0.0 = -inf dB, 1.0 = 0 dB)
    async fn set_volume(&self, project: ProjectContext, track: TrackRef, volume: f64);

    /// Set track pan (-1.0 = left, 0.0 = center, 1.0 = right)
    async fn set_pan(&self, project: ProjectContext, track: TrackRef, pan: f64);

    // =========================================================================
    // Selection
    // =========================================================================

    /// Set the selection state of a track
    async fn set_selected(&self, project: ProjectContext, track: TrackRef, selected: bool);

    /// Select a track exclusively (deselect all others)
    async fn select_exclusive(&self, project: ProjectContext, track: TrackRef);

    /// Clear selection from all tracks
    async fn clear_selection(&self, project: ProjectContext);

    // =========================================================================
    // Batch Operations
    // =========================================================================

    /// Mute all tracks
    async fn mute_all(&self, project: ProjectContext);

    /// Unmute all tracks
    async fn unmute_all(&self, project: ProjectContext);

    // =========================================================================
    // Visibility
    // =========================================================================

    /// Set track visibility in the TCP (track control panel / arrange view)
    async fn set_visible_in_tcp(&self, project: ProjectContext, track: TrackRef, visible: bool);

    /// Set track visibility in the MCP (mixer control panel)
    async fn set_visible_in_mixer(&self, project: ProjectContext, track: TrackRef, visible: bool);

    // =========================================================================
    // Track Management
    // =========================================================================

    /// Insert a new track. If `at_index` is Some, inserts at that position
    /// (0-based, shifting existing tracks down); if None, appends at the end.
    /// Returns the GUID of the newly created track.
    async fn add_track(
        &self,
        project: ProjectContext,
        name: String,
        at_index: Option<u32>,
    ) -> String;

    /// Remove a track from the project
    async fn remove_track(&self, project: ProjectContext, track: TrackRef);

    /// Rename a track
    async fn rename_track(&self, project: ProjectContext, track: TrackRef, name: String);

    /// Set track color (0xRRGGBB format, or 0 for default)
    async fn set_track_color(&self, project: ProjectContext, track: TrackRef, color: u32);

    /// Set the full track state chunk (RPP format).
    ///
    /// This replaces the entire track state with the given chunk string,
    /// which should be a valid REAPER track state chunk (the content of a
    /// `<TRACK ...>` block from an RPP file or `.RTrackTemplate`).
    async fn set_track_chunk(
        &self,
        project: ProjectContext,
        track: TrackRef,
        chunk: String,
    ) -> Result<(), String>;

    /// Get the full track state chunk (RPP format).
    ///
    /// Returns the complete track state as an RPP chunk string, suitable for
    /// round-tripping with `set_track_chunk` or parsing with `dawfile-reaper`.
    async fn get_track_chunk(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Result<String, String>;

    /// Set the folder depth change for a track.
    ///
    /// REAPER encodes folder hierarchy as depth deltas on each track:
    /// - `1` = folder start (this track is a folder)
    /// - `0` = normal track (no folder change)
    /// - `-1` = close one folder level
    /// - `-N` = close N folder levels
    async fn set_folder_depth(
        &self,
        project: ProjectContext,
        track: TrackRef,
        depth: i32,
    ) -> Result<(), String>;

    /// Set the number of audio channels for a track (e.g., 2 for stereo, 8 for multi-out plugins).
    async fn set_num_channels(
        &self,
        project: ProjectContext,
        track: TrackRef,
        num_channels: u32,
    ) -> Result<(), String>;

    /// Remove all tracks from the project (excluding master).
    async fn remove_all_tracks(&self, project: ProjectContext) -> Result<(), String>;

    /// Move a track to a new position in the track list.
    ///
    /// Reorders by capturing the track's state chunk, removing it, inserting
    /// a new track at the target index, and restoring the chunk.
    async fn move_track(
        &self,
        project: ProjectContext,
        track: TrackRef,
        new_index: u32,
    ) -> Result<(), String>;

    // =========================================================================
    // Track ExtState (P_EXT)
    // =========================================================================

    /// Get a track-scoped extended state value.
    ///
    /// Uses REAPER's `GetSetMediaTrackInfo_String` with `P_EXT:section:key`.
    /// Values are saved in the .RPP project file and copy with the track.
    async fn get_ext_state(
        &self,
        project: ProjectContext,
        track: TrackRef,
        request: TrackExtStateRequest,
    ) -> Option<String>;

    /// Set a track-scoped extended state value.
    ///
    /// Uses REAPER's `GetSetMediaTrackInfo_String` with `P_EXT:section:key`.
    /// Values are saved in the .RPP project file and copy with the track.
    async fn set_ext_state(
        &self,
        project: ProjectContext,
        track: TrackRef,
        request: TrackExtStateRequest,
    );

    /// Delete a track-scoped extended state value.
    ///
    /// Sets the value to empty string, which REAPER treats as deleted.
    async fn delete_ext_state(
        &self,
        project: ProjectContext,
        track: TrackRef,
        request: TrackExtStateRequest,
    );

    // =========================================================================
    // Subscriptions
    // =========================================================================

    /// Subscribe to track change events for a project.
    ///
    /// Streams granular `TrackEvent`s (added, removed, renamed, mute/solo
    /// changes, volume/pan changes, etc.) for as long as the client is
    /// connected.
    async fn subscribe_tracks(&self, project: ProjectContext, tx: Tx<TrackEvent>);
}
