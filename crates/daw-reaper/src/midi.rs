//! REAPER MIDI editing/reading implementation.

use crate::main_thread;
use crate::project_context::resolve_project_context;
use crate::safe_wrappers::item as item_sw;
use crate::safe_wrappers::midi as sw;
use daw_proto::{
    HumanizeParams, ItemRef, MidiCC, MidiCCCreate, MidiNote, MidiNoteCreate, MidiPitchBend,
    MidiPitchBendCreate, MidiProgramChange, MidiService, MidiSysEx, MidiTakeLocation,
    ProjectContext, PpqRange, QuantizeParams, TakeRef, TrackRef,
};
use reaper_medium::{MediaItem, MediaItemTake, ProjectContext as ReaperProjectContext};
use roam::Context;
use tracing::warn;

// =============================================================================
// Public sync helpers — callable directly from the main thread
// =============================================================================

/// Create a new empty MIDI item on a track, returning the active take.
///
/// Must be called from the main thread.
pub fn create_midi_item_on_main_thread(
    track: *mut reaper_low::raw::MediaTrack,
    start_seconds: f64,
    end_seconds: f64,
) -> Option<MediaItemTake> {
    let low = reaper_high::Reaper::get().medium_reaper().low();
    let item = sw::create_new_midi_item(low, track, start_seconds, end_seconds);
    if item.is_null() {
        return None;
    }
    let take = sw::get_active_take(low, item);
    MediaItemTake::new(take)
}

/// Insert MIDI notes into a take, converting quarter-note positions to PPQ.
///
/// Each `MidiNoteCreate` contains `start_ppq` and `length_ppq`, but here we
/// treat `start_ppq` as a project quarter-note position and convert it to PPQ
/// using `MIDI_GetPPQPosFromProjQN`. This matches the guide_track use-case
/// where note positions are in quarter-notes.
///
/// Must be called from the main thread.
pub fn add_notes_to_take_on_main_thread(
    take: MediaItemTake,
    notes: &[MidiNoteCreate],
) {
    let low = reaper_high::Reaper::get().medium_reaper().low();

    for note in notes {
        let start_ppq = sw::get_ppq_pos_from_proj_qn(low, take, note.start_ppq);
        let end_ppq = start_ppq + note.length_ppq;

        sw::insert_note(
            low,
            take,
            false,                   // selected
            false,                   // muted
            start_ppq,               // startppqpos
            end_ppq,                 // endppqpos
            note.channel as i32,     // channel
            note.pitch as i32,       // pitch
            note.velocity as i32,    // velocity
        );
    }

    // Sort notes after bulk insertion
    sw::sort(low, take);
}

#[derive(Clone)]
pub struct ReaperMidi;

impl ReaperMidi {
    pub fn new() -> Self {
        Self
    }

    fn get_item_state_chunk(
        medium: &reaper_medium::Reaper,
        item: MediaItem,
        buffer_size: usize,
    ) -> Option<String> {
        item_sw::get_item_state_chunk(medium.low(), item, buffer_size)
    }

    fn extract_guid_from_chunk(chunk: &str) -> Option<String> {
        for line in chunk.lines() {
            if let Some(rest) = line.strip_prefix("GUID ") {
                return Some(rest.trim().to_string());
            }
        }
        None
    }

    pub fn resolve_item(
        medium: &reaper_medium::Reaper,
        project_ctx: ReaperProjectContext,
        item_ref: &ItemRef,
    ) -> Option<MediaItem> {
        match item_ref {
            ItemRef::ProjectIndex(index) => medium.get_media_item(project_ctx, *index),
            ItemRef::Index(index) => medium.get_media_item(project_ctx, *index),
            ItemRef::Guid(guid) => {
                let count = medium.count_media_items(project_ctx);
                for i in 0..count {
                    let Some(item) = medium.get_media_item(project_ctx, i) else {
                        continue;
                    };
                    let Some(chunk) = Self::get_item_state_chunk(medium, item, 2048) else {
                        continue;
                    };
                    if let Some(item_guid) = Self::extract_guid_from_chunk(&chunk)
                        && &item_guid == guid
                    {
                        return Some(item);
                    }
                }
                None
            }
        }
    }

    pub fn resolve_take(
        medium: &reaper_medium::Reaper,
        item: MediaItem,
        take_ref: &TakeRef,
    ) -> Option<MediaItemTake> {
        let low = medium.low();
        match take_ref {
            TakeRef::Active => {
                // Use medium's get_active_take which handles this properly
                crate::safe_wrappers::item::get_active_take(medium, item)
            }
            TakeRef::Index(index) => {
                let take_ptr = item_sw::get_take(low, item, *index as i32);
                MediaItemTake::new(take_ptr)
            }
            TakeRef::Guid(_) => crate::safe_wrappers::item::get_active_take(medium, item),
        }
    }

    fn resolve_take_for_location(
        medium: &reaper_medium::Reaper,
        location: &MidiTakeLocation,
    ) -> Option<MediaItemTake> {
        let project_ctx = resolve_project_context(&location.project);
        let item = Self::resolve_item(medium, project_ctx, &location.item)?;
        Self::resolve_take(medium, item, &location.take)
    }

    fn read_notes(medium: &reaper_medium::Reaper, take: MediaItemTake) -> Vec<MidiNote> {
        let low = medium.low();
        let counts = sw::count_events(low, take);

        let mut notes = Vec::with_capacity(counts.notes.max(0) as usize);
        for index in 0..counts.notes {
            let Some(n) = sw::get_note(low, take, index) else {
                continue;
            };
            notes.push(MidiNote {
                index: index as u32,
                channel: n.channel.clamp(0, 15) as u8,
                pitch: n.pitch.clamp(0, 127) as u8,
                velocity: n.velocity.clamp(1, 127) as u8,
                start_ppq: n.start_ppq,
                length_ppq: (n.end_ppq - n.start_ppq).max(0.0),
                selected: n.selected,
                muted: n.muted,
            });
        }
        notes
    }

    fn readonly_warn(method: &str) {
        warn!("ReaperMidi::{method} is read-only in this pass; skipping mutation");
    }
}

impl Default for ReaperMidi {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiService for ReaperMidi {
    async fn get_notes(&self, _cx: &Context, location: MidiTakeLocation) -> Vec<MidiNote> {
        main_thread::query(move || {
            let medium = reaper_high::Reaper::get().medium_reaper();
            let Some(take) = Self::resolve_take_for_location(medium, &location) else {
                return Vec::new();
            };
            Self::read_notes(medium, take)
        })
        .await
        .unwrap_or_default()
    }

    async fn get_notes_in_range(
        &self,
        cx: &Context,
        location: MidiTakeLocation,
        range: PpqRange,
    ) -> Vec<MidiNote> {
        self.get_notes(cx, location)
            .await
            .into_iter()
            .filter(|note| note.overlaps(range.start, range.end))
            .collect()
    }

    async fn get_selected_notes(&self, cx: &Context, location: MidiTakeLocation) -> Vec<MidiNote> {
        self.get_notes(cx, location)
            .await
            .into_iter()
            .filter(|note| note.selected)
            .collect()
    }

    async fn note_count(&self, cx: &Context, location: MidiTakeLocation) -> u32 {
        self.get_notes(cx, location).await.len() as u32
    }

    async fn create_midi_item(
        &self,
        _cx: &Context,
        project: ProjectContext,
        track: TrackRef,
        start_seconds: f64,
        end_seconds: f64,
    ) -> Option<MidiTakeLocation> {
        main_thread::query(move || {
            let reaper = reaper_high::Reaper::get();
            let proj = match &project {
                ProjectContext::Current => reaper.current_project(),
                ProjectContext::Project(guid) => {
                    crate::project_context::find_project_by_guid(guid)?
                }
            };
            let track_obj = crate::track::resolve_track_pub(&proj, &track)?;
            let raw_track = track_obj.raw().ok()?.as_ptr();
            let take = create_midi_item_on_main_thread(raw_track, start_seconds, end_seconds)?;

            // Build a MidiTakeLocation from the item/take we just created
            let low = reaper.medium_reaper().low();
            let item_ptr = item_sw::get_take_item(low, take);
            let item = MediaItem::new(item_ptr)?;

            // Get the item index in the project
            let item_count = reaper.medium_reaper().count_media_items(
                resolve_project_context(&project),
            );
            let mut item_index = None;
            for i in 0..item_count {
                if let Some(candidate) = reaper.medium_reaper().get_media_item(
                    resolve_project_context(&project),
                    i,
                ) {
                    if candidate.as_ptr() == item.as_ptr() {
                        item_index = Some(i);
                        break;
                    }
                }
            }

            Some(MidiTakeLocation::active(
                project,
                ItemRef::Index(item_index.unwrap_or(0)),
            ))
        })
        .await
        .flatten()
    }

    async fn add_note(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _note: MidiNoteCreate,
    ) -> u32 {
        Self::readonly_warn("add_note");
        0
    }

    async fn add_notes(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        notes: Vec<MidiNoteCreate>,
    ) -> Vec<u32> {
        main_thread::query(move || {
            let medium = reaper_high::Reaper::get().medium_reaper();
            let Some(take) = Self::resolve_take_for_location(medium, &location) else {
                return Vec::new();
            };
            let count_before = Self::read_notes(medium, take).len() as u32;
            add_notes_to_take_on_main_thread(take, &notes);
            // Return indices of newly added notes
            (count_before..count_before + notes.len() as u32).collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn delete_note(&self, _cx: &Context, _location: MidiTakeLocation, _index: u32) {
        Self::readonly_warn("delete_note");
    }

    async fn delete_notes(&self, _cx: &Context, _location: MidiTakeLocation, _indices: Vec<u32>) {
        Self::readonly_warn("delete_notes");
    }

    async fn delete_selected_notes(&self, _cx: &Context, _location: MidiTakeLocation) {
        Self::readonly_warn("delete_selected_notes");
    }

    async fn set_note_pitch(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _pitch: u8,
    ) {
        Self::readonly_warn("set_note_pitch");
    }

    async fn set_note_velocity(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _velocity: u8,
    ) {
        Self::readonly_warn("set_note_velocity");
    }

    async fn set_note_position(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _start_ppq: f64,
    ) {
        Self::readonly_warn("set_note_position");
    }

    async fn set_note_length(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _length_ppq: f64,
    ) {
        Self::readonly_warn("set_note_length");
    }

    async fn set_note_channel(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _channel: u8,
    ) {
        Self::readonly_warn("set_note_channel");
    }

    async fn set_note_selected(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _selected: bool,
    ) {
        Self::readonly_warn("set_note_selected");
    }

    async fn set_note_muted(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _muted: bool,
    ) {
        Self::readonly_warn("set_note_muted");
    }

    async fn select_all_notes(&self, _cx: &Context, _location: MidiTakeLocation, _selected: bool) {
        Self::readonly_warn("select_all_notes");
    }

    async fn transpose_notes(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _indices: Vec<u32>,
        _semitones: i8,
    ) {
        Self::readonly_warn("transpose_notes");
    }

    async fn quantize_notes(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _params: QuantizeParams,
    ) {
        Self::readonly_warn("quantize_notes");
    }

    async fn humanize_notes(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _params: HumanizeParams,
    ) {
        Self::readonly_warn("humanize_notes");
    }

    async fn get_ccs(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _controller: Option<u8>,
    ) -> Vec<MidiCC> {
        Vec::new()
    }

    async fn add_cc(&self, _cx: &Context, _location: MidiTakeLocation, _cc: MidiCCCreate) -> u32 {
        Self::readonly_warn("add_cc");
        0
    }

    async fn delete_cc(&self, _cx: &Context, _location: MidiTakeLocation, _index: u32) {
        Self::readonly_warn("delete_cc");
    }

    async fn set_cc_value(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _index: u32,
        _value: u8,
    ) {
        Self::readonly_warn("set_cc_value");
    }

    async fn get_pitch_bends(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
    ) -> Vec<MidiPitchBend> {
        Vec::new()
    }

    async fn add_pitch_bend(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
        _pb: MidiPitchBendCreate,
    ) -> u32 {
        Self::readonly_warn("add_pitch_bend");
        0
    }

    async fn get_program_changes(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
    ) -> Vec<MidiProgramChange> {
        Vec::new()
    }

    async fn get_sysex(&self, _cx: &Context, _location: MidiTakeLocation) -> Vec<MidiSysEx> {
        Vec::new()
    }
}
