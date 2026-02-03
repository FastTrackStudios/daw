//! Standalone live MIDI implementation

use daw_proto::live_midi::{
    LiveMidiEvent, LiveMidiService, MidiInputDevice, MidiMessage, MidiOutputDevice, SendMidiTiming,
};
use roam::Context;

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
    async fn get_input_devices(&self, _cx: &Context) -> Vec<MidiInputDevice> {
        vec![]
    }

    async fn get_output_devices(&self, _cx: &Context) -> Vec<MidiOutputDevice> {
        vec![]
    }

    async fn get_input_device(&self, _cx: &Context, _id: u32) -> Option<MidiInputDevice> {
        None
    }

    async fn get_output_device(&self, _cx: &Context, _id: u32) -> Option<MidiOutputDevice> {
        None
    }

    async fn open_input_device(&self, _cx: &Context, _id: u32) -> bool {
        false
    }

    async fn close_input_device(&self, _cx: &Context, _id: u32) {
        // No-op
    }

    async fn open_output_device(&self, _cx: &Context, _id: u32) -> bool {
        false
    }

    async fn close_output_device(&self, _cx: &Context, _id: u32) {
        // No-op
    }

    async fn send_midi(
        &self,
        _cx: &Context,
        _device_id: u32,
        _message: MidiMessage,
        _timing: SendMidiTiming,
    ) {
        // No-op
    }

    async fn send_midi_batch(&self, _cx: &Context, _device_id: u32, _events: Vec<LiveMidiEvent>) {
        // No-op
    }

    async fn subscribe_input(&self, _cx: &Context, _device_id: u32) -> bool {
        false
    }

    async fn unsubscribe_input(&self, _cx: &Context, _device_id: u32) {
        // No-op
    }
}
