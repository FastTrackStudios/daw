//! MIDI editing service trait

use super::{MidiCC, MidiNote, MidiNoteCreate, MidiPitchBend, MidiProgramChange, MidiSysEx};
use crate::item::{ItemRef, TakeRef};
use crate::project::ProjectContext;
use crate::TrackRef;
use facet::Facet;
use roam::service;

/// Location of a MIDI take (project + item + take)
#[derive(Clone, Debug, Facet)]
pub struct MidiTakeLocation {
    /// Project context
    pub project: ProjectContext,
    /// Item containing the take
    pub item: ItemRef,
    /// Take reference
    pub take: TakeRef,
}

impl MidiTakeLocation {
    /// Create a new MIDI take location
    pub fn new(project: ProjectContext, item: ItemRef, take: TakeRef) -> Self {
        Self {
            project,
            item,
            take,
        }
    }

    /// Create for the active take of an item
    pub fn active(project: ProjectContext, item: ItemRef) -> Self {
        Self::new(project, item, TakeRef::Active)
    }
}

/// PPQ range for queries
#[derive(Clone, Debug, Facet)]
pub struct PpqRange {
    /// Start position in PPQ
    pub start: f64,
    /// End position in PPQ
    pub end: f64,
}

impl PpqRange {
    /// Create a new PPQ range
    pub fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }
}

/// Parameters for quantizing notes
#[derive(Clone, Debug, Facet)]
pub struct QuantizeParams {
    /// Note indices to quantize (empty = selected notes)
    pub indices: Vec<u32>,
    /// Grid size in PPQ (1.0 = quarter note)
    pub grid_ppq: f64,
    /// Strength (0.0 = no change, 1.0 = full snap)
    pub strength: f64,
}

/// Parameters for humanizing notes
#[derive(Clone, Debug, Facet)]
pub struct HumanizeParams {
    /// Note indices to humanize (empty = selected notes)
    pub indices: Vec<u32>,
    /// Random timing variation range in PPQ
    pub timing_range_ppq: f64,
    /// Random velocity variation range
    pub velocity_range: u8,
}

/// Parameters for creating a CC event
#[derive(Clone, Debug, Facet)]
pub struct MidiCCCreate {
    /// MIDI channel (0-15)
    pub channel: u8,
    /// Controller number (0-127)
    pub controller: u8,
    /// Controller value (0-127)
    pub value: u8,
    /// Position in PPQ
    pub position_ppq: f64,
}

impl MidiCCCreate {
    /// Create a new CC event
    pub fn new(channel: u8, controller: u8, value: u8, position_ppq: f64) -> Self {
        Self {
            channel: channel & 0x0F,
            controller: controller & 0x7F,
            value: value & 0x7F,
            position_ppq,
        }
    }
}

/// Parameters for creating a pitch bend event
#[derive(Clone, Debug, Facet)]
pub struct MidiPitchBendCreate {
    /// MIDI channel (0-15)
    pub channel: u8,
    /// Pitch bend value (-8192 to 8191)
    pub value: i16,
    /// Position in PPQ
    pub position_ppq: f64,
}

impl MidiPitchBendCreate {
    /// Create a new pitch bend event
    pub fn new(channel: u8, value: i16, position_ppq: f64) -> Self {
        Self {
            channel: channel & 0x0F,
            value: value.clamp(-8192, 8191),
            position_ppq,
        }
    }
}

/// Service for editing MIDI data in takes
///
/// This service provides CRUD operations on MIDI notes, CC events, and other
/// MIDI data within takes. For real-time MIDI I/O, see `LiveMidiService`.
#[service]
pub trait MidiService {
    // === Note Queries ===

    /// Get all notes in a MIDI take
    async fn get_notes(&self, location: MidiTakeLocation) -> Vec<MidiNote>;

    /// Get notes within a PPQ range
    async fn get_notes_in_range(
        &self,
        location: MidiTakeLocation,
        range: PpqRange,
    ) -> Vec<MidiNote>;

    /// Get only selected notes
    async fn get_selected_notes(&self, location: MidiTakeLocation) -> Vec<MidiNote>;

    /// Get the total note count
    async fn note_count(&self, location: MidiTakeLocation) -> u32;

    // === Note CRUD ===

    /// Create a new empty MIDI item on a track, returning the take location
    async fn create_midi_item(
        &self,
        project: ProjectContext,
        track: TrackRef,
        start_seconds: f64,
        end_seconds: f64,
    ) -> Option<MidiTakeLocation>;

    /// Add a note, returns the note index
    async fn add_note(&self, location: MidiTakeLocation, note: MidiNoteCreate) -> u32;

    /// Add multiple notes, returns their indices
    async fn add_notes(&self, location: MidiTakeLocation, notes: Vec<MidiNoteCreate>) -> Vec<u32>;

    /// Delete a note by index
    async fn delete_note(&self, location: MidiTakeLocation, index: u32);

    /// Delete multiple notes
    async fn delete_notes(&self, location: MidiTakeLocation, indices: Vec<u32>);

    /// Delete all selected notes
    async fn delete_selected_notes(&self, location: MidiTakeLocation);

    // === Note Modification ===

    /// Set note pitch
    async fn set_note_pitch(&self, location: MidiTakeLocation, index: u32, pitch: u8);

    /// Set note velocity
    async fn set_note_velocity(&self, location: MidiTakeLocation, index: u32, velocity: u8);

    /// Set note position
    async fn set_note_position(&self, location: MidiTakeLocation, index: u32, start_ppq: f64);

    /// Set note length
    async fn set_note_length(&self, location: MidiTakeLocation, index: u32, length_ppq: f64);

    /// Set note channel
    async fn set_note_channel(&self, location: MidiTakeLocation, index: u32, channel: u8);

    /// Set note selected state
    async fn set_note_selected(&self, location: MidiTakeLocation, index: u32, selected: bool);

    /// Set note muted state
    async fn set_note_muted(&self, location: MidiTakeLocation, index: u32, muted: bool);

    // === Batch Operations ===

    /// Select or deselect all notes
    async fn select_all_notes(&self, location: MidiTakeLocation, selected: bool);

    /// Transpose notes by semitones
    async fn transpose_notes(&self, location: MidiTakeLocation, indices: Vec<u32>, semitones: i8);

    /// Quantize notes to a grid
    async fn quantize_notes(&self, location: MidiTakeLocation, params: QuantizeParams);

    /// Humanize notes (add random variation)
    async fn humanize_notes(&self, location: MidiTakeLocation, params: HumanizeParams);

    // === CC Queries ===

    /// Get CC events (optionally filtered by controller number)
    async fn get_ccs(&self, location: MidiTakeLocation, controller: Option<u8>) -> Vec<MidiCC>;

    // === CC CRUD ===

    /// Add a CC event, returns the event index
    async fn add_cc(&self, location: MidiTakeLocation, cc: MidiCCCreate) -> u32;

    /// Delete a CC event
    async fn delete_cc(&self, location: MidiTakeLocation, index: u32);

    /// Set CC value
    async fn set_cc_value(&self, location: MidiTakeLocation, index: u32, value: u8);

    // === Other Events ===

    /// Get pitch bend events
    async fn get_pitch_bends(&self, location: MidiTakeLocation) -> Vec<MidiPitchBend>;

    /// Add a pitch bend event
    async fn add_pitch_bend(&self, location: MidiTakeLocation, pb: MidiPitchBendCreate) -> u32;

    /// Get program change events
    async fn get_program_changes(&self, location: MidiTakeLocation) -> Vec<MidiProgramChange>;

    /// Get SysEx events
    async fn get_sysex(&self, location: MidiTakeLocation) -> Vec<MidiSysEx>;
}
