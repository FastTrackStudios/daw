//! MIDI message types for real-time I/O

use facet::Facet;

/// A parsed MIDI message
#[repr(C)]
#[derive(Clone, Debug, Facet)]
pub enum MidiMessage {
    /// Note On event
    NoteOn { channel: u8, note: u8, velocity: u8 },
    /// Note Off event
    NoteOff { channel: u8, note: u8, velocity: u8 },
    /// Control Change (CC)
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Program Change
    ProgramChange { channel: u8, program: u8 },
    /// Pitch Bend (-8192 to 8191)
    PitchBend { channel: u8, value: i16 },
    /// Channel Pressure (Aftertouch)
    ChannelPressure { channel: u8, pressure: u8 },
    /// Polyphonic Key Pressure
    PolyPressure { channel: u8, note: u8, pressure: u8 },
    /// System Exclusive
    SysEx(Vec<u8>),
    /// Raw MIDI bytes (for unrecognized messages)
    Raw(Vec<u8>),
}

impl MidiMessage {
    /// Create a Note On message
    pub fn note_on(channel: u8, note: u8, velocity: u8) -> Self {
        Self::NoteOn {
            channel: channel & 0x0F,
            note: note & 0x7F,
            velocity: velocity & 0x7F,
        }
    }

    /// Create a Note Off message
    pub fn note_off(channel: u8, note: u8, velocity: u8) -> Self {
        Self::NoteOff {
            channel: channel & 0x0F,
            note: note & 0x7F,
            velocity: velocity & 0x7F,
        }
    }

    /// Create a Control Change message
    pub fn control_change(channel: u8, controller: u8, value: u8) -> Self {
        Self::ControlChange {
            channel: channel & 0x0F,
            controller: controller & 0x7F,
            value: value & 0x7F,
        }
    }

    /// Create a Program Change message
    pub fn program_change(channel: u8, program: u8) -> Self {
        Self::ProgramChange {
            channel: channel & 0x0F,
            program: program & 0x7F,
        }
    }

    /// Create a Pitch Bend message
    pub fn pitch_bend(channel: u8, value: i16) -> Self {
        Self::PitchBend {
            channel: channel & 0x0F,
            value: value.clamp(-8192, 8191),
        }
    }

    /// Get the MIDI channel (0-15) if applicable
    pub fn channel(&self) -> Option<u8> {
        match self {
            Self::NoteOn { channel, .. }
            | Self::NoteOff { channel, .. }
            | Self::ControlChange { channel, .. }
            | Self::ProgramChange { channel, .. }
            | Self::PitchBend { channel, .. }
            | Self::ChannelPressure { channel, .. }
            | Self::PolyPressure { channel, .. } => Some(*channel),
            Self::SysEx(_) | Self::Raw(_) => None,
        }
    }

    /// Check if this is a note message (on or off)
    pub fn is_note(&self) -> bool {
        matches!(self, Self::NoteOn { .. } | Self::NoteOff { .. })
    }

    /// Check if this is a Note On with non-zero velocity
    pub fn is_note_on(&self) -> bool {
        matches!(self, Self::NoteOn { velocity, .. } if *velocity > 0)
    }

    /// Check if this is a Note Off (or Note On with velocity 0)
    pub fn is_note_off(&self) -> bool {
        matches!(
            self,
            Self::NoteOff { .. } | Self::NoteOn { velocity: 0, .. }
        )
    }
}

impl MidiMessage {
    /// Convert to raw MIDI bytes (status, data1, data2).
    ///
    /// Returns None for SysEx and Raw messages (not short messages).
    pub fn to_raw_bytes(&self) -> Option<(u8, u8, u8)> {
        match *self {
            Self::NoteOn {
                channel,
                note,
                velocity,
            } => Some((0x90 | channel, note, velocity)),
            Self::NoteOff {
                channel,
                note,
                velocity,
            } => Some((0x80 | channel, note, velocity)),
            Self::ControlChange {
                channel,
                controller,
                value,
            } => Some((0xB0 | channel, controller, value)),
            Self::ProgramChange { channel, program } => Some((0xC0 | channel, program, 0)),
            Self::PitchBend { channel, value } => {
                let unsigned = (value + 8192) as u16;
                Some((
                    0xE0 | channel,
                    (unsigned & 0x7F) as u8,
                    (unsigned >> 7) as u8,
                ))
            }
            Self::ChannelPressure { channel, pressure } => Some((0xD0 | channel, pressure, 0)),
            Self::PolyPressure {
                channel,
                note,
                pressure,
            } => Some((0xA0 | channel, note, pressure)),
            Self::SysEx(_) | Self::Raw(_) => None,
        }
    }
}

/// Target queue for StuffMIDIMessage injection.
///
/// Controls where injected MIDI is routed within REAPER.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, Facet)]
pub enum StuffMidiTarget {
    /// Virtual MIDI keyboard queue — routes to armed tracks with VKB input.
    #[default]
    VirtualMidiKeyboard,
    /// MIDI-as-control-input queue — routes to control surfaces / actions.
    ControlInput,
    /// Virtual MIDI keyboard on the currently selected channel.
    VirtualMidiKeyboardCurrentChannel,
}

/// When to send a MIDI message
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Facet)]
pub enum SendMidiTiming {
    /// Send immediately
    #[default]
    Instantly,
    /// Send at a specific frame offset (in 1/1024000 second units)
    AtFrameOffset(u32),
}
