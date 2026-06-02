//! `impl AudioAccessors for Standalone` — stub.

use daw_proto::{
    AudioAccessors, AudioSampleData, GetSamplesRequest, ItemRef, ProjectContext, TakeRef, TrackRef,
};

use crate::sync::Standalone;

impl AudioAccessors for Standalone {
    fn create_track_accessor(&self, _project: ProjectContext, _track: TrackRef) -> Option<String> {
        None
    }
    fn create_take_accessor(
        &self,
        _project: ProjectContext,
        _item: ItemRef,
        _take: TakeRef,
    ) -> Option<String> {
        None
    }
    fn has_state_changed(&self, _accessor_id: &str) -> bool {
        false
    }
    fn get_samples(&self, _request: GetSamplesRequest) -> AudioSampleData {
        AudioSampleData::default()
    }
    fn destroy_accessor(&self, _accessor_id: &str) {}
}
