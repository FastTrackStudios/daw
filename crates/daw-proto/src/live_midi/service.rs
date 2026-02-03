//! Live MIDI service trait

use super::{LiveMidiEvent, MidiInputDevice, MidiMessage, MidiOutputDevice, SendMidiTiming};
use roam::service;

/// Service for real-time MIDI input/output
///
/// This service handles MIDI device management and real-time MIDI I/O during
/// playback. For editing MIDI data in takes, see `MidiService`.
#[service]
pub trait LiveMidiService {
    // === Device Enumeration ===

    /// Get all available MIDI input devices
    async fn get_input_devices(&self) -> Vec<MidiInputDevice>;

    /// Get all available MIDI output devices
    async fn get_output_devices(&self) -> Vec<MidiOutputDevice>;

    /// Get a specific input device by ID
    async fn get_input_device(&self, id: u32) -> Option<MidiInputDevice>;

    /// Get a specific output device by ID
    async fn get_output_device(&self, id: u32) -> Option<MidiOutputDevice>;

    // === Device State ===

    /// Open a MIDI input device for receiving
    /// Returns true if successful
    async fn open_input_device(&self, id: u32) -> bool;

    /// Close a MIDI input device
    async fn close_input_device(&self, id: u32);

    /// Open a MIDI output device for sending
    /// Returns true if successful
    async fn open_output_device(&self, id: u32) -> bool;

    /// Close a MIDI output device
    async fn close_output_device(&self, id: u32);

    // === Output ===

    /// Send a MIDI message to a device
    async fn send_midi(&self, device_id: u32, message: MidiMessage, timing: SendMidiTiming);

    /// Send multiple MIDI events (with timing)
    async fn send_midi_batch(&self, device_id: u32, events: Vec<LiveMidiEvent>);

    // === Input Subscription ===
    // Events are delivered via the event system (LiveMidiServiceEvent::MidiReceived)

    /// Subscribe to MIDI input from a device
    /// Returns true if successful
    async fn subscribe_input(&self, device_id: u32) -> bool;

    /// Unsubscribe from MIDI input
    async fn unsubscribe_input(&self, device_id: u32);
}
