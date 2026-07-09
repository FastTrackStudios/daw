//! `impl AudioAccessors for Reaper` — sync trait + REAPER's audio
//! accessor C API. Handle-based: opaque string IDs map to
//! `AudioAccessor*` pointers stored in a module-level Mutex.

use daw_proto::{
    AudioAccessors, AudioSampleData, GetSamplesRequest, ItemRef, ProjectContext, TakeRef, TrackRef,
};
use reaper_high::Reaper;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tracing::warn;

use crate::project_context::find_project_by_guid;
use crate::safe_wrappers::audio_accessor as aa_sw;
use crate::track::resolve_track_pub;

use aa_sw::SendableAccessorPtr;
use daw_control::lock::LockExt;

struct AccessorRegistry {
    accessors: Mutex<HashMap<String, SendableAccessorPtr>>,
    next_id: Mutex<u64>,
}

static REGISTRY: OnceLock<AccessorRegistry> = OnceLock::new();

fn registry() -> &'static AccessorRegistry {
    REGISTRY.get_or_init(|| AccessorRegistry {
        accessors: Mutex::new(HashMap::new()),
        next_id: Mutex::new(1),
    })
}

fn next_id() -> String {
    let mut counter = registry().next_id.lock_recoverable("audio_accessor");
    let id = *counter;
    *counter += 1;
    format!("aa-{id}")
}

pub fn store(ptr: SendableAccessorPtr) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let id = next_id();
    registry()
        .accessors
        .lock_recoverable("audio_accessor")
        .insert(id.clone(), ptr);
    Some(id)
}

pub fn get_ptr(id: &str) -> Option<SendableAccessorPtr> {
    registry()
        .accessors
        .lock_recoverable("audio_accessor")
        .get(id)
        .copied()
}

pub fn remove_ptr(id: &str) -> Option<SendableAccessorPtr> {
    registry()
        .accessors
        .lock_recoverable("audio_accessor")
        .remove(id)
}

fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

impl AudioAccessors for crate::Reaper {
    fn create_track_accessor(&self, project: ProjectContext, track: TrackRef) -> Option<String> {
        let proj = resolve_project(&project)?;
        let t = resolve_track_pub(&proj, &track)?;
        let raw = t.raw().ok()?;
        let low = Reaper::get().medium_reaper().low();
        let accessor = aa_sw::create_track_audio_accessor(low, raw);
        store(SendableAccessorPtr::new(accessor))
    }

    fn create_take_accessor(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> Option<String> {
        let reaper = Reaper::get();
        let medium = reaper.medium_reaper();
        let reaper_project_ctx = match &project {
            ProjectContext::Current => reaper_medium::ProjectContext::CurrentProject,
            ProjectContext::Project(guid) => {
                let proj = find_project_by_guid(guid)?;
                reaper_medium::ProjectContext::Proj(proj.raw())
            }
        };
        let midi_item = crate::midi::resolve_item(medium, reaper_project_ctx, &item)?;
        let midi_take = crate::midi::resolve_take(medium, midi_item, &take)?;
        let low = medium.low();
        let accessor = aa_sw::create_take_audio_accessor(low, midi_take);
        store(SendableAccessorPtr::new(accessor))
    }

    fn has_state_changed(&self, accessor_id: &str) -> bool {
        let Some(ptr) = get_ptr(accessor_id) else {
            warn!("has_state_changed: unknown accessor ID '{}'", accessor_id);
            return false;
        };
        let low = Reaper::get().medium_reaper().low();
        aa_sw::audio_accessor_state_changed(low, ptr.get())
    }

    fn get_samples(&self, request: GetSamplesRequest) -> AudioSampleData {
        let Some(ptr) = get_ptr(&request.accessor_id) else {
            warn!("get_samples: unknown accessor ID '{}'", request.accessor_id);
            return AudioSampleData::default();
        };
        let low = Reaper::get().medium_reaper().low();
        let buf_size = (request.num_channels * request.num_samples) as usize;
        let mut buf = vec![0.0f64; buf_size];
        let result = aa_sw::get_audio_accessor_samples(
            low,
            ptr.get(),
            request.sample_rate as i32,
            request.num_channels as i32,
            request.start_time,
            request.num_samples as i32,
            &mut buf,
        );
        if result <= 0 {
            return AudioSampleData::default();
        }
        let actual_samples = result as u32;
        let actual_size = (request.num_channels * actual_samples) as usize;
        buf.truncate(actual_size);
        AudioSampleData {
            samples: buf,
            sample_rate: request.sample_rate,
            num_channels: request.num_channels,
            num_samples: actual_samples,
        }
    }

    fn destroy_accessor(&self, accessor_id: &str) {
        let Some(ptr) = remove_ptr(accessor_id) else {
            warn!("destroy_accessor: unknown accessor ID '{}'", accessor_id);
            return;
        };
        let low = Reaper::get().medium_reaper().low();
        aa_sw::destroy_audio_accessor(low, ptr.get());
    }
}
