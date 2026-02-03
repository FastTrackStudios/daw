//! Standalone track implementation

use daw_proto::{ProjectContext, Track, TrackRef, TrackService};
use roam::Context;
use std::sync::Arc;
use tokio::sync::RwLock;
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
        }
    }

    fn to_track(&self) -> Track {
        Track::new(self.guid.clone(), self.index, self.name.clone())
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
            tracks: Arc::new(RwLock::new(default_tracks)),
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
    async fn get_tracks(&self, _cx: &Context, _project: ProjectContext) -> Vec<Track> {
        let tracks = self.tracks.read().await;
        tracks.iter().map(|t| t.to_track()).collect()
    }

    async fn get_track(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
    ) -> Option<Track> {
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

    async fn track_count(&self, _cx: &Context, _project: ProjectContext) -> u32 {
        self.tracks.read().await.len() as u32
    }

    async fn get_selected_tracks(&self, _cx: &Context, _project: ProjectContext) -> Vec<Track> {
        let tracks = self.tracks.read().await;
        tracks
            .iter()
            .filter(|t| t.selected)
            .map(|t| t.to_track())
            .collect()
    }

    async fn get_master_track(&self, _cx: &Context, _project: ProjectContext) -> Option<Track> {
        let tracks = self.tracks.read().await;
        tracks.first().map(|t| t.to_track())
    }

    async fn set_muted(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        muted: bool,
    ) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.muted = muted;
        }
    }

    async fn set_soloed(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        soloed: bool,
    ) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.soloed = soloed;
        }
    }

    async fn set_solo_exclusive(&self, _cx: &Context, _project: ProjectContext, track: TrackRef) {
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

    async fn clear_all_solo(&self, _cx: &Context, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.soloed = false;
        }
    }

    async fn set_armed(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        armed: bool,
    ) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.armed = armed;
        }
    }

    async fn set_volume(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        volume: f64,
    ) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.volume = volume.clamp(0.0, 4.0);
        }
    }

    async fn set_pan(&self, _cx: &Context, _project: ProjectContext, track: TrackRef, pan: f64) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.pan = pan.clamp(-1.0, 1.0);
        }
    }

    async fn set_selected(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        selected: bool,
    ) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.selected = selected;
        }
    }

    async fn select_exclusive(&self, _cx: &Context, _project: ProjectContext, track: TrackRef) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.selected = false;
        }
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.selected = true;
        }
    }

    async fn clear_selection(&self, _cx: &Context, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.selected = false;
        }
    }

    async fn mute_all(&self, _cx: &Context, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.muted = true;
        }
    }

    async fn unmute_all(&self, _cx: &Context, _project: ProjectContext) {
        let mut tracks = self.tracks.write().await;
        for t in tracks.iter_mut() {
            t.muted = false;
        }
    }

    async fn rename_track(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        name: String,
    ) {
        let mut tracks = self.tracks.write().await;
        if let Some(t) = Self::find_track(&mut tracks, &track) {
            t.name = name;
        }
    }

    async fn set_track_color(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _track: TrackRef,
        _color: u32,
    ) {
        // Color tracking not implemented in standalone
    }
}
