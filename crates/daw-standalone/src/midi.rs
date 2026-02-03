//! Standalone MIDI editing implementation

use daw_proto::midi::{
    HumanizeParams, MidiCC, MidiCCCreate, MidiNote, MidiNoteCreate, MidiPitchBend,
    MidiPitchBendCreate, MidiProgramChange, MidiService, MidiSysEx, MidiTakeLocation, PpqRange,
    QuantizeParams,
};
use roam::Context;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Internal note state
#[derive(Clone)]
struct NoteState {
    index: u32,
    channel: u8,
    pitch: u8,
    velocity: u8,
    start_ppq: f64,
    length_ppq: f64,
    selected: bool,
    muted: bool,
}

impl NoteState {
    fn to_note(&self) -> MidiNote {
        MidiNote {
            index: self.index,
            channel: self.channel,
            pitch: self.pitch,
            velocity: self.velocity,
            start_ppq: self.start_ppq,
            length_ppq: self.length_ppq,
            selected: self.selected,
            muted: self.muted,
        }
    }
}

/// Internal CC state
#[derive(Clone)]
struct CcState {
    index: u32,
    channel: u8,
    controller: u8,
    value: u8,
    position_ppq: f64,
    selected: bool,
}

impl CcState {
    fn to_cc(&self) -> MidiCC {
        MidiCC {
            index: self.index,
            channel: self.channel,
            controller: self.controller,
            value: self.value,
            position_ppq: self.position_ppq,
            selected: self.selected,
        }
    }
}

/// Internal pitch bend state
#[derive(Clone)]
struct PitchBendState {
    index: u32,
    channel: u8,
    value: i16,
    position_ppq: f64,
    selected: bool,
}

impl PitchBendState {
    fn to_pitch_bend(&self) -> MidiPitchBend {
        MidiPitchBend {
            index: self.index,
            channel: self.channel,
            value: self.value,
            position_ppq: self.position_ppq,
            selected: self.selected,
        }
    }
}

/// MIDI data for a take
#[derive(Clone, Default)]
struct TakeMidiData {
    take_guid: String,
    notes: Vec<NoteState>,
    ccs: Vec<CcState>,
    pitch_bends: Vec<PitchBendState>,
}

/// Standalone MIDI editing service implementation
#[derive(Clone, Default)]
pub struct StandaloneMidi {
    takes: Arc<RwLock<Vec<TakeMidiData>>>,
}

impl StandaloneMidi {
    pub fn new() -> Self {
        Self {
            takes: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn get_take_guid(_location: &MidiTakeLocation) -> String {
        // In a real implementation, we'd look up the take GUID from the item
        // For the stub, we just use a placeholder
        Uuid::new_v4().to_string()
    }

    async fn get_or_create_take(&self, location: &MidiTakeLocation) -> String {
        let take_guid = Self::get_take_guid(location);
        let mut takes = self.takes.write().await;
        if !takes.iter().any(|t| t.take_guid == take_guid) {
            takes.push(TakeMidiData {
                take_guid: take_guid.clone(),
                notes: Vec::new(),
                ccs: Vec::new(),
                pitch_bends: Vec::new(),
            });
        }
        take_guid
    }
}

impl MidiService for StandaloneMidi {
    async fn get_notes(&self, _cx: &Context, location: MidiTakeLocation) -> Vec<MidiNote> {
        let take_guid = Self::get_take_guid(&location);
        let takes = self.takes.read().await;
        takes
            .iter()
            .find(|t| t.take_guid == take_guid)
            .map(|t| t.notes.iter().map(|n| n.to_note()).collect())
            .unwrap_or_default()
    }

    async fn get_notes_in_range(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        range: PpqRange,
    ) -> Vec<MidiNote> {
        let take_guid = Self::get_take_guid(&location);
        let takes = self.takes.read().await;
        takes
            .iter()
            .find(|t| t.take_guid == take_guid)
            .map(|t| {
                t.notes
                    .iter()
                    .filter(|n| n.start_ppq >= range.start && n.start_ppq <= range.end)
                    .map(|n| n.to_note())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn get_selected_notes(&self, _cx: &Context, location: MidiTakeLocation) -> Vec<MidiNote> {
        let take_guid = Self::get_take_guid(&location);
        let takes = self.takes.read().await;
        takes
            .iter()
            .find(|t| t.take_guid == take_guid)
            .map(|t| {
                t.notes
                    .iter()
                    .filter(|n| n.selected)
                    .map(|n| n.to_note())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn note_count(&self, _cx: &Context, location: MidiTakeLocation) -> u32 {
        let take_guid = Self::get_take_guid(&location);
        let takes = self.takes.read().await;
        takes
            .iter()
            .find(|t| t.take_guid == take_guid)
            .map(|t| t.notes.len() as u32)
            .unwrap_or(0)
    }

    async fn add_note(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        note: MidiNoteCreate,
    ) -> u32 {
        let take_guid = self.get_or_create_take(&location).await;
        let mut takes = self.takes.write().await;
        let take = takes.iter_mut().find(|t| t.take_guid == take_guid).unwrap();
        let index = take.notes.len() as u32;
        take.notes.push(NoteState {
            index,
            channel: note.channel,
            pitch: note.pitch,
            velocity: note.velocity,
            start_ppq: note.start_ppq,
            length_ppq: note.length_ppq,
            selected: false,
            muted: false,
        });
        index
    }

    async fn add_notes(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        notes: Vec<MidiNoteCreate>,
    ) -> Vec<u32> {
        let take_guid = self.get_or_create_take(&location).await;
        let mut takes = self.takes.write().await;
        let take = takes.iter_mut().find(|t| t.take_guid == take_guid).unwrap();
        let mut indices = Vec::new();
        for note in notes {
            let index = take.notes.len() as u32;
            take.notes.push(NoteState {
                index,
                channel: note.channel,
                pitch: note.pitch,
                velocity: note.velocity,
                start_ppq: note.start_ppq,
                length_ppq: note.length_ppq,
                selected: false,
                muted: false,
            });
            indices.push(index);
        }
        indices
    }

    async fn delete_note(&self, _cx: &Context, location: MidiTakeLocation, index: u32) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            take.notes.retain(|n| n.index != index);
            // Re-index
            for (i, n) in take.notes.iter_mut().enumerate() {
                n.index = i as u32;
            }
        }
    }

    async fn delete_notes(&self, _cx: &Context, location: MidiTakeLocation, indices: Vec<u32>) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            take.notes.retain(|n| !indices.contains(&n.index));
            // Re-index
            for (i, n) in take.notes.iter_mut().enumerate() {
                n.index = i as u32;
            }
        }
    }

    async fn delete_selected_notes(&self, _cx: &Context, location: MidiTakeLocation) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            take.notes.retain(|n| !n.selected);
            // Re-index
            for (i, n) in take.notes.iter_mut().enumerate() {
                n.index = i as u32;
            }
        }
    }

    async fn set_note_pitch(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        pitch: u8,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.pitch = pitch & 0x7F;
            }
        }
    }

    async fn set_note_velocity(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        velocity: u8,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.velocity = velocity.clamp(1, 127);
            }
        }
    }

    async fn set_note_position(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        start_ppq: f64,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.start_ppq = start_ppq;
            }
        }
    }

    async fn set_note_length(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        length_ppq: f64,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.length_ppq = length_ppq;
            }
        }
    }

    async fn set_note_channel(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        channel: u8,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.channel = channel & 0x0F;
            }
        }
    }

    async fn set_note_selected(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        selected: bool,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.selected = selected;
            }
        }
    }

    async fn set_note_muted(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        index: u32,
        muted: bool,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(note) = take.notes.iter_mut().find(|n| n.index == index) {
                note.muted = muted;
            }
        }
    }

    async fn select_all_notes(&self, _cx: &Context, location: MidiTakeLocation, selected: bool) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            for note in &mut take.notes {
                note.selected = selected;
            }
        }
    }

    async fn transpose_notes(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        indices: Vec<u32>,
        semitones: i8,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            for note in &mut take.notes {
                let should_transpose =
                    indices.is_empty() && note.selected || indices.contains(&note.index);
                if should_transpose {
                    let new_pitch = (note.pitch as i16 + semitones as i16).clamp(0, 127) as u8;
                    note.pitch = new_pitch;
                }
            }
        }
    }

    async fn quantize_notes(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        params: QuantizeParams,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            for note in &mut take.notes {
                let should_quantize = params.indices.is_empty() && note.selected
                    || params.indices.contains(&note.index);
                if should_quantize {
                    let grid = params.grid_ppq;
                    let quantized = (note.start_ppq / grid).round() * grid;
                    note.start_ppq =
                        note.start_ppq + (quantized - note.start_ppq) * params.strength;
                }
            }
        }
    }

    async fn humanize_notes(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        params: HumanizeParams,
    ) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            for note in &mut take.notes {
                let should_humanize = params.indices.is_empty() && note.selected
                    || params.indices.contains(&note.index);
                if should_humanize {
                    // Simple random variation (using index as seed for determinism)
                    let timing_offset =
                        ((note.index as f64 * 0.1).sin() * params.timing_range_ppq) as f64;
                    let velocity_offset =
                        ((note.index as f64 * 0.2).cos() * params.velocity_range as f64) as i8;
                    note.start_ppq += timing_offset;
                    note.velocity =
                        (note.velocity as i16 + velocity_offset as i16).clamp(1, 127) as u8;
                }
            }
        }
    }

    async fn get_ccs(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        controller: Option<u8>,
    ) -> Vec<MidiCC> {
        let take_guid = Self::get_take_guid(&location);
        let takes = self.takes.read().await;
        takes
            .iter()
            .find(|t| t.take_guid == take_guid)
            .map(|t| {
                t.ccs
                    .iter()
                    .filter(|c| controller.is_none() || Some(c.controller) == controller)
                    .map(|c| c.to_cc())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn add_cc(&self, _cx: &Context, location: MidiTakeLocation, cc: MidiCCCreate) -> u32 {
        let take_guid = self.get_or_create_take(&location).await;
        let mut takes = self.takes.write().await;
        let take = takes.iter_mut().find(|t| t.take_guid == take_guid).unwrap();
        let index = take.ccs.len() as u32;
        take.ccs.push(CcState {
            index,
            channel: cc.channel,
            controller: cc.controller,
            value: cc.value,
            position_ppq: cc.position_ppq,
            selected: false,
        });
        index
    }

    async fn delete_cc(&self, _cx: &Context, location: MidiTakeLocation, index: u32) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            take.ccs.retain(|c| c.index != index);
            // Re-index
            for (i, c) in take.ccs.iter_mut().enumerate() {
                c.index = i as u32;
            }
        }
    }

    async fn set_cc_value(&self, _cx: &Context, location: MidiTakeLocation, index: u32, value: u8) {
        let take_guid = Self::get_take_guid(&location);
        let mut takes = self.takes.write().await;
        if let Some(take) = takes.iter_mut().find(|t| t.take_guid == take_guid) {
            if let Some(cc) = take.ccs.iter_mut().find(|c| c.index == index) {
                cc.value = value & 0x7F;
            }
        }
    }

    async fn get_pitch_bends(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
    ) -> Vec<MidiPitchBend> {
        let take_guid = Self::get_take_guid(&location);
        let takes = self.takes.read().await;
        takes
            .iter()
            .find(|t| t.take_guid == take_guid)
            .map(|t| t.pitch_bends.iter().map(|p| p.to_pitch_bend()).collect())
            .unwrap_or_default()
    }

    async fn add_pitch_bend(
        &self,
        _cx: &Context,
        location: MidiTakeLocation,
        pb: MidiPitchBendCreate,
    ) -> u32 {
        let take_guid = self.get_or_create_take(&location).await;
        let mut takes = self.takes.write().await;
        let take = takes.iter_mut().find(|t| t.take_guid == take_guid).unwrap();
        let index = take.pitch_bends.len() as u32;
        take.pitch_bends.push(PitchBendState {
            index,
            channel: pb.channel,
            value: pb.value,
            position_ppq: pb.position_ppq,
            selected: false,
        });
        index
    }

    async fn get_program_changes(
        &self,
        _cx: &Context,
        _location: MidiTakeLocation,
    ) -> Vec<MidiProgramChange> {
        vec![]
    }

    async fn get_sysex(&self, _cx: &Context, _location: MidiTakeLocation) -> Vec<MidiSysEx> {
        vec![]
    }
}
