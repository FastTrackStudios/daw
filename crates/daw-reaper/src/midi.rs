//! REAPER MIDI Editing Implementation
//!
//! Stub implementation for MidiService - to be implemented when needed

pub struct ReaperMidi;

impl ReaperMidi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperMidi {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: Implement MidiService trait when needed
// REAPER's MIDI editing APIs are primarily in the low-level layer
// (MIDI_CountEvts, MIDI_GetNote, MIDI_SetNote, etc.)
// and will require careful wrapping for safe usage
