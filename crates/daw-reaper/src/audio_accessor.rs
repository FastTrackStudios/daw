//! REAPER Audio Accessor Service Implementation
//!
//! Implements AudioAccessorService for REAPER, providing handle-based
//! random-access sample reading from tracks and takes.

use daw_proto::{
    AudioAccessorService, AudioSampleData, GetSamplesRequest, ItemRef, ProjectContext, TakeRef,
    TrackRef,
};
use reaper_high::Reaper;
use roam::Context;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::warn;

use crate::main_thread;
use crate::project_context::find_project_by_guid;
use crate::safe_wrappers::audio_accessor as aa_sw;
use crate::track::resolve_track_pub;

use aa_sw::SendableAccessorPtr;

/// REAPER audio accessor service implementation.
///
/// Maintains a map of opaque string IDs → raw AudioAccessor pointers,
/// protected by a mutex so that handle creation/destruction is thread-safe.
pub struct ReaperAudioAccessor {
    /// Map from opaque handle IDs to raw accessor pointers.
    accessors: Mutex<HashMap<String, SendableAccessorPtr>>,
    /// Counter for generating unique IDs.
    next_id: Mutex<u64>,
}

impl ReaperAudioAccessor {
    pub fn new() -> Self {
        Self {
            accessors: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }

    /// Generate a unique accessor ID.
    fn next_id(&self) -> String {
        let mut counter = self.next_id.lock().unwrap();
        let id = *counter;
        *counter += 1;
        format!("aa-{}", id)
    }

    /// Store a pointer and return its ID. Returns None if pointer is null.
    fn store(&self, ptr: SendableAccessorPtr) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let id = self.next_id();
        self.accessors.lock().unwrap().insert(id.clone(), ptr);
        Some(id)
    }

    /// Look up a pointer by ID.
    fn get_ptr(&self, id: &str) -> Option<SendableAccessorPtr> {
        self.accessors.lock().unwrap().get(id).copied()
    }

    /// Remove and return a pointer by ID.
    fn remove_ptr(&self, id: &str) -> Option<SendableAccessorPtr> {
        self.accessors.lock().unwrap().remove(id)
    }
}

impl Default for ReaperAudioAccessor {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ReaperAudioAccessor {
    fn clone(&self) -> Self {
        Self::new()
    }
}

/// Resolve a ProjectContext to a REAPER Project
fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

impl AudioAccessorService for ReaperAudioAccessor {
    async fn create_track_accessor(
        &self,
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
    ) -> Option<String> {
        let ptr = main_thread::query(move || {
            let proj = resolve_project(&project)?;
            let t = resolve_track_pub(&proj, &track)?;
            let raw = t.raw().ok()?;
            let low = Reaper::get().medium_reaper().low();
            let accessor = aa_sw::create_track_audio_accessor(low, raw);
            Some(SendableAccessorPtr::new(accessor))
        })
        .await
        .flatten()?;

        self.store(ptr)
    }

    async fn create_take_accessor(
        &self,
        _cx: &Context,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> Option<String> {
        let ptr = main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let reaper_project_ctx = match &project {
                ProjectContext::Current => reaper_medium::ProjectContext::CurrentProject,
                ProjectContext::Project(guid) => {
                    let proj = find_project_by_guid(guid)?;
                    reaper_medium::ProjectContext::Proj(proj.raw())
                }
            };

            let midi_item =
                crate::midi::ReaperMidi::resolve_item(medium, reaper_project_ctx, &item)?;
            let midi_take = crate::midi::ReaperMidi::resolve_take(medium, midi_item, &take)?;

            let low = medium.low();
            let accessor = aa_sw::create_take_audio_accessor(low, midi_take);
            Some(SendableAccessorPtr::new(accessor))
        })
        .await
        .flatten()?;

        self.store(ptr)
    }

    async fn has_state_changed(&self, _cx: &Context, accessor_id: String) -> bool {
        let Some(ptr) = self.get_ptr(&accessor_id) else {
            warn!("has_state_changed: unknown accessor ID '{}'", accessor_id);
            return false;
        };

        main_thread::query(move || {
            let low = Reaper::get().medium_reaper().low();
            aa_sw::audio_accessor_state_changed(low, ptr.get())
        })
        .await
        .unwrap_or(false)
    }

    async fn get_samples(
        &self,
        _cx: &Context,
        request: GetSamplesRequest,
    ) -> AudioSampleData {
        let Some(ptr) = self.get_ptr(&request.accessor_id) else {
            warn!("get_samples: unknown accessor ID '{}'", request.accessor_id);
            return AudioSampleData::default();
        };

        let sample_rate = request.sample_rate;
        let num_channels = request.num_channels;
        let start_time = request.start_time;
        let num_samples = request.num_samples;

        main_thread::query(move || {
            let low = Reaper::get().medium_reaper().low();

            let buf_size = (num_channels * num_samples) as usize;
            let mut buf = vec![0.0f64; buf_size];

            let result = aa_sw::get_audio_accessor_samples(
                low,
                ptr.get(),
                sample_rate as i32,
                num_channels as i32,
                start_time,
                num_samples as i32,
                &mut buf,
            );

            if result <= 0 {
                return AudioSampleData::default();
            }

            let actual_samples = result as u32;
            let actual_size = (num_channels * actual_samples) as usize;
            buf.truncate(actual_size);

            AudioSampleData {
                samples: buf,
                sample_rate,
                num_channels,
                num_samples: actual_samples,
            }
        })
        .await
        .unwrap_or_default()
    }

    async fn destroy_accessor(&self, _cx: &Context, accessor_id: String) {
        let Some(ptr) = self.remove_ptr(&accessor_id) else {
            warn!("destroy_accessor: unknown accessor ID '{}'", accessor_id);
            return;
        };

        main_thread::query(move || {
            let low = Reaper::get().medium_reaper().low();
            aa_sw::destroy_audio_accessor(low, ptr.get());
            Some(())
        })
        .await;
    }
}
