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

/// Behavior to use when moving selected tracks to a new index.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
pub enum ReorderTracksBehavior {
    /// Move selected tracks without changing folder membership.
    Normal,
    /// Make moved tracks children of the track immediately above the destination.
    MakeChildOfPreviousTrack,
    /// Extend the folder above the destination when applicable.
    ExtendFolder,
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
    /// Polarity/phase invert (REAPER `IPHASE`): flip the signal's sign.
    pub phase_inverted: bool,
    /// Track automation mode (REAPER `I_AUTOMODE`): trim/read/touch/
    /// write/latch. Governs whether control moves record envelope
    /// points during playback.
    pub automation_mode: crate::primitives::AutomationMode,
    /// Record input monitoring (REAPER `I_RECMON`).
    pub input_monitor: InputMonitoringMode,
    /// What the track records from (REAPER `I_RECINPUT`).
    ///
    /// On the track rather than behind a getter of its own, deliberately:
    /// a strip needs it, and building a strip has to stay *one* bulk read.
    /// A per-track getter would make an N-track mixer cost N extra round
    /// trips to show something every strip displays.
    pub record_input: RecordInput,
    /// Does the track send to its parent / master (REAPER `B_MAINSEND`)?
    ///
    /// Here for the same reason as `record_input`: it was a separate
    /// per-track routing call, so a mixer paid N round trips for a flag its
    /// IO indicator has to show on every strip.
    pub parent_send: bool,

    // === Structure ===
    /// GUID of the parent folder track, if any
    pub parent_guid: Option<String>,
    /// Folder depth (positive = start folder, negative = end folder levels)
    pub folder_depth: i32,
    /// Whether this track is a folder track
    pub is_folder: bool,

    // === Fixed item lanes (REAPER 7 comping) ===
    /// Number of fixed item lanes (0 = lanes disabled).
    pub lane_count: u32,
    /// Bitmask of lanes that PLAY (bit n = lane n audible). Lanes
    /// outside the mask hold alternate takes that stay silent.
    pub lane_play_mask: u64,
    /// Lane display names, one per lane (may be shorter than
    /// `lane_count`; missing entries default to 1-based numbers).
    pub lane_names: Vec<String>,
    /// How fixed lanes are displayed (REAPER's one/small/big cycle).
    pub lane_display: LaneDisplay,

    // === Grouping ===
    /// Track-grouping matrix membership (control ganging + VCA).
    pub grouping: TrackGrouping,

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
            phase_inverted: false,
            automation_mode: crate::primitives::AutomationMode::TrimRead,
            input_monitor: InputMonitoringMode::Off,
            parent_guid: None,
            folder_depth: 0,
            is_folder: false,
            lane_count: 0,
            lane_play_mask: 0,
            lane_names: Vec::new(),
            lane_display: LaneDisplay::default(),
            grouping: TrackGrouping::default(),
            visible_in_tcp: true,
            visible_in_mixer: true,
            fx_count: 0,
            input_fx_count: 0,
            record_input: RecordInput::None,
            // Sending, because that is what a new track does — a default of
            // "cut off from the master" would put a disabled badge on every
            // strip the moment anything constructed a Track without asking.
            parent_send: true,
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

/// Track-grouping matrix membership (REAPER's Track Grouping
/// Parameters). Each field is a bitmask of group numbers (bit n =
/// group n+1); groups 1–32 come from `GROUP_FLAGS`, 33–64 from
/// `GROUP_FLAGS_HIGH`, folded into one u64 per parameter.
///
/// Lead/follow gangs CONTROL GESTURES (touching a lead's control moves
/// followers' controls); VCA follow is the one that changes PLAYBACK —
/// a follower's effective gain is its fader × every shared-group VCA
/// lead's fader.
#[derive(Clone, Debug, Default, PartialEq, Eq, Facet)]
pub struct TrackGrouping {
    pub volume_lead: u64,
    pub volume_follow: u64,
    pub pan_lead: u64,
    pub pan_follow: u64,
    pub mute_lead: u64,
    pub mute_follow: u64,
    pub solo_lead: u64,
    pub solo_follow: u64,
    pub recarm_lead: u64,
    pub recarm_follow: u64,
    pub polarity_lead: u64,
    pub polarity_follow: u64,
    pub vca_lead: u64,
    pub vca_follow: u64,
    /// VCA follow applied pre-FX instead of at the fader.
    pub vca_prefx_follow: u64,
    pub media_edit_lead: u64,
    pub media_edit_follow: u64,
}

impl TrackGrouping {
    /// Whether every mask is empty (track belongs to no groups).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Fixed-lane display mode (REAPER 7's lane-button cycle:
/// show-one-lane → small lanes → big lanes).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub enum LaneDisplay {
    /// All lanes visible, compact rows.
    #[default]
    Small = 0,
    /// All lanes visible, each at full item height.
    Big = 1,
    /// Only the playing lane is shown, full height.
    One = 2,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
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
    /// Live audio input from a hardware input channel.
    ///
    /// `channel` is the 0-based input-device channel the track records /
    /// monitors from. The engine taps this channel of its open input
    /// stream and feeds it into the track's bus (and FX chain).
    Audio { channel: u32 },
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
