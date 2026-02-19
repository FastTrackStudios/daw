//! REAPER MIDI editing/reading implementation.

use crate::main_thread;
use crate::project_context::resolve_project_context;
use daw_proto::{
    HumanizeParams, ItemRef, MidiCC, MidiCCCreate, MidiNote, MidiNoteCreate, MidiPitchBend,
    MidiPitchBendCreate, MidiProgramChange, MidiService, MidiSysEx, MidiTakeLocation, PpqRange,
    QuantizeParams, TakeRef,
};
use reaper_medium::{MediaItem, MediaItemTake, ProjectContext as ReaperProjectContext};
use roam::Context;
use tracing::warn;

#[derive(Clone)]
pub struct ReaperMidi;

impl ReaperMidi {
    pub fn new() -> Self {
        Self
    }

    unsafe fn get_item_state_chunk(
        medium: &reaper_medium::Reaper,
        item: MediaItem,
        buffer_size: usize,
    ) -> Option<String> {
        let mut buf = vec![0u8; buffer_size];
        let ok = unsafe {
            medium.low().GetSetItemState(
                item.as_ptr(),
                buf.as_mut_ptr() as *mut i8,
                buffer_size as i32,
            )
        };
        if !ok {
            return None;
        }
        let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..len]).into_owned())
    }

    fn extract_guid_from_chunk(chunk: &str) -> Option<String> {
        for line in chunk.lines() {
            if let Some(rest) = line.strip_prefix("GUID ") {
                return Some(rest.trim().to_string());
            }
        }
        None
    }

    fn resolve_item(
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
                    let Some(chunk) = (unsafe { Self::get_item_state_chunk(medium, item, 2048) })
                    else {
                        continue;
                    };
                    if let Some(item_guid) = Self::extract_guid_from_chunk(&chunk) {
                        if &item_guid == guid {
                            return Some(item);
                        }
                    }
                }
                None
            }
        }
    }

    fn resolve_take(
        medium: &reaper_medium::Reaper,
        item: MediaItem,
        take_ref: &TakeRef,
    ) -> Option<MediaItemTake> {
        match take_ref {
            TakeRef::Active => unsafe { medium.get_active_take(item) },
            TakeRef::Index(index) => {
                let take_ptr = unsafe { medium.low().GetTake(item.as_ptr(), *index as i32) };
                MediaItemTake::new(take_ptr)
            }
            TakeRef::Guid(_) => unsafe { medium.get_active_take(item) },
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
        let mut note_count: i32 = 0;
        let mut cc_count: i32 = 0;
        let mut text_sysex_count: i32 = 0;
        unsafe {
            medium.low().MIDI_CountEvts(
                take.as_ptr(),
                &mut note_count,
                &mut cc_count,
                &mut text_sysex_count,
            );
        }

        let mut notes = Vec::with_capacity(note_count.max(0) as usize);
        for index in 0..note_count {
            let mut selected = false;
            let mut muted = false;
            let mut start_ppq = 0.0;
            let mut end_ppq = 0.0;
            let mut channel = 0;
            let mut pitch = 0;
            let mut velocity = 0;
            let success = unsafe {
                medium.low().MIDI_GetNote(
                    take.as_ptr(),
                    index,
                    &mut selected,
                    &mut muted,
                    &mut start_ppq,
                    &mut end_ppq,
                    &mut channel,
                    &mut pitch,
                    &mut velocity,
                )
            };
            if !success {
                continue;
            }
            notes.push(MidiNote {
                index: index as u32,
                channel: channel.clamp(0, 15) as u8,
                pitch: pitch.clamp(0, 127) as u8,
                velocity: velocity.clamp(1, 127) as u8,
                start_ppq,
                length_ppq: (end_ppq - start_ppq).max(0.0),
                selected,
                muted,
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
        _location: MidiTakeLocation,
        _notes: Vec<MidiNoteCreate>,
    ) -> Vec<u32> {
        Self::readonly_warn("add_notes");
        Vec::new()
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
