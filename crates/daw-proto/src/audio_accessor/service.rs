//! Audio accessor service trait

use super::{AudioSampleData, GetSamplesRequest};
use crate::item::{ItemRef, TakeRef};
use crate::project::ProjectContext;
use crate::track::TrackRef;
use roam::service;

/// Service for creating audio accessors that provide random-access sample reading.
///
/// Audio accessors are handle-based: `create_*` returns an opaque string ID,
/// which is passed to subsequent calls. Call `destroy_accessor` when done
/// to free REAPER resources.
#[service]
pub trait AudioAccessorService {
    /// Create an audio accessor for a track, returning an opaque handle ID.
    async fn create_track_accessor(
        &self,
        project: ProjectContext,
        track: TrackRef,
    ) -> Option<String>;

    /// Create an audio accessor for a take, returning an opaque handle ID.
    async fn create_take_accessor(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> Option<String>;

    /// Check if the accessor's underlying audio has changed since creation.
    async fn has_state_changed(&self, accessor_id: String) -> bool;

    /// Read interleaved sample data from the accessor.
    async fn get_samples(&self, request: GetSamplesRequest) -> AudioSampleData;

    /// Destroy an accessor and free its resources.
    async fn destroy_accessor(&self, accessor_id: String);
}
