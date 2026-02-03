//! MIDI CC and other event types for editing

use facet::Facet;

/// A MIDI Control Change event in a take
#[derive(Clone, Debug, Facet)]
pub struct MidiCC {
    /// Index of this CC event in the take
    pub index: u32,
    /// MIDI channel (0-15)
    pub channel: u8,
    /// Controller number (0-127)
    pub controller: u8,
    /// Controller value (0-127)
    pub value: u8,
    /// Position in PPQ
    pub position_ppq: f64,
    /// Whether this event is selected
    pub selected: bool,
}

/// A MIDI Pitch Bend event in a take
#[derive(Clone, Debug, Facet)]
pub struct MidiPitchBend {
    /// Index of this event in the take
    pub index: u32,
    /// MIDI channel (0-15)
    pub channel: u8,
    /// Pitch bend value (-8192 to 8191)
    pub value: i16,
    /// Position in PPQ
    pub position_ppq: f64,
    /// Whether this event is selected
    pub selected: bool,
}

/// A MIDI Program Change event in a take
#[derive(Clone, Debug, Facet)]
pub struct MidiProgramChange {
    /// Index of this event in the take
    pub index: u32,
    /// MIDI channel (0-15)
    pub channel: u8,
    /// Program number (0-127)
    pub program: u8,
    /// Position in PPQ
    pub position_ppq: f64,
}

/// A MIDI System Exclusive event in a take
#[derive(Clone, Debug, Facet)]
pub struct MidiSysEx {
    /// Index of this event in the take
    pub index: u32,
    /// Position in PPQ
    pub position_ppq: f64,
    /// SysEx data (including F0 and F7)
    pub data: Vec<u8>,
}

impl Default for MidiCC {
    fn default() -> Self {
        Self {
            index: 0,
            channel: 0,
            controller: 0,
            value: 0,
            position_ppq: 0.0,
            selected: false,
        }
    }
}

impl Default for MidiPitchBend {
    fn default() -> Self {
        Self {
            index: 0,
            channel: 0,
            value: 0,
            position_ppq: 0.0,
            selected: false,
        }
    }
}

impl Default for MidiProgramChange {
    fn default() -> Self {
        Self {
            index: 0,
            channel: 0,
            program: 0,
            position_ppq: 0.0,
        }
    }
}

impl Default for MidiSysEx {
    fn default() -> Self {
        Self {
            index: 0,
            position_ppq: 0.0,
            data: Vec::new(),
        }
    }
}

/// Common MIDI CC numbers
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Facet)]
pub enum CommonCC {
    /// Bank Select MSB
    BankSelectMsb = 0,
    /// Modulation Wheel
    ModWheel = 1,
    /// Breath Controller
    Breath = 2,
    /// Foot Controller
    Foot = 4,
    /// Portamento Time
    PortamentoTime = 5,
    /// Data Entry MSB
    DataEntryMsb = 6,
    /// Channel Volume
    Volume = 7,
    /// Balance
    Balance = 8,
    /// Pan
    Pan = 10,
    /// Expression
    Expression = 11,
    /// Bank Select LSB
    BankSelectLsb = 32,
    /// Sustain Pedal
    Sustain = 64,
    /// Portamento On/Off
    Portamento = 65,
    /// Sostenuto Pedal
    Sostenuto = 66,
    /// Soft Pedal
    SoftPedal = 67,
    /// Legato Footswitch
    Legato = 68,
    /// All Sound Off
    AllSoundOff = 120,
    /// Reset All Controllers
    ResetAllControllers = 121,
    /// All Notes Off
    AllNotesOff = 123,
}
