//! Standalone live MIDI implementation

use daw_proto::live_midi::{
    LiveMidiEvent, LiveMidiService, MidiInputDevice, MidiMessage, MidiOutputDevice, SendMidiTiming,
    StuffMidiTarget,
};

/// Standalone live MIDI service implementation
///
/// This is a stub implementation that provides no actual MIDI devices.
#[derive(Clone, Default)]
pub struct StandaloneLiveMidi;

impl StandaloneLiveMidi {
    pub fn new() -> Self {
        Self
    }
}

impl LiveMidiService for StandaloneLiveMidi {
    async fn get_input_devices(&self) -> Vec<MidiInputDevice> {
        vec![]
    }

    async fn get_output_devices(&self) -> Vec<MidiOutputDevice> {
        vec![]
    }

    async fn get_input_device(&self, __id: u32) -> Option<MidiInputDevice> {
        None
    }

    async fn get_output_device(&self, __id: u32) -> Option<MidiOutputDevice> {
        None
    }

    async fn open_input_device(&self, __id: u32) -> bool {
        false
    }

    async fn close_input_device(&self, __id: u32) {
        // No-op
    }

    async fn open_output_device(&self, __id: u32) -> bool {
        false
    }

    async fn close_output_device(&self, __id: u32) {
        // No-op
    }

    async fn send_midi(
        &self,
        _device_id: u32,
        _message: MidiMessage,
        _timing: SendMidiTiming,
    ) {
        // No-op
    }

    async fn send_midi_batch(&self, __device_id: u32, _events: Vec<LiveMidiEvent>) {
        // No-op
    }

    async fn subscribe_input(&self, __device_id: u32) -> bool {
        false
    }

    async fn unsubscribe_input(&self, __device_id: u32) {
        // No-op
    }

    async fn stuff_midi_message(
        &self,
        _target: StuffMidiTarget,
        _message: MidiMessage,
    ) {
        // No-op
    }
}
