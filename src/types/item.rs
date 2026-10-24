//! Media item data structures and parsing for REAPER

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::primitives::Token;

/// Fade curve types for items
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum FadeCurveType {
    Linear = 0,
    Square = 1,
    SlowStartEnd = 2,
    FastStart = 3,
    FastEnd = 4,
    Bezier = 5,
    Unknown(i32),
}

impl From<i32> for FadeCurveType {
    fn from(value: i32) -> Self {
        match value {
            0 => FadeCurveType::Linear,
            1 => FadeCurveType::Square,
            2 => FadeCurveType::SlowStartEnd,
            3 => FadeCurveType::FastStart,
            4 => FadeCurveType::FastEnd,
            5 => FadeCurveType::Bezier,
            _ => FadeCurveType::Unknown(value),
        }
    }
}

impl fmt::Display for FadeCurveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FadeCurveType::Linear => write!(f, "Linear"),
            FadeCurveType::Square => write!(f, "Square"),
            FadeCurveType::SlowStartEnd => write!(f, "Slow Start/End"),
            FadeCurveType::FastStart => write!(f, "Fast Start"),
            FadeCurveType::FastEnd => write!(f, "Fast End"),
            FadeCurveType::Bezier => write!(f, "Bezier"),
            FadeCurveType::Unknown(val) => write!(f, "Unknown({})", val),
        }
    }
}

/// Channel mode for takes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum ChannelMode {
    Normal = 0,
    ReverseStereo = 1,
    MonoDownmix = 2,
    MonoLeft = 3,
    MonoRight = 4,
    MonoChannel(u8),  // 5-194 for mono channels 3-128
    StereChannel(u8), // 67-257 for stereo channels 1-128
    Unknown(i32),
}

impl From<i32> for ChannelMode {
    fn from(value: i32) -> Self {
        match value {
            0 => ChannelMode::Normal,
            1 => ChannelMode::ReverseStereo,
            2 => ChannelMode::MonoDownmix,
            3 => ChannelMode::MonoLeft,
            4 => ChannelMode::MonoRight,
            5..=66 => ChannelMode::MonoChannel((value - 2) as u8), // 5-66 -> 3-64
            67..=130 => ChannelMode::StereChannel((value - 66) as u8), // 67-130 -> 1-64
            131..=194 => ChannelMode::MonoChannel((value - 66) as u8), // 131-194 -> 65-128
            195..=257 => ChannelMode::StereChannel((value - 128) as u8), // 195-257 -> 67-128
            _ => ChannelMode::Unknown(value),
        }
    }
}

impl fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelMode::Normal => write!(f, "Normal"),
            ChannelMode::ReverseStereo => write!(f, "Reverse Stereo"),
            ChannelMode::MonoDownmix => write!(f, "Mono (Downmix)"),
            ChannelMode::MonoLeft => write!(f, "Mono (Left)"),
            ChannelMode::MonoRight => write!(f, "Mono (Right)"),
            ChannelMode::MonoChannel(ch) => write!(f, "Mono (Channel {})", ch),
            ChannelMode::StereChannel(ch) => write!(f, "Stereo (Channels {}/{})", ch, ch + 1),
            ChannelMode::Unknown(val) => write!(f, "Unknown({})", val),
        }
    }
}

/// Pitch shifting and time stretch modes for PLAYRATE field 4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum PitchMode {
    ProjectDefault = -1,
    SoundTouchPreset1 = 0,
    SoundTouchPreset2 = 1,
    SoundTouchPreset3 = 2,
    DiracLE(u8),              // 0-31 presets (65536-65567)
    LowQualityWindowed(u8),   // 0-47 presets (131072-131119)
    ElastiquePro(u8),         // 0-31 presets (196608-196639)
    ElastiqueEfficient(u8),   // 0-3 presets (262144-262147)
    ElastiqueSoloist(u8),     // 0-3 presets (327680-327683)
    Elastique21Pro(u8),       // 0-31 presets (393216-393247)
    Elastique21Efficient(u8), // 0-3 presets (458752-458755)
    Elastique21Soloist(u8),   // 0-3 presets (524288-524291)
    Unknown(i32),
}

impl From<i32> for PitchMode {
    fn from(value: i32) -> Self {
        match value {
            -1 => PitchMode::ProjectDefault,
            0 => PitchMode::SoundTouchPreset1,
            1 => PitchMode::SoundTouchPreset2,
            2 => PitchMode::SoundTouchPreset3,
            65536..=65567 => PitchMode::DiracLE((value - 65536) as u8),
            131072..=131119 => PitchMode::LowQualityWindowed((value - 131072) as u8),
            196608..=196639 => PitchMode::ElastiquePro((value - 196608) as u8),
            262144..=262147 => PitchMode::ElastiqueEfficient((value - 262144) as u8),
            327680..=327683 => PitchMode::ElastiqueSoloist((value - 327680) as u8),
            393216..=393247 => PitchMode::Elastique21Pro((value - 393216) as u8),
            458752..=458755 => PitchMode::Elastique21Efficient((value - 458752) as u8),
            524288..=524291 => PitchMode::Elastique21Soloist((value - 524288) as u8),
            _ => PitchMode::Unknown(value),
        }
    }
}

impl fmt::Display for PitchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PitchMode::ProjectDefault => write!(f, "Project Default"),
            PitchMode::SoundTouchPreset1 => write!(f, "Sound Touch (Preset 1)"),
            PitchMode::SoundTouchPreset2 => write!(f, "Sound Touch (Preset 2)"),
            PitchMode::SoundTouchPreset3 => write!(f, "Sound Touch (Preset 3)"),
            PitchMode::DiracLE(preset) => write!(f, "Dirac LE (Preset {})", preset + 1),
            PitchMode::LowQualityWindowed(preset) => {
                write!(f, "Low Quality Windowed (Preset {})", preset + 1)
            }
            PitchMode::ElastiquePro(preset) => write!(f, "élastique Pro (Preset {})", preset + 1),
            PitchMode::ElastiqueEfficient(preset) => {
                write!(f, "élastique Efficient (Preset {})", preset + 1)
            }
            PitchMode::ElastiqueSoloist(preset) => {
                write!(f, "élastique SOLOIST (Preset {})", preset + 1)
            }
            PitchMode::Elastique21Pro(preset) => {
                write!(f, "élastique 2.1 Pro (Preset {})", preset + 1)
            }
            PitchMode::Elastique21Efficient(preset) => {
                write!(f, "élastique 2.1 Efficient (Preset {})", preset + 1)
            }
            PitchMode::Elastique21Soloist(preset) => {
                write!(f, "élastique 2.1 SOLOIST (Preset {})", preset + 1)
            }
            PitchMode::Unknown(val) => write!(f, "Unknown({})", val),
        }
    }
}

/// Solo states for MUTE field 2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum SoloState {
    NotSoloed = 0,
    Soloed = -1,
    SoloOverridden = 1,
    Unknown(i32),
}

impl From<i32> for SoloState {
    fn from(value: i32) -> Self {
        match value {
            0 => SoloState::NotSoloed,
            -1 => SoloState::Soloed,
            1 => SoloState::SoloOverridden,
            _ => SoloState::Unknown(value),
        }
    }
}

impl fmt::Display for SoloState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SoloState::NotSoloed => write!(f, "Not Soloed"),
            SoloState::Soloed => write!(f, "Soloed"),
            SoloState::SoloOverridden => write!(f, "Solo Overridden"),
            SoloState::Unknown(val) => write!(f, "Unknown({})", val),
        }
    }
}

/// Source types for SOURCE blocks
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    Wave,
    Mp3,
    Midi,
    Video,
    Section,
    Empty,
    Flac,
    Vorbis,
    OfflineWave,
    Unknown(String),
}

impl From<&str> for SourceType {
    fn from(value: &str) -> Self {
        match value.to_uppercase().as_str() {
            "WAVE" => SourceType::Wave,
            "MP3" => SourceType::Mp3,
            "MIDI" => SourceType::Midi,
            "VIDEO" => SourceType::Video,
            "SECTION" => SourceType::Section,
            "EMPTY" => SourceType::Empty,
            "FLAC" => SourceType::Flac,
            "VORBIS" => SourceType::Vorbis,
            "_OFFLINE_WAVE" => SourceType::OfflineWave,
            _ => SourceType::Unknown(value.to_string()),
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceType::Wave => write!(f, "Wave"),
            SourceType::Mp3 => write!(f, "MP3"),
            SourceType::Midi => write!(f, "MIDI"),
            SourceType::Video => write!(f, "Video"),
            SourceType::Section => write!(f, "Section"),
            SourceType::Empty => write!(f, "Empty"),
            SourceType::Flac => write!(f, "FLAC"),
            SourceType::Vorbis => write!(f, "Vorbis"),
            SourceType::OfflineWave => write!(f, "Offline Wave"),
            SourceType::Unknown(val) => write!(f, "Unknown({})", val),
        }
    }
}

/// Item timebase for BEAT field
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum ItemTimebase {
    ProjectDefault = -1,
    Time = 0,
    Beats = 1,
    Unknown(i32),
}

impl From<i32> for ItemTimebase {
    fn from(value: i32) -> Self {
        match value {
            -1 => ItemTimebase::ProjectDefault,
            0 => ItemTimebase::Time,
            1 => ItemTimebase::Beats,
            _ => ItemTimebase::Unknown(value),
        }
    }
}

impl fmt::Display for ItemTimebase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemTimebase::ProjectDefault => write!(f, "Project Default"),
            ItemTimebase::Time => write!(f, "Time"),
            ItemTimebase::Beats => write!(f, "Beats"),
            ItemTimebase::Unknown(val) => write!(f, "Unknown({})", val),
        }
    }
}

/// A REAPER media item
/// Item vertical geometry within its track (`YPOS`): fractions of the
/// track height. REAPER writes this for free item positioning and for
/// fixed-lane membership (lane = `y / height` when lanes are uniform).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ItemYPos {
    pub y: f64,
    pub height: f64,
    pub mode: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    // Basic item properties
    pub position: f64,              // POSITION - Position on timeline in seconds
    pub snap_offset: f64,           // SNAPOFFS - Snap offset in seconds
    pub length: f64,                // LENGTH - Item length in seconds
    pub loop_source: bool,          // LOOP - Loop source flag
    pub play_all_takes: bool,       // ALLTAKES - Play all takes flag
    pub color: Option<i32>,         // COLOR - Item color (optional)
    pub beat: Option<ItemTimebase>, // BEAT - Item timebase (optional)
    pub selected: bool,             // SEL - Is item selected

    // Fade settings
    pub fade_in: Option<FadeSettings>,  // FADEIN - Fade in settings
    pub fade_out: Option<FadeSettings>, // FADEOUT - Fade out settings

    // Mute/Solo settings
    pub mute: Option<MuteSettings>, // MUTE - Mute and solo settings

    // Item identification
    pub item_guid: Option<String>, // IGUID - Item GUID
    pub item_id: Option<i32>,      // IID - Item ordinal number (deprecated)

    // Item properties
    pub name: String,                       // NAME - Item name
    pub volpan: Option<VolPanSettings>,     // VOLPAN - Volume and pan settings
    pub slip_offset: f64,                   // SOFFS - Slip offset in seconds
    pub playrate: Option<PlayRateSettings>, // PLAYRATE - Play rate settings
    pub channel_mode: ChannelMode,          // CHANMODE - Channel mode
    pub take_guid: Option<String>,          // GUID - Take GUID
    pub rec_pass: Option<i32>,              // RECPASS - Recording pass number
    /// Fixed item lane index (`LANE`, or derived from `YPOS` on
    /// fixed-lanes tracks), REAPER 7+ comping. `None` for ordinary items.
    pub lane: Option<i32>,
    /// Raw vertical geometry (`YPOS y height [mode]`) — free item
    /// positioning and fixed-lane placement.
    pub y_pos: Option<ItemYPos>,

    // Takes
    pub takes: Vec<Take>,

    // Stretch markers
    pub stretch_markers: Vec<StretchMarker>,

    // Raw content for preservation
    pub raw_content: String,
}

impl Default for Item {
    fn default() -> Self {
        Self {
            position: 0.0,
            snap_offset: 0.0,
            length: 0.0,
            loop_source: false,
            play_all_takes: false,
            color: None,
            beat: None,
            selected: false,
            fade_in: None,
            fade_out: None,
            mute: None,
            item_guid: None,
            item_id: None,
            name: String::new(),
            volpan: None,
            slip_offset: 0.0,
            playrate: None,
            channel_mode: ChannelMode::Normal,
            take_guid: None,
            rec_pass: None,
            lane: None,
            y_pos: None,
            takes: vec![],
            stretch_markers: vec![],
            raw_content: String::new(),
        }
    }
}

/// Fade settings for an item
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FadeSettings {
    pub curve_type: FadeCurveType, // field 1 - fade curve type
    pub time: f64,                 // field 2 - fade time in seconds
    pub unknown_field_3: f64,      // field 3 - unknown
    pub unknown_field_4: i32,      // field 4 - unknown
    pub unknown_field_5: i32,      // field 5 - unknown
    pub unknown_field_6: i32,      // field 6 - unknown
    pub unknown_field_7: i32,      // field 7 - unknown
}

/// Mute and solo settings for an item
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MuteSettings {
    pub muted: bool,           // field 1 - item is muted
    pub solo_state: SoloState, // field 2 - solo state (-1, 0, 1)
}

/// Volume and pan settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolPanSettings {
    pub item_trim: f64,    // field 1 - item trim (1.0 = 0 dB)
    pub take_pan: f64,     // field 2 - take pan (-1.0 to 1.0)
    pub take_volume: f64,  // field 3 - take volume
    pub take_pan_law: f64, // field 4 - take pan law
}

/// Play rate settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayRateSettings {
    pub rate: f64,             // field 1 - play rate
    pub preserve_pitch: bool,  // field 2 - preserve pitch while changing rate
    pub pitch_adjust: f64,     // field 3 - pitch adjust in semitones.cents
    pub pitch_mode: PitchMode, // field 4 - pitch shifting/time stretch mode
    pub unknown_field_5: i32,  // field 5 - unknown
    pub unknown_field_6: f64,  // field 6 - unknown
}

/// A take within a media item
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub is_selected: bool,                  // TAKE SEL - Is this take selected
    pub name: String,                       // NAME - Take name
    pub volpan: Option<VolPanSettings>,     // TAKEVOLPAN - Take volume and pan
    pub slip_offset: f64,                   // SOFFS - Take slip offset
    pub playrate: Option<PlayRateSettings>, // PLAYRATE - Take play rate
    pub channel_mode: ChannelMode,          // CHANMODE - Take channel mode
    pub take_color: Option<i32>,            // TAKECOLOR - Take color
    pub take_guid: Option<String>,          // GUID - Take GUID
    pub rec_pass: Option<i32>,              // RECPASS - Recording pass number
    pub source: Option<SourceBlock>,        // SOURCE block
    /// `SM` lines read under this take. REAPER keys stretch markers per
    /// take; the first take's markers are also mirrored on
    /// `Item::stretch_markers` for callers that predate this field.
    pub stretch_markers: Vec<StretchMarker>,
    /// Nested blocks under this take that are not `<SOURCE>` — chiefly
    /// `<EXT>` (REAPER's per-take extension store, e.g.
    /// `ORIGINAL_FILENAME`). Kept verbatim so a round trip does not drop
    /// them; the serializer re-indents and writes them back after the
    /// source block.
    pub extra_blocks: Vec<String>,
}

impl Default for Take {
    fn default() -> Self {
        Self {
            is_selected: false,
            name: String::new(),
            volpan: None,
            slip_offset: 0.0,
            playrate: None,
            channel_mode: ChannelMode::Normal,
            take_color: None,
            take_guid: None,
            rec_pass: None,
            source: None,
            stretch_markers: Vec::new(),
            extra_blocks: Vec::new(),
        }
    }
}

/// Source block for a take
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceBlock {
    pub source_type: SourceType,       // WAVE, MIDI, etc.
    pub file_path: String,             // FILE - Source file path
    pub midi_data: Option<MidiSource>, // Parsed MIDI data (if source is MIDI)
    pub raw_content: String,           // Raw content for preservation
}

/// Parsed MIDI source data from a `<SOURCE MIDI>` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiSource {
    /// Whether this source reports any data (`HASDATA` field 1)
    pub has_data: bool,
    /// Ticks per quarter note (from HASDATA line, e.g. 960)
    pub ticks_per_qn: u32,
    /// Timebase label from HASDATA (typically `QN`)
    pub ticks_timebase: Option<String>,
    /// CC interpolation mode (`CCINTERP`)
    pub cc_interp: Option<i32>,
    /// Pooled events GUID (from POOLEDEVTS line)
    pub pooled_evts_guid: Option<String>,
    /// MIDI events
    pub events: Vec<MidiEvent>,
    /// Extended `X/x` event blocks (base64 payload blocks)
    pub extended_events: Vec<MidiExtendedEvent>,
    /// MIDI source events in original file order, preserving timing deltas from both `E` and `<X>` lines.
    pub event_stream: Vec<MidiSourceEvent>,
    /// Ignore project tempo override (`IGNTEMPO`)
    pub ignore_tempo: Option<MidiIgnoreTempo>,
    /// MIDI editor velocity/CC lanes (`VELLANE`)
    pub vel_lanes: Vec<MidiVelLane>,
    /// Optional ReaBank path (`BANKPROGRAMFILE`)
    pub bank_program_file: Option<String>,
    /// Raw CFGEDITVIEW fields (`CFGEDITVIEW`)
    pub cfg_edit_view: Option<Vec<String>>,
    /// Raw CFGEDIT fields (`CFGEDIT`)
    pub cfg_edit: Option<Vec<String>>,
    /// Raw EVTFILTER fields (`EVTFILTER`)
    pub evt_filter: Option<Vec<String>>,
    /// Source GUID
    pub guid: Option<String>,
    /// Unrecognized lines preserved for forward-compatibility.
    pub unknown_lines: Vec<String>,
}

/// A single MIDI event from an RPP `E` line.
///
/// Format: `E <delta_ticks> <status_hex> <data1_hex> [data2_hex] [...]`
/// The status byte determines the event type:
/// - `8x` = Note Off, `9x` = Note On, `Ax` = Aftertouch
/// - `Bx` = Control Change, `Cx` = Program Change, `Dx` = Channel Pressure
/// - `Ex` = Pitch Bend, `Fx` = System (SysEx, etc.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiEvent {
    /// Delta time in ticks from previous event
    pub delta_ticks: u32,
    /// Raw MIDI bytes (status + data, parsed from hex)
    pub bytes: Vec<u8>,
}

/// Extended MIDI event block (`<X ...>` / `<x ...>`) with base64 payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiExtendedEvent {
    /// `true` if lowercase `<x ...>` (selected), `false` for uppercase `<X ...>`.
    pub selected: bool,
    /// Header fields following `X/x`.
    pub fields: Vec<String>,
    /// Base64 payload lines contained by this block.
    pub data_lines: Vec<String>,
}

/// A MIDI source event preserving the original interleaving of standard `E` and extended `<X>` events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MidiSourceEvent {
    Midi(MidiEvent),
    Extended(MidiExtendedEvent),
}

/// `IGNTEMPO` override for MIDI sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiIgnoreTempo {
    pub enabled: bool,
    pub bpm: f64,
    pub numerator: i32,
    pub denominator: i32,
}

/// `VELLANE` row from a MIDI source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiVelLane {
    pub lane_type: i32,
    pub midi_editor_height: i32,
    pub inline_editor_height: i32,
}

impl MidiEvent {
    /// Get the MIDI status byte (first byte).
    pub fn status(&self) -> u8 {
        self.bytes.first().copied().unwrap_or(0)
    }

    /// Get the event type (upper nibble of status byte).
    pub fn event_type(&self) -> MidiEventType {
        match self.status() >> 4 {
            0x8 => MidiEventType::NoteOff,
            0x9 => {
                // Note On with velocity 0 is actually Note Off
                if self.bytes.get(2).copied().unwrap_or(0) == 0 {
                    MidiEventType::NoteOff
                } else {
                    MidiEventType::NoteOn
                }
            }
            0xA => MidiEventType::Aftertouch,
            0xB => MidiEventType::ControlChange,
            0xC => MidiEventType::ProgramChange,
            0xD => MidiEventType::ChannelPressure,
            0xE => MidiEventType::PitchBend,
            0xF => MidiEventType::System,
            _ => MidiEventType::Unknown,
        }
    }

    /// Get the MIDI channel (0-15, from lower nibble of status byte).
    pub fn channel(&self) -> u8 {
        self.status() & 0x0F
    }
}

impl MidiExtendedEvent {
    /// Delta time in ticks from the previous source event.
    pub fn delta_ticks(&self) -> u32 {
        self.fields
            .first()
            .and_then(|field| field.parse::<u32>().ok())
            .unwrap_or(0)
    }
}

impl MidiSourceEvent {
    /// Delta time in ticks from the previous source event.
    pub fn delta_ticks(&self) -> u32 {
        match self {
            Self::Midi(event) => event.delta_ticks,
            Self::Extended(event) => event.delta_ticks(),
        }
    }
}

/// MIDI event type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiEventType {
    NoteOn,
    NoteOff,
    Aftertouch,
    ControlChange,
    ProgramChange,
    ChannelPressure,
    PitchBend,
    System,
    Unknown,
}

/// A stretch marker within a media item.
///
/// From `SM` lines: `SM <position> <source_position> [<rate>]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StretchMarker {
    /// Position within the item (in seconds)
    pub position: f64,
    /// Source position (in seconds)
    pub source_position: f64,
    /// Stretch rate at this marker (optional)
    pub rate: Option<f64>,
}

/// Whether an accumulated take holds anything — a source, a name or a GUID.
///
/// Distinguishes "the item began with an inline take" from "the parser had an
/// empty accumulator when the first `TAKE` marker arrived".
fn take_has_content(take: &Take) -> bool {
    take.source.is_some() || !take.name.is_empty() || take.take_guid.is_some()
}

impl Item {
    fn should_parse_item_raw_line(raw: &str) -> bool {
        let trimmed = raw.trim_start();
        let Some(first) = trimmed.split_whitespace().next() else {
            return false;
        };
        matches!(
            first,
            "POSITION"
                | "SNAPOFFS"
                | "LENGTH"
                | "LOOP"
                | "ALLTAKES"
                | "COLOR"
                | "BEAT"
                | "SEL"
                | "FADEIN"
                | "FADEOUT"
                | "MUTE"
                | "IGUID"
                | "IID"
                | "NAME"
                | "VOLPAN"
                | "SOFFS"
                | "PLAYRATE"
                | "CHANMODE"
                | "GUID"
                | "RECPASS"
                | "LANE"
                | "YPOS"
                | "TAKE"
                | "TAKEVOLPAN"
                | "TAKECOLOR"
                | "SM"
        )
    }

    fn fast_classify_item_token(raw: &str) -> Token {
        if let Some(hex) = raw.strip_prefix("0x")
            && let Ok(v) = u64::from_str_radix(hex, 16)
        {
            return Token::HexInteger(v);
        }

        if let Ok(v) = raw.parse::<i64>() {
            return Token::Integer(v);
        }
        if let Ok(v) = raw.parse::<f64>() {
            return Token::Float(v);
        }
        if raw.contains(',') {
            let normalized = raw.replace(',', ".");
            if let Ok(v) = normalized.parse::<f64>() {
                return Token::Float(v);
            }
        }
        Token::Identifier(raw.to_string())
    }

    fn parse_item_token_line(line: &str) -> Result<Vec<Token>, String> {
        if line.contains('"')
            || line.contains('\'')
            || line.contains('`')
            || line.contains('#')
            || line.contains(';')
        {
            return crate::primitives::token::parse_token_line(line)
                .map(|(_, tokens)| tokens)
                .map_err(|e| format!("{e:?}"));
        }

        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(8);
        out.push(Self::fast_classify_item_token(first));
        out.extend(parts.map(Self::fast_classify_item_token));
        Ok(out)
    }

    fn default_take() -> Take {
        Take {
            is_selected: false,
            name: String::new(),
            volpan: None,
            slip_offset: 0.0,
            playrate: None,
            channel_mode: ChannelMode::Normal,
            take_color: None,
            take_guid: None,
            rec_pass: None,
            source: None,
            stretch_markers: Vec::new(),
            extra_blocks: Vec::new(),
        }
    }

    fn apply_item_tokens(
        item: &mut Item,
        tokens: &[Token],
        current_take: &mut Option<Take>,
        in_take_context: &mut bool,
    ) -> Result<(), String> {
        if tokens.is_empty() {
            return Ok(());
        }
        let identifier = match &tokens[0] {
            Token::Identifier(id) => id.as_str(),
            _ => return Ok(()),
        };

        match identifier {
            "POSITION" if tokens.len() > 1 => {
                item.position = Self::parse_float(&tokens[1])?;
            }
            "SNAPOFFS" if tokens.len() > 1 => {
                item.snap_offset = Self::parse_float(&tokens[1])?;
            }
            "LENGTH" if tokens.len() > 1 => {
                item.length = Self::parse_float(&tokens[1])?;
            }
            "LOOP" if tokens.len() > 1 => {
                item.loop_source = Self::parse_bool(&tokens[1])?;
            }
            "ALLTAKES" if tokens.len() > 1 => {
                item.play_all_takes = Self::parse_bool(&tokens[1])?;
            }
            "COLOR" if tokens.len() > 1 => {
                item.color = Some(Self::parse_int(&tokens[1])?);
            }
            "BEAT" if tokens.len() > 1 => {
                item.beat = Some(ItemTimebase::from(Self::parse_int(&tokens[1])?));
            }
            "SEL" if tokens.len() > 1 => {
                item.selected = Self::parse_bool(&tokens[1])?;
            }
            "FADEIN" if tokens.len() >= 3 => {
                item.fade_in = Some(FadeSettings {
                    curve_type: FadeCurveType::from(Self::parse_int(&tokens[1])?),
                    time: Self::parse_float(&tokens[2])?,
                    unknown_field_3: if tokens.len() > 3 {
                        Self::parse_float(&tokens[3])?
                    } else {
                        0.0
                    },
                    unknown_field_4: if tokens.len() > 4 {
                        Self::parse_int(&tokens[4])?
                    } else {
                        0
                    },
                    unknown_field_5: if tokens.len() > 5 {
                        Self::parse_int(&tokens[5])?
                    } else {
                        0
                    },
                    unknown_field_6: if tokens.len() > 6 {
                        Self::parse_int(&tokens[6])?
                    } else {
                        0
                    },
                    unknown_field_7: if tokens.len() > 7 {
                        Self::parse_int(&tokens[7])?
                    } else {
                        0
                    },
                });
            }
            "FADEOUT" if tokens.len() >= 3 => {
                item.fade_out = Some(FadeSettings {
                    curve_type: FadeCurveType::from(Self::parse_int(&tokens[1])?),
                    time: Self::parse_float(&tokens[2])?,
                    unknown_field_3: if tokens.len() > 3 {
                        Self::parse_float(&tokens[3])?
                    } else {
                        0.0
                    },
                    unknown_field_4: if tokens.len() > 4 {
                        Self::parse_int(&tokens[4])?
                    } else {
                        0
                    },
                    unknown_field_5: if tokens.len() > 5 {
                        Self::parse_int(&tokens[5])?
                    } else {
                        0
                    },
                    unknown_field_6: if tokens.len() > 6 {
                        Self::parse_int(&tokens[6])?
                    } else {
                        0
                    },
                    unknown_field_7: if tokens.len() > 7 {
                        Self::parse_int(&tokens[7])?
                    } else {
                        0
                    },
                });
            }
            "MUTE" if tokens.len() >= 3 => {
                item.mute = Some(MuteSettings {
                    muted: Self::parse_bool(&tokens[1])?,
                    solo_state: SoloState::from(Self::parse_int(&tokens[2])?),
                });
            }
            "IGUID" if tokens.len() > 1 => {
                item.item_guid = Some(Self::parse_string(&tokens[1])?);
            }
            "IID" if tokens.len() > 1 => {
                item.item_id = Some(Self::parse_int(&tokens[1])?);
            }
            // REAPER writes the FIRST take's attributes at item level (the
            // implicit take before any `TAKE` statement); later takes carry
            // theirs after their `TAKE` line. These arms therefore route to
            // the current take always, and mirror onto the item fields
            // outside take context (compatibility with existing readers).
            "NAME" if tokens.len() > 1 => {
                let name = Self::parse_string(&tokens[1])?;
                if !*in_take_context {
                    item.name = name.clone();
                }
                if let Some(take) = current_take {
                    take.name = name;
                }
            }
            "VOLPAN" if tokens.len() >= 5 => {
                let vp = VolPanSettings {
                    item_trim: Self::parse_float(&tokens[1])?,
                    take_pan: Self::parse_float(&tokens[2])?,
                    take_volume: Self::parse_float(&tokens[3])?,
                    take_pan_law: Self::parse_float(&tokens[4])?,
                };
                if !*in_take_context {
                    item.volpan = Some(vp.clone());
                }
                if let Some(take) = current_take {
                    take.volpan = Some(vp);
                }
            }
            "SOFFS" if tokens.len() > 1 => {
                let v = Self::parse_float(&tokens[1])?;
                if !*in_take_context {
                    item.slip_offset = v;
                }
                if let Some(take) = current_take {
                    take.slip_offset = v;
                }
            }
            "PLAYRATE" if tokens.len() >= 4 => {
                let pr = PlayRateSettings {
                    rate: Self::parse_float(&tokens[1])?,
                    preserve_pitch: Self::parse_bool(&tokens[2])?,
                    pitch_adjust: Self::parse_float(&tokens[3])?,
                    pitch_mode: PitchMode::from(Self::parse_int(&tokens[4])?),
                    unknown_field_5: if tokens.len() > 5 {
                        Self::parse_int(&tokens[5])?
                    } else {
                        0
                    },
                    unknown_field_6: if tokens.len() > 6 {
                        Self::parse_float(&tokens[6])?
                    } else {
                        0.0
                    },
                };
                if !*in_take_context {
                    item.playrate = Some(pr.clone());
                }
                if let Some(take) = current_take {
                    take.playrate = Some(pr);
                }
            }
            "CHANMODE" if tokens.len() > 1 => {
                let cm = ChannelMode::from(Self::parse_int(&tokens[1])?);
                if !*in_take_context {
                    item.channel_mode = cm;
                }
                if let Some(take) = current_take {
                    take.channel_mode = cm;
                }
            }
            "GUID" if tokens.len() > 1 => {
                let guid = Self::parse_string(&tokens[1])?;
                if *in_take_context {
                    if let Some(take) = current_take {
                        take.take_guid = Some(guid);
                    }
                } else {
                    item.take_guid = Some(guid.clone());
                    if let Some(take) = current_take {
                        take.take_guid = Some(guid);
                    }
                }
            }
            "LANE" if tokens.len() >= 2 => {
                item.lane = Some(Self::parse_int(&tokens[1])?);
            }
            "YPOS" if tokens.len() >= 3 => {
                // Free-positioning / fixed-lane geometry:
                // `YPOS <y> <height> [mode]`, both 0..1 fractions of
                // the track height. On a fixed-lanes track each lane
                // is `1/lane_count` tall, so `lane = round(y/height)`.
                let y = Self::parse_float(&tokens[1])?;
                let height = Self::parse_float(&tokens[2])?;
                item.y_pos = Some(ItemYPos {
                    y,
                    height,
                    mode: tokens.get(3).map(Self::parse_int).transpose()?.unwrap_or(0),
                });
                // Derive the fixed-lane index when not explicitly set
                // via a LANE line.
                if item.lane.is_none() && height > 1e-9 {
                    item.lane = Some((y / height).round() as i32);
                }
            }
            "RECPASS" if tokens.len() > 1 => {
                let rec_pass = Self::parse_int(&tokens[1])?;
                if *in_take_context {
                    if let Some(take) = current_take {
                        take.rec_pass = Some(rec_pass);
                    }
                } else {
                    item.rec_pass = Some(rec_pass);
                }
            }
            "TAKE" => {
                // `TAKE NULL` is an empty comp-lane slot. It is still a take
                // as far as REAPER is concerned — it occupies an index — so
                // it is kept, not skipped: dropping it would silently
                // re-number every take on a comped item.
                let is_selected =
                    matches!(tokens.get(1), Some(Token::Identifier(flag)) if flag == "SEL");
                if let Some(take) = current_take.take() {
                    // An item whose FIRST take is written inline has content
                    // before any `TAKE` marker, and that content is take 0.
                    // A comped item has none — it opens straight with
                    // `TAKE NULL` — and pushing the empty accumulator there
                    // invented a take 0 that REAPER does not count, shifting
                    // every later index by one. Only real inline content
                    // becomes take 0.
                    if *in_take_context || take_has_content(&take) {
                        item.takes.push(take);
                    }
                }
                *current_take = Some(Take {
                    is_selected,
                    ..Self::default_take()
                });
                *in_take_context = true;
            }
            "TAKEVOLPAN" => {
                if let Some(take) = current_take
                    && tokens.len() >= 4
                {
                    take.volpan = Some(VolPanSettings {
                        item_trim: 0.0,
                        take_pan: Self::parse_float(&tokens[1])?,
                        take_volume: Self::parse_float(&tokens[2])?,
                        take_pan_law: Self::parse_float(&tokens[3])?,
                    });
                }
            }
            "TAKECOLOR" => {
                if let Some(take) = current_take
                    && tokens.len() > 1
                {
                    take.take_color = Some(Self::parse_int(&tokens[1])?);
                }
            }
            "SM" if tokens.len() >= 3 => {
                let sm = StretchMarker {
                    position: Self::parse_float(&tokens[1])?,
                    source_position: Self::parse_float(&tokens[2])?,
                    rate: if tokens.len() > 3 {
                        Some(Self::parse_float(&tokens[3])?)
                    } else {
                        None
                    },
                };
                // Markers belong to the take they were read under. Before
                // the first `TAKE` line that is the implicit first take,
                // whose markers are mirrored on the item for compatibility.
                if !*in_take_context {
                    item.stretch_markers.push(sm.clone());
                }
                if let Some(take) = current_take {
                    take.stretch_markers.push(sm);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Create an Item from a parsed RPP block (legacy method for compatibility)
    // r[impl rpp.parse.item]
    pub fn from_block(block: &crate::primitives::RppBlock) -> Result<Self, String> {
        let mut item = Item {
            position: 0.0,
            snap_offset: 0.0,
            length: 0.0,
            loop_source: false,
            play_all_takes: false,
            color: None,
            beat: None,
            selected: false,
            fade_in: None,
            fade_out: None,
            mute: None,
            item_guid: None,
            item_id: None,
            name: String::new(),
            volpan: None,
            slip_offset: 0.0,
            playrate: None,
            channel_mode: ChannelMode::Normal,
            take_guid: None,
            rec_pass: None,
            lane: None,
            y_pos: None,
            takes: Vec::new(),
            stretch_markers: Vec::new(),
            raw_content: String::new(),
        };
        let mut current_take: Option<Take> = Some(Self::default_take());
        let mut in_take_context = false;

        for child in &block.children {
            match child {
                crate::primitives::RppBlockContent::Content(tokens) => {
                    let reparsed;
                    let tokens = if let [Token::Identifier(raw)] = tokens.as_slice() {
                        if raw.contains(' ') && Self::should_parse_item_raw_line(raw) {
                            reparsed = Self::parse_item_token_line(raw)?;
                            reparsed.as_slice()
                        } else {
                            tokens.as_slice()
                        }
                    } else {
                        tokens.as_slice()
                    };
                    Self::apply_item_tokens(
                        &mut item,
                        tokens,
                        &mut current_take,
                        &mut in_take_context,
                    )?;
                }
                crate::primitives::RppBlockContent::RawLine(raw) => {
                    if Self::should_parse_item_raw_line(raw) {
                        let tokens = Self::parse_item_token_line(raw)?;
                        Self::apply_item_tokens(
                            &mut item,
                            &tokens,
                            &mut current_take,
                            &mut in_take_context,
                        )?;
                    }
                }
                crate::primitives::RppBlockContent::Block(nested_block) => {
                    if nested_block.name == "SOURCE" {
                        let source_block = Self::parse_source_block_from_rpp_block(nested_block)?;
                        if let Some(ref mut take) = current_take {
                            take.source = Some(source_block);
                        }
                    } else if let Some(ref mut take) = current_take {
                        // Anything else under a take (`<EXT>` above all) is
                        // real project content: keep it verbatim rather than
                        // dropping it on the floor.
                        take.extra_blocks.push(nested_block.to_string());
                    }
                }
            }
        }

        if let Some(take) = current_take {
            item.takes.push(take);
        }
        Ok(item)
    }

    /// Create an Item from a raw RPP item block string
    ///
    /// # Example
    /// ```
    /// use dawfile_reaper::Item;
    ///
    /// let rpp_content = r#"<ITEM
    ///   POSITION 0.5
    ///   SNAPOFFS 0
    ///   LENGTH 1.256
    ///   LOOP 1
    ///   ALLTAKES 0
    ///   FADEIN 1 0.01 0 1 0 0 0
    ///   FADEOUT 1 0.01 0 1 0 0 0
    ///   MUTE 0 0
    ///   SEL 1
    ///   IGUID {A6A9CEA1-F124-DEB6-16F0-D1761D2084F4}
    ///   IID 3
    ///   NAME 01-250919_0416.wav
    ///   VOLPAN 1 0 1 -1
    ///   SOFFS 0
    ///   PLAYRATE 1 1 0 -1 0 0.0025
    ///   CHANMODE 0
    ///   GUID {1E52F736-F6BD-D957-F1DD-65CA3C4DE4E9}
    ///   RECPASS 1
    ///   <SOURCE WAVE
    ///     FILE "/home/cody/Documents/REAPER Media/01-250919_0416.wav"
    ///   >
    ///   TAKE SEL
    ///   NAME 01-250919_0416-01.wav
    ///   TAKEVOLPAN 0 1 -1
    ///   SOFFS 0
    ///   PLAYRATE 1 1 0 -1 0 0.0025
    ///   CHANMODE 0
    ///   TAKECOLOR 33319124 B
    ///   GUID {4F3DCC1C-3A7B-8161-9D5E-BCA88432F95F}
    ///   RECPASS 2
    ///   <SOURCE WAVE
    ///     FILE "/home/cody/Documents/REAPER Media/01-250919_0416-01.wav"
    ///   >
    /// >"#;
    ///
    /// let item = Item::from_rpp_block(rpp_content).unwrap();
    /// ```
    pub fn from_rpp_block(block_content: &str) -> Result<Self, String> {
        let mut item = Item {
            position: 0.0,
            snap_offset: 0.0,
            length: 0.0,
            loop_source: false,
            play_all_takes: false,
            color: None,
            beat: None,
            selected: false,
            fade_in: None,
            fade_out: None,
            mute: None,
            item_guid: None,
            item_id: None,
            name: String::new(),
            volpan: None,
            slip_offset: 0.0,
            playrate: None,
            channel_mode: ChannelMode::Normal,
            take_guid: None,
            rec_pass: None,
            lane: None,
            y_pos: None,
            takes: Vec::new(),
            stretch_markers: Vec::new(),
            raw_content: block_content.to_string(),
        };

        let lines: Vec<&str> = block_content.lines().collect();
        let mut i = 0;
        let mut current_take: Option<Take> = Some(Self::default_take());
        let mut in_take_context = false; // Track whether we're parsing take-level fields

        while i < lines.len() {
            let line = lines[i].trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("//") {
                i += 1;
                continue;
            }

            // The `<ITEM` header is structure, not content.
            if line.starts_with("<ITEM") {
                i += 1;
                continue;
            }

            // Nested blocks: `<SOURCE ...>` and everything else (`<EXT>`).
            if line.starts_with('<') {
                let mut block_lines = Vec::new();
                block_lines.push(line);
                i += 1;
                let mut depth = 1i32;

                while i < lines.len() {
                    let current_line = lines[i].trim();
                    block_lines.push(current_line);

                    if current_line.starts_with('<') {
                        depth += 1;
                    } else if current_line == ">" {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    i += 1;
                }

                if i >= lines.len() {
                    return Err(format!("nested block {line} not properly closed with '>'"));
                }

                let block_content = block_lines.join("\n");
                if line.starts_with("<SOURCE") {
                    let source_block = Self::parse_source_block(&block_content)?;
                    if let Some(ref mut take) = current_take {
                        take.source = Some(source_block);
                    }
                } else if let Some(ref mut take) = current_take {
                    take.extra_blocks.push(block_content);
                }

                // Skip the increment at the end since we already incremented in the loop
                continue;
            }

            // Skip block end markers
            if line == ">" {
                i += 1;
                continue;
            }

            // Parse token line
            let tokens = match Self::parse_item_token_line(line) {
                Ok(tokens) => tokens,
                Err(_) => {
                    i += 1;
                    continue;
                }
            };

            if tokens.is_empty() {
                i += 1;
                continue;
            }

            Self::apply_item_tokens(&mut item, &tokens, &mut current_take, &mut in_take_context)?;

            i += 1;
        }

        // Add the last take if it exists
        if let Some(take) = current_take {
            item.takes.push(take);
        }

        Ok(item)
    }

    /// The media path out of a tokenized `FILE` line, or `None` if this
    /// is not one.
    ///
    /// The single place that knows what a `FILE` line looks like. Both
    /// SOURCE parsers call it, and they did not always: each had its own
    /// copy, both copies were wrong in the same way, and fixing one of
    /// them fixed nothing because the other was the one that ran.
    ///
    /// What they got wrong: the path is the **first** token after
    /// `FILE`, not everything after it. REAPER writes a flag behind the
    /// path —
    ///
    /// ```text
    /// FILE "Media/_NSYNC - Bye Bye Bye (Official Audio) 0.mp3" 1
    /// ```
    ///
    /// — and joining the remaining tokens folds that `1` into the
    /// filename. The source then fails to open with a "no such file"
    /// naming a file that is sitting right there on disk, the project
    /// loads with zero decoded sources, and it plays silence.
    ///
    /// Joining was presumably meant to hold together a path with spaces
    /// in it. It never did that job: the tokenizer keeps a quoted path
    /// as one token, spaces and all.
    fn file_path_from_tokens(tokens: &[Token]) -> Result<Option<String>, String> {
        if !matches!(tokens.first(), Some(Token::Identifier(id)) if id == "FILE") {
            return Ok(None);
        }
        match tokens.get(1) {
            Some(token) => Ok(Some(Self::parse_string(token)?.replace("\\\\", "\\"))),
            None => Ok(None),
        }
    }

    /// Parse a SOURCE block
    fn parse_source_block(block_content: &str) -> Result<SourceBlock, String> {
        let lines: Vec<&str> = block_content.lines().collect();

        if lines.len() < 2 {
            return Err("SOURCE block must have at least 2 lines".to_string());
        }

        // First line should be "<SOURCE TYPE"
        let first_line = lines[0].trim();
        if !first_line.starts_with("<SOURCE") {
            return Err("Expected SOURCE block to start with '<SOURCE'".to_string());
        }

        // Parse source type
        let source_type_str = first_line.replace("<SOURCE", "").trim().to_string();
        let source_type = SourceType::from(source_type_str.as_str());

        // Find FILE line
        let mut file_path = String::new();
        let inner_lines = &lines[1..lines.len().saturating_sub(1)];
        for line in inner_lines {
            let line = line.trim();
            if line.starts_with("FILE") {
                // Tokenize and go through the one shared extractor, so
                // this parser and the token-based one cannot drift apart
                // on what a FILE line means — which is exactly how the
                // trailing-flag bug survived being fixed once.
                let tokens = Self::parse_item_token_line(line)?;
                if let Some(path) = Self::file_path_from_tokens(&tokens)? {
                    file_path = path;
                }
                break;
            }
        }

        // Parse MIDI data if this is a MIDI source
        let midi_data = if source_type == SourceType::Midi {
            Some(Self::parse_midi_source(inner_lines))
        } else {
            None
        };

        Ok(SourceBlock {
            source_type,
            file_path,
            midi_data,
            raw_content: block_content.to_string(),
        })
    }

    /// Parse a SOURCE block directly from parsed block structure.
    /// Falls back to string parser for MIDI/complex nested formats.
    fn parse_source_block_from_rpp_block(
        block: &crate::primitives::RppBlock,
    ) -> Result<SourceBlock, String> {
        if block.name != "SOURCE" {
            return Err("Expected SOURCE block".to_string());
        }

        if block
            .children
            .iter()
            .any(|c| matches!(c, crate::primitives::RppBlockContent::Block(_)))
        {
            return Self::parse_source_block(&block.to_string());
        }

        let source_type = block
            .params
            .first()
            .and_then(Token::as_string)
            .map(SourceType::from)
            .unwrap_or_else(|| SourceType::Unknown(String::new()));

        if source_type == SourceType::Midi {
            return Self::parse_source_block(&block.to_string());
        }

        let mut file_path = String::new();
        for child in &block.children {
            let mut reparsed_tokens: Option<Vec<Token>> = None;
            let tokens: &[Token] = match child {
                crate::primitives::RppBlockContent::Content(tokens) => {
                    if let [Token::Identifier(raw)] = tokens.as_slice() {
                        if raw.contains(' ') {
                            if !raw.trim_start().starts_with("FILE") {
                                continue;
                            }
                            reparsed_tokens = Some(Self::parse_item_token_line(raw)?);
                            reparsed_tokens.as_deref().unwrap_or(tokens.as_slice())
                        } else {
                            tokens.as_slice()
                        }
                    } else {
                        tokens.as_slice()
                    }
                }
                crate::primitives::RppBlockContent::RawLine(raw) => {
                    if !raw.trim_start().starts_with("FILE") {
                        continue;
                    }
                    reparsed_tokens = Some(Self::parse_item_token_line(raw)?);
                    reparsed_tokens.as_deref().unwrap_or(&[])
                }
                crate::primitives::RppBlockContent::Block(_) => continue,
            };
            if !matches!(tokens.first(), Some(Token::Identifier(id)) if id == "FILE") {
                continue;
            }
            let Some(path) = Self::file_path_from_tokens(tokens)? else {
                continue;
            };
            file_path = path;
            break;
        }

        Ok(SourceBlock {
            source_type,
            file_path,
            midi_data: None,
            raw_content: String::new(),
        })
    }

    /// Parse MIDI source data from inner lines of a `<SOURCE MIDI>` block.
    fn parse_midi_source(lines: &[&str]) -> MidiSource {
        let mut midi = MidiSource {
            has_data: false,
            ticks_per_qn: 960,
            ticks_timebase: None,
            cc_interp: None,
            pooled_evts_guid: None,
            events: Vec::new(),
            extended_events: Vec::new(),
            event_stream: Vec::new(),
            ignore_tempo: None,
            vel_lanes: Vec::new(),
            bank_program_file: None,
            cfg_edit_view: None,
            cfg_edit: None,
            evt_filter: None,
            guid: None,
            unknown_lines: Vec::new(),
        };

        let mut i = 0usize;
        while i < lines.len() {
            let line = lines[i].trim();

            if line.starts_with("HASDATA ") {
                // HASDATA 1 960 QN
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    midi.has_data = parts[1] != "0";
                }
                if parts.len() >= 3 {
                    midi.ticks_per_qn = parts[2].parse().unwrap_or(960);
                }
                if parts.len() >= 4 {
                    midi.ticks_timebase = Some(parts[3].to_string());
                }
            } else if line.starts_with("CCINTERP ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    midi.cc_interp = parts[1].parse().ok();
                }
            } else if let Some(stripped) = line.strip_prefix("POOLEDEVTS ") {
                midi.pooled_evts_guid = Some(stripped.trim().to_string());
            } else if let Some(stripped) = line.strip_prefix("GUID ") {
                midi.guid = Some(stripped.trim().to_string());
            } else if line.starts_with("IGNTEMPO ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    midi.ignore_tempo = Some(MidiIgnoreTempo {
                        enabled: parts[1] != "0",
                        bpm: parts[2].parse().unwrap_or(120.0),
                        numerator: parts[3].parse().unwrap_or(4),
                        denominator: parts[4].parse().unwrap_or(4),
                    });
                }
            } else if line.starts_with("VELLANE ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    midi.vel_lanes.push(MidiVelLane {
                        lane_type: parts[1].parse().unwrap_or(0),
                        midi_editor_height: parts[2].parse().unwrap_or(0),
                        inline_editor_height: parts[3].parse().unwrap_or(0),
                    });
                }
            } else if let Some(stripped) = line.strip_prefix("BANKPROGRAMFILE ") {
                let mut v = stripped.trim().to_string();
                if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    v = v[1..v.len() - 1].to_string();
                }
                v = v.replace("\\\\", "\\");
                midi.bank_program_file = Some(v);
            } else if let Some(stripped) = line.strip_prefix("CFGEDITVIEW ") {
                midi.cfg_edit_view =
                    Some(stripped.split_whitespace().map(|s| s.to_string()).collect());
            } else if let Some(stripped) = line.strip_prefix("CFGEDIT ") {
                midi.cfg_edit = Some(stripped.split_whitespace().map(|s| s.to_string()).collect());
            } else if let Some(stripped) = line.strip_prefix("EVTFILTER ") {
                midi.evt_filter =
                    Some(stripped.split_whitespace().map(|s| s.to_string()).collect());
            } else if line.starts_with("<X ") || line.starts_with("<x ") {
                let selected = line.starts_with("<x ");
                let fields: Vec<String> = line[2..]
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                let mut data_lines = Vec::new();
                i += 1;
                while i < lines.len() {
                    let payload_line = lines[i].trim();
                    if payload_line == ">" {
                        break;
                    }
                    if !payload_line.is_empty() {
                        data_lines.push(payload_line.to_string());
                    }
                    i += 1;
                }
                let event = MidiExtendedEvent {
                    selected,
                    fields,
                    data_lines,
                };
                midi.extended_events.push(event.clone());
                midi.event_stream.push(MidiSourceEvent::Extended(event));
            } else if line.starts_with("E ") || line.starts_with("e ") {
                // MIDI event: E <delta_ticks> <status_hex> <data1_hex> [data2_hex ...]
                let parts: Vec<&str> = line[2..].split_whitespace().collect();
                if parts.len() >= 2 {
                    let delta_ticks = parts[0].parse::<u32>().unwrap_or(0);
                    let bytes: Vec<u8> = parts[1..]
                        .iter()
                        .filter_map(|s| u8::from_str_radix(s, 16).ok())
                        .collect();
                    if !bytes.is_empty() {
                        let event = MidiEvent { delta_ticks, bytes };
                        midi.events.push(event.clone());
                        midi.event_stream.push(MidiSourceEvent::Midi(event));
                    }
                }
            } else if !line.is_empty() {
                midi.unknown_lines.push(line.to_string());
            }

            i += 1;
        }

        midi
    }

    /// Parse a float from a token
    fn parse_float(token: &Token) -> Result<f64, String> {
        match token {
            Token::Float(f) => Ok(*f),
            Token::Integer(i) => Ok(*i as f64),
            _ => Err(format!("Expected float or integer, got {:?}", token)),
        }
    }

    /// Parse an integer from a token
    fn parse_int(token: &Token) -> Result<i32, String> {
        match token {
            Token::Integer(i) => Ok(*i as i32),
            Token::Float(f) => Ok(*f as i32),
            _ => Err(format!("Expected integer or float, got {:?}", token)),
        }
    }

    /// Parse a boolean from a token
    fn parse_bool(token: &Token) -> Result<bool, String> {
        match token {
            Token::Integer(i) => Ok(*i != 0),
            Token::Float(f) => Ok(*f != 0.0),
            _ => Err(format!(
                "Expected integer or float for boolean, got {:?}",
                token
            )),
        }
    }

    /// Parse a string from a token
    fn parse_string(token: &Token) -> Result<String, String> {
        match token {
            Token::String(s, _) => Ok(s.clone()),
            Token::Identifier(s) => Ok(s.clone()),
            // Purely numeric names appear unquoted in real projects
            // (`LANENAME 1 2 3`, takes named `2`) — stringify them.
            Token::Integer(i) => Ok(i.to_string()),
            Token::Float(f) => Ok(f.to_string()),
            _ => Err(format!("Expected string or identifier, got {:?}", token)),
        }
    }
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Item({}) - {} takes", self.name, self.takes.len())
    }
}

/// Helper function to format Option values nicely
fn format_option<T: fmt::Display>(opt: &Option<T>) -> String {
    match opt {
        Some(value) => value.to_string(),
        None => "None".to_string(),
    }
}

/// Helper function to format GUIDs without braces
fn format_guid(guid: &Option<String>) -> String {
    match guid {
        Some(g) => {
            // Remove braces if present
            let cleaned = g.trim_start_matches('{').trim_end_matches('}');
            cleaned.to_string()
        }
        None => "None".to_string(),
    }
}

impl fmt::Display for Take {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Take({})", self.name)
    }
}

impl fmt::Display for FadeSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "curve: {}, time: {:.3}s", self.curve_type, self.time)
    }
}

impl fmt::Display for MuteSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "muted: {}, solo: {}", self.muted, self.solo_state)
    }
}

impl fmt::Display for VolPanSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "trim: {:.2}, pan: {:.2}, vol: {:.2}",
            self.item_trim, self.take_pan, self.take_volume
        )
    }
}

impl fmt::Display for PlayRateSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rate: {:.3}, preserve_pitch: {}, pitch: {:.2}, mode: {}",
            self.rate, self.preserve_pitch, self.pitch_adjust, self.pitch_mode
        )
    }
}

impl fmt::Display for SourceBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.source_type, self.file_path)
    }
}
