//! Standalone track implementation

use crate::platform::RwLock;
use daw_proto::{
    InputMonitoringMode, ProjectContext, RecordInput, Track, TrackEvent, TrackExtStateRequest,
    TrackRef, TrackService,
};
use roam::Tx;
use std::sync::Arc;
use uuid::Uuid;

/// Internal track state for standalone implementation
#[derive(Clone)]
struct TrackState {
    guid: String,
    index: u32,
    name: String,
    muted: bool,
    soloed: bool,
    armed: bool,
    selected: bool,
    volume: f64,
    pan: f64,
    visible_in_tcp: bool,
    visible_in_mixer: bool,
    color: Option<u32>,
}

impl TrackState {
    fn new(guid: String, index: u32, name: String) -> Self {
        Self {
            guid,
            index,
            name,
            muted: false,
            soloed: false,
            armed: false,
            selected: false,
            volume: 1.0,
            pan: 0.0,
            visible_in_tcp: true,
            visible_in_mixer: true,
            color: None,
        }
    }

    fn to_track(&self) -> Track {
        let mut track = Track::new(self.guid.clone(), self.index, self.name.clone());
        track.muted = self.muted;
        track.soloed = self.soloed;
        track.armed = self.armed;
        track.selected = self.selected;
        track.volume = self.volume;
        track.pan = self.pan;
        track.visible_in_tcp = self.visible_in_tcp;
        track.visible_in_mixer = self.visible_in_mixer;
        track.color = self.color;
        track
    }
}

/// Standalone track service implementation.
///
/// Maintains an in-memory list of tracks for testing.
#[derive(Clone)]
pub struct StandaloneTrack {
    tracks: Arc<RwLock<Vec<TrackState>>>,
}

impl Default for StandaloneTrack {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneTrack {
    pub fn new() -> Self {
        // Create some default tracks
        let default_tracks = vec![
            TrackState::new(Uuid::new_v4().to_string(), 0, "Track 1".to_string()),
            TrackState::new(Uuid::new_v4().to_string(), 1, "Track 2".to_string()),
            TrackState::new(Uuid::new_v4().to_string(), 2, "Vocals".to_string()),
            TrackState::new(Uuid::new_v4().to_string(), 3, "Drums".to_string()),
        ];

        Self {
            tracks: Arc::new(RwLock::new("standalone-tracks", default_tracks)),
        }
    }

    /// Add a track (useful for tests)
    pub async fn add_track(&self, name: &str) -> String {
        let mut tracks = self.tracks.write().await;
        let index = tracks.len() as u32;
        let guid = Uuid::new_v4().to_string();
        tracks.push(TrackState::new(guid.clone(), index, name.to_string()));
        guid
    }

    fn find_track<'a>(
        tracks: &'a mut [TrackState],
        track_ref: &TrackRef,
    ) -> Option<&'a mut TrackState> {
        match track_ref {
            TrackRef::Guid(guid) => tracks.iter_mut().find(|t| &t.guid == guid),
            TrackRef::Index(idx) => tracks.iter_mut().find(|t| t.index == *idx),
            TrackRef::Master => tracks.first_mut(),
        }
    }
}

impl TrackService for StandaloneTrack {
    async fn get_tracks(&self, _project: ProjectContext) -> Vec<Track> {
        let tracks = self.tracks.read().await;
        tracks.iter().map(|t| t.to_track()).collect()
    }

    async fn get_track(&self, _project: ProjectContext, track: TrackRef) -> Option<Track> {
        let tracks = self.tracks.read().await;
        match &track {
            TrackRef::Guid(guid) => tracks
                .iter()
                .find(|t| &t.guid == guid)
                .map(|t| t.to_track()),
            TrackRef::Index(idx) => tracks
                .iter()
                .find(|t| t.index == *idx)
                .map(|t| t.to_track()),
            TrackRef::Master => tracks.first().map(|t| t.to_track()),
        }
    }

    async fn track_count(&self, _project: ProjectContext) -> u32 {
        self.tracks.read().await.len() as u32
    }

    async fn get_selected_tracks(&self, _project: ProjectContext) -> Vec<Track> {
        let tracks = self.tracks.read().await;
        tracks
            .iter()
            .filter(|t| t.selected)
            .map(|t| t.to_track())
            .collect()
    }

    async fn get_master_track(&self, _project: ProjectContext) -> Option<Track> {
        let tracks = self.tracks.read().await;
        tracks.first().map(|t| t.to_track())
    }

    async fn set_muted(&self, _project: ProjectContext, track: TrackRef, muted: bool) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.muted = muted;
        }
    }

    async fn set_soloed(&self, _project: ProjectContext, track: TrackRef, soloed: bool) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.soloed = soloed;
        }
    }

    async fn set_solo_exclusive(&self, _project: ProjectContext, track: TrackRef) {
        let mut tracks = self.tracks.write().await;
        // Unsolo all first
        for t in tracks.iter_mut() {
            t.soloed = false;
        }
        // Solo the target track
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.soloed = true;
        }
    }

    async fn clear_all_solo(&self, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.soloed = false;
        }
    }

    async fn set_armed(&self, _project: ProjectContext, track: TrackRef, armed: bool) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.armed = armed;
        }
    }

    async fn set_input_monitoring(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _mode: InputMonitoringMode,
    ) {
        // No-op in standalone
    }

    async fn set_record_input(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _input: RecordInput,
    ) {
        // No-op in standalone
    }

    async fn set_volume(&self, _project: ProjectContext, track: TrackRef, volume: f64) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.volume = volume.clamp(0.0, 4.0);
        }
    }

    async fn set_pan(&self, _project: ProjectContext, track: TrackRef, pan: f64) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.pan = pan.clamp(-1.0, 1.0);
        }
    }

    async fn set_selected(&self, _project: ProjectContext, track: TrackRef, selected: bool) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.selected = selected;
        }
    }

    async fn select_exclusive(&self, _project: ProjectContext, track: TrackRef) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.selected = false;
        }
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.selected = true;
        }
    }

    async fn clear_selection(&self, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.selected = false;
        }
    }

    async fn mute_all(&self, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.muted = true;
        }
    }

    async fn unmute_all(&self, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.muted = false;
        }
    }

    async fn add_track(
        &self,
        _project: ProjectContext,
        name: String,
        at_index: Option<u32>,
    ) -> String {
        let mut tracks = self.tracks.write().await;
        let index = at_index.unwrap_or(tracks.len() as u32) as usize;
        let guid = Uuid::new_v4().to_string();
        tracks.insert(index, TrackState::new(guid.clone(), index as u32, name));
        // Re-index all tracks after the insertion point
        for (i, t) in tracks.iter_mut().enumerate() {
            t.index = i as u32;
        }
        guid
    }

    async fn remove_track(&self, _project: ProjectContext, track: TrackRef) {
        let mut tracks = self.tracks.write().await;
        let pos = match &track {
            TrackRef::Guid(guid) => tracks.iter().position(|t| &t.guid == guid),
            TrackRef::Index(idx) => tracks.iter().position(|t| t.index == *idx),
            TrackRef::Master => Some(0),
        };
        if let Some(i) = pos {
            tracks.remove(i);
            // Re-index remaining tracks
            for (j, t) in tracks.iter_mut().enumerate() {
                t.index = j as u32;
            }
        }
    }

    async fn rename_track(&self, _project: ProjectContext, track: TrackRef, name: String) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.name = name;
        }
    }

    async fn set_track_color(&self, _project: ProjectContext, track: TrackRef, color: u32) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.color = if color == 0 { None } else { Some(color) };
        }
    }

    async fn set_visible_in_tcp(&self, _project: ProjectContext, track: TrackRef, visible: bool) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.visible_in_tcp = visible;
        }
    }

    async fn set_visible_in_mixer(&self, _project: ProjectContext, track: TrackRef, visible: bool) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.visible_in_mixer = visible;
        }
    }

    async fn set_track_chunk(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _chunk: String,
    ) -> Result<(), String> {
        // Standalone implementation doesn't support chunk operations
        Ok(())
    }

    async fn get_track_chunk(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
    ) -> Result<String, String> {
        Ok(String::new())
    }

    async fn set_folder_depth(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _depth: i32,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn set_num_channels(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _num_channels: u32,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn remove_all_tracks(&self, _project: ProjectContext) -> Result<(), String> {
        let mut tracks = self.tracks.write().await;
        tracks.clear();
        Ok(())
    }

    async fn get_ext_state(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _request: TrackExtStateRequest,
    ) -> Option<String> {
        None
    }

    async fn set_ext_state(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _request: TrackExtStateRequest,
    ) {
    }

    async fn delete_ext_state(
        &self,
        _project: ProjectContext,
        _track: TrackRef,
        _request: TrackExtStateRequest,
    ) {
    }

    async fn subscribe_tracks(&self, _project: ProjectContext, _tx: Tx<TrackEvent>) {}
}
