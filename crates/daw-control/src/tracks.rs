//! Tracks handle and TrackHandle for individual tracks

use std::sync::Arc;

use crate::Result;
use crate::{DawClients, Envelopes, Error, FxChain, HardwareOutputs, Items, Receives, Sends};
use daw_proto::{
    FxChainContext, InputMonitoringMode, ProjectContext, RecordInput, Track, TrackEvent,
    TrackExtStateRequest, TrackRef,
};
use roam::Rx;

/// Tracks handle for a specific project
///
/// This handle provides access to track enumeration and batch operations.
/// Individual track operations are performed through [`TrackHandle`].
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::ErasedCaller) -> daw_control::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let tracks = project.tracks();
///
/// // Enumerate tracks
/// for track in tracks.all().await? {
///     println!("Track: {} ({})", track.name, track.guid);
/// }
///
/// // Get specific track
/// let vocals = tracks.by_name("Vocals").await?;
/// if let Some(track) = vocals {
///     track.solo_exclusive().await?;
/// }
///
/// // Batch operations
/// tracks.clear_solo().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Tracks {
    project_id: String,
    clients: Arc<DawClients>,
}

impl Tracks {
    /// Create a new tracks handle for a project
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all tracks in the project
    pub async fn all(&self) -> Result<Vec<Track>> {
        let tracks = self.clients.track.get_tracks(self.context()).await?;
        Ok(tracks)
    }

    /// Get track by index
    pub async fn by_index(&self, index: u32) -> Result<Option<TrackHandle>> {
        let track = self
            .clients
            .track
            .get_track(self.context(), TrackRef::Index(index))
            .await?;

        Ok(track.map(|t| TrackHandle::new(t.guid, self.project_id.clone(), self.clients.clone())))
    }

    /// Get track by GUID
    pub async fn by_guid(&self, guid: &str) -> Result<Option<TrackHandle>> {
        let track = self
            .clients
            .track
            .get_track(self.context(), TrackRef::Guid(guid.to_string()))
            .await?;

        Ok(track.map(|t| TrackHandle::new(t.guid, self.project_id.clone(), self.clients.clone())))
    }

    /// Get track by name (first match)
    pub async fn by_name(&self, name: &str) -> Result<Option<TrackHandle>> {
        // Get all tracks and find first match
        let tracks = self.clients.track.get_tracks(self.context()).await?;
        let track = tracks.into_iter().find(|t| t.name == name);

        Ok(track.map(|t| TrackHandle::new(t.guid, self.project_id.clone(), self.clients.clone())))
    }

    /// Get the master track
    pub async fn master(&self) -> Result<TrackHandle> {
        let track = self
            .clients
            .track
            .get_master_track(self.context())
            .await?
            .ok_or_else(|| Error::Other("No master track found".to_string()))?;

        Ok(TrackHandle::new(
            track.guid,
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }

    /// Get all currently selected tracks
    pub async fn selected(&self) -> Result<Vec<TrackHandle>> {
        let tracks = self
            .clients
            .track
            .get_selected_tracks(self.context())
            .await?;

        Ok(tracks
            .into_iter()
            .map(|t| TrackHandle::new(t.guid, self.project_id.clone(), self.clients.clone()))
            .collect())
    }

    /// Get total track count (excluding master)
    pub async fn count(&self) -> Result<u32> {
        let count = self.clients.track.track_count(self.context()).await?;
        Ok(count)
    }

    // =========================================================================
    // Batch Operations
    // =========================================================================

    /// Clear solo from all tracks
    pub async fn clear_solo(&self) -> Result<()> {
        self.clients.track.clear_all_solo(self.context()).await?;
        Ok(())
    }

    /// Mute all tracks
    pub async fn mute_all(&self) -> Result<()> {
        self.clients.track.mute_all(self.context()).await?;
        Ok(())
    }

    /// Unmute all tracks
    pub async fn unmute_all(&self) -> Result<()> {
        self.clients.track.unmute_all(self.context()).await?;
        Ok(())
    }

    /// Clear selection from all tracks
    pub async fn clear_selection(&self) -> Result<()> {
        self.clients.track.clear_selection(self.context()).await?;
        Ok(())
    }

    // =========================================================================
    // Bulk Operations
    // =========================================================================

    /// Apply a track hierarchy atomically in a single operation.
    ///
    /// Matches hierarchy nodes to existing tracks by name, preserving their
    /// items, FX, routing, and automation. Creates new tracks only for
    /// unmatched nodes. Reorders everything to match the hierarchy and sets
    /// folder depths and colors — all in one main-thread tick.
    pub async fn apply_hierarchy(&self, hierarchy: daw_proto::TrackHierarchy) -> Result<()> {
        self.clients
            .track
            .apply_hierarchy(self.context(), hierarchy)
            .await
            .map_err(|e| Error::Other(format!("{:?}", e)))
    }

    // =========================================================================
    // Track Creation / Deletion
    // =========================================================================

    /// Add a new track to the project.
    ///
    /// If `at_index` is `Some(i)`, inserts at that position (0-based), shifting
    /// existing tracks down. If `None`, appends at the end.
    /// Returns a [`TrackHandle`] for the newly created track.
    pub async fn add(&self, name: &str, at_index: Option<u32>) -> Result<TrackHandle> {
        let guid = self
            .clients
            .track
            .add_track(self.context(), name.to_string(), at_index)
            .await?;
        if guid.is_empty() {
            return Err(Error::InvalidOperation(
                "add_track returned empty GUID — REAPER may have refused the operation".to_string(),
            ));
        }
        Ok(TrackHandle::new(
            guid,
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }

    /// Remove a track from the project by GUID, index, or master reference.
    pub async fn remove(&self, track: daw_proto::TrackRef) -> Result<()> {
        self.clients
            .track
            .remove_track(self.context(), track)
            .await?;
        Ok(())
    }

    /// Remove all tracks from the project (excluding master).
    pub async fn remove_all(&self) -> Result<()> {
        self.clients.track.remove_all_tracks(self.context()).await?;
        Ok(())
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to track events (added, removed, renamed, mute/solo changes, etc.)
    ///
    /// Returns a receiver that streams granular track events for this project.
    /// The stream continues until the returned `Rx` is dropped.
    pub async fn subscribe(&self) -> Result<Rx<TrackEvent>> {
        let (tx, rx) = roam::channel::<TrackEvent>();
        self.clients
            .track
            .subscribe_tracks(self.context(), tx)
            .await?;
        Ok(rx)
    }
}

impl std::fmt::Debug for Tracks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tracks")
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// TrackHandle
// =============================================================================

/// Handle to a single track - all track operations
///
/// This handle represents a specific track in a DAW project. It's lightweight
/// (stores only GUIDs) and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::ErasedCaller) -> daw_control::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
///
/// // Get a track and work with it
/// let track = project.tracks().by_name("Vocals").await?.unwrap();
///
/// // Solo/mute
/// track.solo_exclusive().await?;
/// track.mute().await?;
///
/// // Volume/pan
/// track.set_volume(0.8).await?;
/// track.set_pan(-0.3).await?;
///
/// // Access FX chain
/// let fx = track.fx_chain().by_name("ReaComp").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TrackHandle {
    track_guid: String,
    project_id: String,
    clients: Arc<DawClients>,
}

impl TrackHandle {
    /// Create a new track handle
    pub(crate) fn new(track_guid: String, project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            track_guid,
            project_id,
            clients,
        }
    }

    /// Get the track GUID
    pub fn guid(&self) -> &str {
        &self.track_guid
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create track reference
    fn track_ref(&self) -> TrackRef {
        TrackRef::Guid(self.track_guid.clone())
    }

    // =========================================================================
    // Info
    // =========================================================================

    /// Get full track state
    pub async fn info(&self) -> Result<Track> {
        self.clients
            .track
            .get_track(self.context(), self.track_ref())
            .await?
            .ok_or_else(|| Error::Other(format!("Track not found: {}", self.track_guid)))
    }

    // =========================================================================
    // Mute
    // =========================================================================

    /// Mute the track
    pub async fn mute(&self) -> Result<()> {
        self.clients
            .track
            .set_muted(self.context(), self.track_ref(), true)
            .await?;
        Ok(())
    }

    /// Unmute the track
    pub async fn unmute(&self) -> Result<()> {
        self.clients
            .track
            .set_muted(self.context(), self.track_ref(), false)
            .await?;
        Ok(())
    }

    /// Toggle mute state
    pub async fn toggle_mute(&self) -> Result<()> {
        let info = self.info().await?;
        self.clients
            .track
            .set_muted(self.context(), self.track_ref(), !info.muted)
            .await?;
        Ok(())
    }

    /// Check if track is muted
    pub async fn is_muted(&self) -> Result<bool> {
        Ok(self.info().await?.muted)
    }

    // =========================================================================
    // Solo
    // =========================================================================

    /// Solo the track
    pub async fn solo(&self) -> Result<()> {
        self.clients
            .track
            .set_soloed(self.context(), self.track_ref(), true)
            .await?;
        Ok(())
    }

    /// Unsolo the track
    pub async fn unsolo(&self) -> Result<()> {
        self.clients
            .track
            .set_soloed(self.context(), self.track_ref(), false)
            .await?;
        Ok(())
    }

    /// Toggle solo state
    pub async fn toggle_solo(&self) -> Result<()> {
        let info = self.info().await?;
        self.clients
            .track
            .set_soloed(self.context(), self.track_ref(), !info.soloed)
            .await?;
        Ok(())
    }

    /// Solo this track exclusively (unsolo all others)
    pub async fn solo_exclusive(&self) -> Result<()> {
        self.clients
            .track
            .set_solo_exclusive(self.context(), self.track_ref())
            .await?;
        Ok(())
    }

    /// Check if track is soloed
    pub async fn is_soloed(&self) -> Result<bool> {
        Ok(self.info().await?.soloed)
    }

    // =========================================================================
    // Arm
    // =========================================================================

    /// Arm the track for recording
    pub async fn arm(&self) -> Result<()> {
        self.clients
            .track
            .set_armed(self.context(), self.track_ref(), true)
            .await?;
        Ok(())
    }

    /// Disarm the track
    pub async fn disarm(&self) -> Result<()> {
        self.clients
            .track
            .set_armed(self.context(), self.track_ref(), false)
            .await?;
        Ok(())
    }

    /// Toggle arm state
    pub async fn toggle_arm(&self) -> Result<()> {
        let info = self.info().await?;
        self.clients
            .track
            .set_armed(self.context(), self.track_ref(), !info.armed)
            .await?;
        Ok(())
    }

    /// Check if track is armed
    pub async fn is_armed(&self) -> Result<bool> {
        Ok(self.info().await?.armed)
    }

    /// Set the input monitoring mode.
    pub async fn set_input_monitoring(&self, mode: InputMonitoringMode) -> Result<()> {
        self.clients
            .track
            .set_input_monitoring(self.context(), self.track_ref(), mode)
            .await?;
        Ok(())
    }

    /// Set the record input source.
    pub async fn set_record_input(&self, input: RecordInput) -> Result<()> {
        self.clients
            .track
            .set_record_input(self.context(), self.track_ref(), input)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Volume/Pan
    // =========================================================================

    /// Get track volume (0.0 = -inf dB, 1.0 = 0 dB)
    pub async fn volume(&self) -> Result<f64> {
        Ok(self.info().await?.volume)
    }

    /// Set track volume (0.0 = -inf dB, 1.0 = 0 dB)
    pub async fn set_volume(&self, volume: f64) -> Result<()> {
        self.clients
            .track
            .set_volume(self.context(), self.track_ref(), volume)
            .await?;
        Ok(())
    }

    /// Get track pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub async fn pan(&self) -> Result<f64> {
        Ok(self.info().await?.pan)
    }

    /// Set track pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub async fn set_pan(&self, pan: f64) -> Result<()> {
        self.clients
            .track
            .set_pan(self.context(), self.track_ref(), pan)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Selection
    // =========================================================================

    /// Select the track
    pub async fn select(&self) -> Result<()> {
        self.clients
            .track
            .set_selected(self.context(), self.track_ref(), true)
            .await?;
        Ok(())
    }

    /// Deselect the track
    pub async fn deselect(&self) -> Result<()> {
        self.clients
            .track
            .set_selected(self.context(), self.track_ref(), false)
            .await?;
        Ok(())
    }

    /// Select this track exclusively (deselect all others)
    pub async fn select_exclusive(&self) -> Result<()> {
        self.clients
            .track
            .select_exclusive(self.context(), self.track_ref())
            .await?;
        Ok(())
    }

    // =========================================================================
    // Track Management
    // =========================================================================

    /// Rename the track
    pub async fn rename(&self, name: &str) -> Result<()> {
        self.clients
            .track
            .rename_track(self.context(), self.track_ref(), name.to_string())
            .await?;
        Ok(())
    }

    /// Move this track to a new position in the track list
    pub async fn move_to_index(&self, new_index: u32) -> Result<()> {
        self.clients
            .track
            .move_track(self.context(), self.track_ref(), new_index)
            .await?;
        Ok(())
    }

    /// Set track color (0xRRGGBB format, or 0 for default)
    pub async fn set_color(&self, color: u32) -> Result<()> {
        self.clients
            .track
            .set_track_color(self.context(), self.track_ref(), color)
            .await?;
        Ok(())
    }

    /// Get the full track state chunk (RPP format).
    ///
    /// Returns the complete track state as an RPP chunk string.
    pub async fn get_chunk(&self) -> Result<String> {
        self.clients
            .track
            .get_track_chunk(self.context(), self.track_ref())
            .await
            .map_err(|e| Error::Other(format!("{:?}", e)))
    }

    /// Set the full track state chunk (RPP format).
    ///
    /// This replaces the entire track state — useful for loading
    /// `.RTrackTemplate` content into an existing track.
    pub async fn set_chunk(&self, chunk: String) -> Result<()> {
        self.clients
            .track
            .set_track_chunk(self.context(), self.track_ref(), chunk)
            .await?;
        Ok(())
    }

    /// Set the number of audio channels for this track.
    ///
    /// Defaults to 2 (stereo). Set to 8 for multi-output plugins like FTS Guide.
    pub async fn set_num_channels(&self, num_channels: u32) -> Result<()> {
        self.clients
            .track
            .set_num_channels(self.context(), self.track_ref(), num_channels)
            .await
            .map_err(|e| Error::Other(format!("{:?}", e)))
    }

    /// Set the folder depth change for this track.
    ///
    /// Controls folder hierarchy: `1` = folder start, `0` = normal,
    /// `-1` = close one level, `-N` = close N levels.
    pub async fn set_folder_depth(&self, depth: i32) -> Result<()> {
        self.clients
            .track
            .set_folder_depth(self.context(), self.track_ref(), depth)
            .await
            .map_err(|e| Error::Other(format!("{:?}", e)))
    }

    // =========================================================================
    // Visibility
    // =========================================================================

    /// Show the track in the TCP (arrange view)
    pub async fn show_in_tcp(&self) -> Result<()> {
        self.clients
            .track
            .set_visible_in_tcp(self.context(), self.track_ref(), true)
            .await?;
        Ok(())
    }

    /// Hide the track from the TCP (arrange view)
    pub async fn hide_in_tcp(&self) -> Result<()> {
        self.clients
            .track
            .set_visible_in_tcp(self.context(), self.track_ref(), false)
            .await?;
        Ok(())
    }

    /// Set TCP visibility
    pub async fn set_visible_in_tcp(&self, visible: bool) -> Result<()> {
        self.clients
            .track
            .set_visible_in_tcp(self.context(), self.track_ref(), visible)
            .await?;
        Ok(())
    }

    /// Check if track is visible in TCP
    pub async fn is_visible_in_tcp(&self) -> Result<bool> {
        Ok(self.info().await?.visible_in_tcp)
    }

    /// Show the track in the mixer
    pub async fn show_in_mixer(&self) -> Result<()> {
        self.clients
            .track
            .set_visible_in_mixer(self.context(), self.track_ref(), true)
            .await?;
        Ok(())
    }

    /// Hide the track from the mixer
    pub async fn hide_in_mixer(&self) -> Result<()> {
        self.clients
            .track
            .set_visible_in_mixer(self.context(), self.track_ref(), false)
            .await?;
        Ok(())
    }

    /// Set mixer visibility
    pub async fn set_visible_in_mixer(&self, visible: bool) -> Result<()> {
        self.clients
            .track
            .set_visible_in_mixer(self.context(), self.track_ref(), visible)
            .await?;
        Ok(())
    }

    /// Check if track is visible in mixer
    pub async fn is_visible_in_mixer(&self) -> Result<bool> {
        Ok(self.info().await?.visible_in_mixer)
    }

    // =========================================================================
    // FX Chain Access
    // =========================================================================

    /// Get the track's FX chain (output/playback)
    pub fn fx_chain(&self) -> FxChain {
        FxChain::new(
            FxChainContext::Track(self.track_guid.clone()),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get the track's input FX chain (recording)
    pub fn input_fx_chain(&self) -> FxChain {
        FxChain::new(
            FxChainContext::Input(self.track_guid.clone()),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    // =========================================================================
    // Items Access
    // =========================================================================

    /// Get access to items on this track
    pub fn items(&self) -> Items {
        Items::new(
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    // =========================================================================
    // Parent Send (folder routing)
    // =========================================================================

    /// Enable or disable the parent send (folder bus routing).
    ///
    /// When disabled, audio from this track does not flow to the parent
    /// folder track — it only flows through explicit sends.
    pub async fn set_parent_send(&self, enabled: bool) -> Result<()> {
        self.clients
            .routing
            .set_parent_send_enabled(self.context(), self.track_ref(), enabled)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Routing Access
    // =========================================================================

    /// Get access to sends from this track
    pub fn sends(&self) -> Sends {
        Sends::new(
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get access to receives to this track
    pub fn receives(&self) -> Receives {
        Receives::new(
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get access to hardware outputs from this track
    pub fn hardware_outputs(&self) -> HardwareOutputs {
        HardwareOutputs::new(
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    // =========================================================================
    // Automation Access
    // =========================================================================

    /// Get access to automation envelopes on this track
    pub fn envelopes(&self) -> Envelopes {
        Envelopes::new(
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get the volume envelope
    pub fn volume_envelope(&self) -> crate::EnvelopeHandle {
        self.envelopes().volume()
    }

    /// Get the pan envelope
    pub fn pan_envelope(&self) -> crate::EnvelopeHandle {
        self.envelopes().pan()
    }

    // =========================================================================
    // Track ExtState (P_EXT)
    // =========================================================================

    /// Get a track-scoped extended state value.
    ///
    /// Uses REAPER's `P_EXT:section:key` mechanism. Values are saved in the
    /// .RPP project file and copy with the track when duplicated.
    pub async fn get_ext_state(&self, section: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .clients
            .track
            .get_ext_state(
                self.context(),
                self.track_ref(),
                TrackExtStateRequest {
                    section: section.to_string(),
                    key: key.to_string(),
                    value: String::new(),
                },
            )
            .await?)
    }

    /// Set a track-scoped extended state value.
    ///
    /// Uses REAPER's `P_EXT:section:key` mechanism. Values are saved in the
    /// .RPP project file and copy with the track when duplicated.
    pub async fn set_ext_state(&self, section: &str, key: &str, value: &str) -> Result<()> {
        self.clients
            .track
            .set_ext_state(
                self.context(),
                self.track_ref(),
                TrackExtStateRequest {
                    section: section.to_string(),
                    key: key.to_string(),
                    value: value.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    /// Delete a track-scoped extended state value.
    pub async fn delete_ext_state(&self, section: &str, key: &str) -> Result<()> {
        self.clients
            .track
            .delete_ext_state(
                self.context(),
                self.track_ref(),
                TrackExtStateRequest {
                    section: section.to_string(),
                    key: key.to_string(),
                    value: String::new(),
                },
            )
            .await?;
        Ok(())
    }
}

impl std::fmt::Debug for TrackHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackHandle")
            .field("track_guid", &self.track_guid)
            .field("project_id", &self.project_id)
            .finish()
    }
}
