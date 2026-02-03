//! REAPER Live MIDI Implementation
//!
//! Stub implementation for LiveMidiService - to be implemented when needed

pub struct ReaperLiveMidi;

impl ReaperLiveMidi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperLiveMidi {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: Implement LiveMidiService trait when needed
// Need to understand REAPER's MIDI device APIs and how to properly
// integrate with the real-time MIDI streaming requirements
