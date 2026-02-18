//! Track data types
//!
//! A track represents an audio or MIDI channel in the DAW mixer.

use facet::Facet;

/// Reference to a track - how to identify a track for operations
///
/// Tracks can be identified by GUID (stable across sessions), index (position-based),
/// or the special Master track designation.
#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum TrackRef {
    /// Track GUID - stable across sessions
    Guid(String),
    /// Track index (0-based position in track list)
    Index(u32),
    /// The master track
    Master,
}

impl TrackRef {
    /// Create a reference by GUID
    pub fn guid(guid: impl Into<String>) -> Self {
        Self::Guid(guid.into())
    }

    /// Create a reference by index
    pub fn index(index: u32) -> Self {
        Self::Index(index)
    }

    /// Create a reference to the master track
    pub fn master() -> Self {
        Self::Master
    }
}

impl From<u32> for TrackRef {
    fn from(index: u32) -> Self {
        Self::Index(index)
    }
}

impl From<&str> for TrackRef {
    fn from(guid: &str) -> Self {
        Self::Guid(guid.to_string())
    }
}

impl From<String> for TrackRef {
    fn from(guid: String) -> Self {
        Self::Guid(guid)
    }
}

/// Complete track state returned from queries
///
/// Contains all relevant track information including identification,
/// state flags, levels, and structural information.
#[derive(Clone, Debug, PartialEq, Facet)]
pub struct Track {
    /// Unique GUID for stable identification across sessions
    pub guid: String,
    /// Track index (0-based position in track list)
    pub index: u32,
    /// Display name of the track
    pub name: String,
    /// Color in native format (0xRRGGBB, or None for default)
    pub color: Option<u32>,

    // === State Flags ===
    /// Whether the track is muted
    pub muted: bool,
    /// Whether the track is soloed
    pub soloed: bool,
    /// Whether the track is armed for recording
    pub armed: bool,
    /// Whether the track is selected
    pub selected: bool,

    // === Levels (normalized) ===
    /// Volume level (0.0 = -inf dB, 1.0 = 0 dB)
    pub volume: f64,
    /// Pan position (-1.0 = left, 0.0 = center, 1.0 = right)
    pub pan: f64,

    // === Structure ===
    /// GUID of the parent folder track, if any
    pub parent_guid: Option<String>,
    /// Folder depth (positive = start folder, negative = end folder levels)
    pub folder_depth: i32,
    /// Whether this track is a folder track
    pub is_folder: bool,

    // === Visibility ===
    /// Whether the track is visible in the TCP (track control panel / arrange view)
    pub visible_in_tcp: bool,
    /// Whether the track is visible in the MCP (mixer control panel)
    pub visible_in_mixer: bool,

    // === FX Info ===
    /// Number of FX in the main FX chain
    pub fx_count: u32,
    /// Number of FX in the input/recording FX chain
    pub input_fx_count: u32,
}

impl Track {
    /// Create a new track with default values
    pub fn new(guid: String, index: u32, name: String) -> Self {
        Self {
            guid,
            index,
            name,
            color: None,
            muted: false,
            soloed: false,
            armed: false,
            selected: false,
            volume: 1.0,
            pan: 0.0,
            parent_guid: None,
            folder_depth: 0,
            is_folder: false,
            visible_in_tcp: true,
            visible_in_mixer: true,
            fx_count: 0,
            input_fx_count: 0,
        }
    }

    /// Check if this is the master track (index 0 with special characteristics)
    pub fn is_master(&self) -> bool {
        // Master track typically has no parent and special naming
        self.parent_guid.is_none() && self.name.to_lowercase().contains("master")
    }

    /// Get a TrackRef for this track by GUID
    pub fn as_ref(&self) -> TrackRef {
        TrackRef::Guid(self.guid.clone())
    }

    /// Get a TrackRef for this track by index
    pub fn as_index_ref(&self) -> TrackRef {
        TrackRef::Index(self.index)
    }
}

impl Default for Track {
    fn default() -> Self {
        Self::new(String::new(), 0, String::new())
    }
}

/// Input monitoring mode for a track.
///
/// Controls whether the track's input signal is monitored (passed through
/// to FX and output) during recording/playback.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub enum InputMonitoringMode {
    /// No input monitoring.
    #[default]
    Off,
    /// Always monitor input (even when not playing/recording).
    Normal,
    /// Only monitor when not playing (tape-style auto-monitoring).
    NotWhenPlaying,
}

/// Record input source for a track.
///
/// Simplified representation of REAPER's I_RECINPUT encoding.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Facet)]
pub enum RecordInput {
    /// No input (-1).
    None,
    /// MIDI input from a specific device on a specific channel.
    /// device_id: None = all devices. channel: None = all channels.
    /// device_id 62 = Virtual MIDI Keyboard (VKB).
    Midi {
        device_id: Option<u8>,
        channel: Option<u8>,
    },
    /// Raw I_RECINPUT value for other input types.
    Raw(i32),
}

impl RecordInput {
    /// MIDI from the Virtual MIDI Keyboard on all channels.
    ///
    /// This is the input source needed for `StuffMIDIMessage` with
    /// `VirtualMidiKeyboard` target to reach the track's FX chain.
    pub fn midi_virtual_keyboard() -> Self {
        Self::Midi {
            device_id: Some(62),
            channel: None,
        }
    }

    /// MIDI from all devices on all channels.
    pub fn midi_all() -> Self {
        Self::Midi {
            device_id: None,
            channel: None,
        }
    }
}
