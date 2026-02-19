//! REAPER Live MIDI Implementation
//!
//! Implements LiveMidiService for REAPER, including StuffMIDIMessage injection
//! for routing MIDI to armed tracks via the virtual keyboard queue.

use crate::main_thread;
use daw_proto::live_midi::{
    LiveMidiEvent, LiveMidiService, MidiInputDevice, MidiMessage, MidiOutputDevice, SendMidiTiming,
    StuffMidiTarget,
};
use roam::Context;
use tracing::{debug, warn};

#[derive(Clone)]
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

impl LiveMidiService for ReaperLiveMidi {
    // === Device Enumeration (stubs — not yet implemented) ===

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

    async fn close_input_device(&self, _cx: &Context, _id: u32) {}

    async fn open_output_device(&self, _cx: &Context, _id: u32) -> bool {
        false
    }

    async fn close_output_device(&self, _cx: &Context, _id: u32) {}

    async fn send_midi(
        &self,
        _cx: &Context,
        _device_id: u32,
        _message: MidiMessage,
        _timing: SendMidiTiming,
    ) {
    }

    async fn send_midi_batch(&self, _cx: &Context, _device_id: u32, _events: Vec<LiveMidiEvent>) {}

    async fn subscribe_input(&self, _cx: &Context, _device_id: u32) -> bool {
        false
    }

    async fn unsubscribe_input(&self, _cx: &Context, _device_id: u32) {}

    // === MIDI Injection ===

    async fn stuff_midi_message(
        &self,
        _cx: &Context,
        target: StuffMidiTarget,
        message: MidiMessage,
    ) {
        let Some((status, data1, data2)) = message.to_raw_bytes() else {
            warn!(
                "stuff_midi_message: cannot convert {:?} to short message (SysEx/Raw not supported)",
                message
            );
            return;
        };

        let mode = match target {
            StuffMidiTarget::VirtualMidiKeyboard => 0,
            StuffMidiTarget::ControlInput => 1,
            StuffMidiTarget::VirtualMidiKeyboardCurrentChannel => 2,
        };

        debug!(
            "stuff_midi_message: mode={} status=0x{:02X} data1={} data2={}",
            mode, status, data1, data2
        );

        main_thread::run(move || {
            let reaper = reaper_high::Reaper::get();
            reaper.medium_reaper().low().StuffMIDIMessage(
                mode,
                status as i32,
                data1 as i32,
                data2 as i32,
            );
        });
    }
}
