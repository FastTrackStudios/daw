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
    Midi,
    OfflineWave,
    Unknown(String),
}

impl From<&str> for SourceType {
    fn from(value: &str) -> Self {
        match value.to_uppercase().as_str() {
            "WAVE" => SourceType::Wave,
            "MIDI" => SourceType::Midi,
            "_OFFLINE_WAVE" => SourceType::OfflineWave,
            _ => SourceType::Unknown(value.to_string()),
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceType::Wave => write!(f, "Wave"),
            SourceType::Midi => write!(f, "MIDI"),
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

    // Takes
    pub takes: Vec<Take>,

    // Raw content for preservation
    pub raw_content: String,
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
}

/// Source block for a take
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceBlock {
    pub source_type: SourceType, // WAVE, MIDI, etc.
    pub file_path: String,       // FILE - Source file path
    pub raw_content: String,     // Raw content for preservation
}

impl Item {
    /// Create an Item from a parsed RPP block (legacy method for compatibility)
    pub fn from_block(block: &crate::primitives::RppBlock) -> Result<Self, String> {
        // Convert the block back to string format for parsing
        let mut content = String::new();
        content.push_str(&format!("<{}", block.name));

        // Add parameters if any
        if !block.params.is_empty() {
            for param in &block.params {
                content.push_str(&format!(" {}", param));
            }
        }
        content.push('\n');

        // Add content lines
        for child in &block.children {
            match child {
                crate::primitives::RppBlockContent::Content(tokens) => {
                    let line = tokens
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    content.push_str(&format!("  {}\n", line));
                }
                crate::primitives::RppBlockContent::Block(nested_block) => {
                    // For nested blocks, we need to handle them specially
                    // This is a simplified approach - in practice, you might want more sophisticated handling
                    content.push_str(&format!("  <{}>\n", nested_block.name));
                }
            }
        }

        content.push('>');

        Self::from_rpp_block(&content)
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
        use crate::primitives::token::parse_token_line;

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
            takes: Vec::new(),
            raw_content: block_content.to_string(),
        };

        let lines: Vec<&str> = block_content.lines().collect();
        let mut i = 0;
        let mut current_take: Option<Take> = Some(Take {
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
        });
        let mut in_take_context = false; // Track whether we're parsing take-level fields

        while i < lines.len() {
            let line = lines[i].trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("//") {
                i += 1;
                continue;
            }

            // Handle SOURCE blocks first (before token parsing)
            if line.starts_with("<SOURCE") {
                let mut source_lines = Vec::new();
                source_lines.push(line);
                i += 1;

                while i < lines.len() {
                    let current_line = lines[i].trim();
                    source_lines.push(current_line);

                    if current_line == ">" {
                        break;
                    }
                    i += 1;
                }

                if i >= lines.len() {
                    return Err("SOURCE block not properly closed with '>'".to_string());
                }

                let source_block_content = source_lines.join("\n");
                let source_block = Self::parse_source_block(&source_block_content)?;

                if let Some(ref mut take) = current_take {
                    take.source = Some(source_block);
                }

                // Skip the increment at the end since we already incremented in the loop
                continue;
            }

            // Skip block start/end markers
            if line.starts_with('<') || line == ">" {
                i += 1;
                continue;
            }

            // Parse token line
            let tokens = match parse_token_line(line) {
                Ok((_, tokens)) => tokens,
                Err(_) => {
                    i += 1;
                    continue;
                }
            };

            if tokens.is_empty() {
                i += 1;
                continue;
            }

            let identifier = match &tokens[0] {
                Token::Identifier(id) => id,
                _ => {
                    i += 1;
                    continue;
                }
            };

            match identifier.as_str() {
                "POSITION" => {
                    if tokens.len() > 1 {
                        item.position = Self::parse_float(&tokens[1])?;
                    }
                }
                "SNAPOFFS" => {
                    if tokens.len() > 1 {
                        item.snap_offset = Self::parse_float(&tokens[1])?;
                    }
                }
                "LENGTH" => {
                    if tokens.len() > 1 {
                        item.length = Self::parse_float(&tokens[1])?;
                    }
                }
                "LOOP" => {
                    if tokens.len() > 1 {
                        item.loop_source = Self::parse_bool(&tokens[1])?;
                    }
                }
                "ALLTAKES" => {
                    if tokens.len() > 1 {
                        item.play_all_takes = Self::parse_bool(&tokens[1])?;
                    }
                }
                "COLOR" => {
                    if tokens.len() > 1 {
                        item.color = Some(Self::parse_int(&tokens[1])?);
                    }
                }
                "BEAT" => {
                    if tokens.len() > 1 {
                        item.beat = Some(ItemTimebase::from(Self::parse_int(&tokens[1])?));
                    }
                }
                "SEL" => {
                    if tokens.len() > 1 {
                        item.selected = Self::parse_bool(&tokens[1])?;
                    }
                }
                "FADEIN" => {
                    if tokens.len() >= 3 {
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
                }
                "FADEOUT" => {
                    if tokens.len() >= 3 {
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
                }
                "MUTE" => {
                    if tokens.len() >= 3 {
                        item.mute = Some(MuteSettings {
                            muted: Self::parse_bool(&tokens[1])?,
                            solo_state: SoloState::from(Self::parse_int(&tokens[2])?),
                        });
                    }
                }
                "IGUID" => {
                    if tokens.len() > 1 {
                        item.item_guid = Some(Self::parse_string(&tokens[1])?);
                    }
                }
                "IID" => {
                    if tokens.len() > 1 {
                        item.item_id = Some(Self::parse_int(&tokens[1])?);
                    }
                }
                "NAME" => {
                    if tokens.len() > 1 {
                        let name = Self::parse_string(&tokens[1])?;
                        if in_take_context {
                            // This is a take-level NAME
                            if let Some(ref mut take) = current_take {
                                take.name = name;
                            }
                        } else {
                            // This is an item-level NAME
                            item.name = name;
                        }
                    }
                }
                "VOLPAN" => {
                    if tokens.len() >= 5 {
                        item.volpan = Some(VolPanSettings {
                            item_trim: Self::parse_float(&tokens[1])?,
                            take_pan: Self::parse_float(&tokens[2])?,
                            take_volume: Self::parse_float(&tokens[3])?,
                            take_pan_law: Self::parse_float(&tokens[4])?,
                        });
                    }
                }
                "SOFFS" => {
                    if tokens.len() > 1 {
                        item.slip_offset = Self::parse_float(&tokens[1])?;
                    }
                }
                "PLAYRATE" => {
                    if tokens.len() >= 4 {
                        item.playrate = Some(PlayRateSettings {
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
                        });
                    }
                }
                "CHANMODE" => {
                    if tokens.len() > 1 {
                        item.channel_mode = ChannelMode::from(Self::parse_int(&tokens[1])?);
                    }
                }
                "GUID" => {
                    if tokens.len() > 1 {
                        let guid = Self::parse_string(&tokens[1])?;
                        if in_take_context {
                            // This is a take-level GUID
                            if let Some(ref mut take) = current_take {
                                take.take_guid = Some(guid);
                            }
                        } else {
                            // This is an item-level GUID (first take's GUID)
                            item.take_guid = Some(guid.clone());
                            // Also set it as the first take's GUID
                            if let Some(ref mut take) = current_take {
                                take.take_guid = Some(guid);
                            }
                        }
                    }
                }
                "RECPASS" => {
                    if tokens.len() > 1 {
                        let rec_pass = Self::parse_int(&tokens[1])?;
                        if in_take_context {
                            // This is a take-level RECPASS
                            if let Some(ref mut take) = current_take {
                                take.rec_pass = Some(rec_pass);
                            }
                        } else {
                            // This is an item-level RECPASS
                            item.rec_pass = Some(rec_pass);
                        }
                    }
                }
                "TAKE" => {
                    // Check if this is TAKE SEL
                    let is_selected =
                        tokens.len() > 1 && tokens[1] == Token::Identifier("SEL".to_string());

                    // Start of a new take
                    if let Some(take) = current_take.take() {
                        item.takes.push(take);
                    }
                    current_take = Some(Take {
                        is_selected,
                        name: String::new(),
                        volpan: None,
                        slip_offset: 0.0,
                        playrate: None,
                        channel_mode: ChannelMode::Normal,
                        take_color: None,
                        take_guid: None,
                        rec_pass: None,
                        source: None,
                    });
                    in_take_context = true; // We're now in take context
                }
                "TAKEVOLPAN" => {
                    if let Some(ref mut take) = current_take {
                        if tokens.len() >= 4 {
                            take.volpan = Some(VolPanSettings {
                                item_trim: 0.0, // Not applicable for takes
                                take_pan: Self::parse_float(&tokens[1])?,
                                take_volume: Self::parse_float(&tokens[2])?,
                                take_pan_law: Self::parse_float(&tokens[3])?,
                            });
                        }
                    }
                }
                "TAKECOLOR" => {
                    if let Some(ref mut take) = current_take {
                        if tokens.len() > 1 {
                            take.take_color = Some(Self::parse_int(&tokens[1])?);
                        }
                    }
                }
                _ => {
                    // Handle take-specific fields
                    if let Some(ref mut take) = current_take {
                        match identifier.as_str() {
                            "SOFFS" => {
                                if tokens.len() > 1 {
                                    take.slip_offset = Self::parse_float(&tokens[1])?;
                                }
                            }
                            "PLAYRATE" => {
                                if tokens.len() >= 4 {
                                    take.playrate = Some(PlayRateSettings {
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
                                    });
                                }
                            }
                            "CHANMODE" => {
                                if tokens.len() > 1 {
                                    take.channel_mode =
                                        ChannelMode::from(Self::parse_int(&tokens[1])?);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            i += 1;
        }

        // Add the last take if it exists
        if let Some(take) = current_take {
            item.takes.push(take);
        }

        Ok(item)
    }

    /// Parse a SOURCE block
    fn parse_source_block(block_content: &str) -> Result<SourceBlock, String> {
        let lines: Vec<&str> = block_content.lines().collect();

        if lines.len() < 3 {
            return Err("SOURCE block must have at least 3 lines".to_string());
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
        for line in &lines[1..lines.len() - 1] {
            let line = line.trim();
            if line.starts_with("FILE") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() > 1 {
                    file_path = parts[1..].join(" ");
                    // Remove quotes if present
                    if file_path.starts_with('"') && file_path.ends_with('"') {
                        file_path = file_path[1..file_path.len() - 1].to_string();
                    }
                    // Fix double-escaped backslashes
                    file_path = file_path.replace("\\\\", "\\");
                }
                break;
            }
        }

        Ok(SourceBlock {
            source_type,
            file_path,
            raw_content: block_content.to_string(),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_complex_item() {
        let rpp_content = r#"<ITEM
  POSITION 0.5
  SNAPOFFS 0
  LENGTH 1.256
  LOOP 1
  ALLTAKES 0
  FADEIN 1 0.01 0 1 0 0 0
  FADEOUT 1 0.01 0 1 0 0 0
  MUTE 0 0
  SEL 1
  IGUID {A6A9CEA1-F124-DEB6-16F0-D1761D2084F4}
  IID 3
  NAME 01-250919_0416.wav
  VOLPAN 1 0 1 -1
  SOFFS 0
  PLAYRATE 1 1 0 -1 0 0.0025
  CHANMODE 0
  GUID {1E52F736-F6BD-D957-F1DD-65CA3C4DE4E9}
  RECPASS 1
  <SOURCE WAVE
    FILE "/home/cody/Documents/REAPER Media/01-250919_0416.wav"
  >
  TAKE SEL
  NAME 01-250919_0416-01.wav
  TAKEVOLPAN 0 1 -1
  SOFFS 0
  PLAYRATE 1 1 0 -1 0 0.0025
  CHANMODE 0
  TAKECOLOR 33319124 B
  GUID {4F3DCC1C-3A7B-8161-9D5E-BCA88432F95F}
  RECPASS 2
  <SOURCE WAVE
    FILE "/home/cody/Documents/REAPER Media/01-250919_0416-01.wav"
  >
  TAKE
  NAME 01-250919_0416-02.wav
  TAKEVOLPAN 0 1 -1
  SOFFS 0
  PLAYRATE 1 1 0 -1 0 0.0025
  CHANMODE 0
  TAKECOLOR 32163693 B
  GUID {7D9D5C13-2CF0-FF8B-66FD-F9C0D53E2FD5}
  RECPASS 3
  <SOURCE WAVE
    FILE "/home/cody/Documents/REAPER Media/01-250919_0416-02.wav"
  >
  TAKE
  NAME 01-250919_0416-03.wav
  TAKEVOLPAN 0 1 -1
  SOFFS 0
  PLAYRATE 1 1 0 -1 0 0.0025
  CHANMODE 0
  TAKECOLOR 24072829 B
  GUID {A943CE1C-349F-F012-5C9E-1568123B2135}
  RECPASS 4
  <SOURCE WAVE
    FILE "/home/cody/Documents/REAPER Media/01-250919_0416-03.wav"
  >
>"#;

        let item = Item::from_rpp_block(rpp_content).unwrap();

        println!("\n🎵 Parsed Complex Item:");
        println!("{}", "=".repeat(60));
        println!("Item: {}", item);
        println!(
            "  Position: {:.3}s, Length: {:.3}s",
            item.position, item.length
        );
        println!("  Snap Offset: {:.3}s", item.snap_offset);
        println!(
            "  Loop Source: {}, Play All Takes: {}",
            item.loop_source, item.play_all_takes
        );
        println!("  Selected: {}", item.selected);
        println!("  Item GUID: {}", format_guid(&item.item_guid));
        println!("  Item ID: {}", format_option(&item.item_id));
        println!("  Recording Pass: {}", format_option(&item.rec_pass));

        if let Some(ref fade_in) = item.fade_in {
            println!("  Fade In: {}", fade_in);
        }
        if let Some(ref fade_out) = item.fade_out {
            println!("  Fade Out: {}", fade_out);
        }
        if let Some(ref mute) = item.mute {
            println!("  Mute: {}", mute);
        }
        if let Some(ref volpan) = item.volpan {
            println!("  VolPan: {}", volpan);
        }
        if let Some(ref playrate) = item.playrate {
            println!("  PlayRate: {}", playrate);
        }

        println!("\n  Takes ({}):", item.takes.len());
        for (i, take) in item.takes.iter().enumerate() {
            println!("    Take {}: {}", i + 1, take);
            println!("      Selected: {}", take.is_selected);
            println!("      Slip Offset: {:.3}s", take.slip_offset);
            println!("      Channel Mode: {}", take.channel_mode);
            println!("      Take Color: {}", format_option(&take.take_color));
            println!("      Take GUID: {}", format_guid(&take.take_guid));
            println!("      Recording Pass: {}", format_option(&take.rec_pass));

            if let Some(ref volpan) = take.volpan {
                println!("      VolPan: {}", volpan);
            }
            if let Some(ref playrate) = take.playrate {
                println!("      PlayRate: {}", playrate);
            }
            if let Some(ref source) = take.source {
                println!("      Source: {}", source);
            }
            println!();
        }

        // Verify basic item properties
        assert_eq!(item.position, 0.5);
        assert_eq!(item.snap_offset, 0.0);
        assert_eq!(item.length, 1.256);
        assert!(item.loop_source);
        assert!(!item.play_all_takes);
        assert!(item.selected);
        assert_eq!(item.name, "01-250919_0416.wav");
        assert_eq!(item.item_id, Some(3));
        assert_eq!(item.rec_pass, Some(1));

        // Verify fade settings
        assert!(item.fade_in.is_some());
        let fade_in = item.fade_in.unwrap();
        assert_eq!(fade_in.curve_type, FadeCurveType::Square);
        assert_eq!(fade_in.time, 0.01);
        assert_eq!(fade_in.unknown_field_3, 0.0);
        assert_eq!(fade_in.unknown_field_4, 1);
        assert_eq!(fade_in.unknown_field_5, 0);
        assert_eq!(fade_in.unknown_field_6, 0);
        assert_eq!(fade_in.unknown_field_7, 0);

        assert!(item.fade_out.is_some());
        let fade_out = item.fade_out.unwrap();
        assert_eq!(fade_out.curve_type, FadeCurveType::Square);
        assert_eq!(fade_out.time, 0.01);
        assert_eq!(fade_out.unknown_field_3, 0.0);
        assert_eq!(fade_out.unknown_field_4, 1);
        assert_eq!(fade_out.unknown_field_5, 0);
        assert_eq!(fade_out.unknown_field_6, 0);
        assert_eq!(fade_out.unknown_field_7, 0);

        // Verify mute settings
        assert!(item.mute.is_some());
        let mute = item.mute.unwrap();
        assert!(!mute.muted);
        assert_eq!(mute.solo_state, SoloState::NotSoloed);

        // Verify volpan settings
        assert!(item.volpan.is_some());
        let volpan = item.volpan.unwrap();
        assert_eq!(volpan.item_trim, 1.0);
        assert_eq!(volpan.take_pan, 0.0);
        assert_eq!(volpan.take_volume, 1.0);
        assert_eq!(volpan.take_pan_law, -1.0);

        // Verify playrate settings
        assert!(item.playrate.is_some());
        let playrate = item.playrate.unwrap();
        assert_eq!(playrate.rate, 1.0);
        assert!(playrate.preserve_pitch);
        assert_eq!(playrate.pitch_adjust, 0.0);
        assert_eq!(playrate.pitch_mode, PitchMode::ProjectDefault);

        // Verify takes
        assert_eq!(item.takes.len(), 4);

        // First take (implicit, not selected)
        let take1 = &item.takes[0];
        assert!(!take1.is_selected);
        assert_eq!(take1.name, "");
        assert_eq!(take1.rec_pass, None);
        assert!(take1.source.is_some());
        let source1 = take1.source.as_ref().unwrap();
        assert_eq!(source1.source_type, SourceType::Wave);
        assert_eq!(
            source1.file_path,
            "/home/cody/Documents/REAPER Media/01-250919_0416.wav"
        );

        // Second take (selected)
        let take2 = &item.takes[1];
        assert!(take2.is_selected);
        assert_eq!(take2.name, "01-250919_0416-01.wav");
        assert_eq!(take2.rec_pass, Some(2));

        // Third take
        let take3 = &item.takes[2];
        assert!(!take3.is_selected);
        assert_eq!(take3.name, "01-250919_0416-02.wav");
        assert_eq!(take3.rec_pass, Some(3));

        // Fourth take
        let take4 = &item.takes[3];
        assert!(!take4.is_selected);
        assert_eq!(take4.name, "01-250919_0416-03.wav");
        assert_eq!(take4.rec_pass, Some(4));

        println!(
            "✅ Successfully parsed complex item with {} takes!",
            item.takes.len()
        );
    }

    #[test]
    fn test_parse_simple_item() {
        let rpp_content = r#"<ITEM
  POSITION 0.0
  SNAPOFFS 0.0
  LENGTH 145.5
  LOOP 0
  ALLTAKES 0
  SEL 1
  NAME "Simple Item"
  VOLPAN 1.0 0.0 1.0 -1.0
  SOFFS 0.0
  PLAYRATE 1.0 1 0.0 -1
  CHANMODE 0
  <SOURCE WAVE
    FILE "C:\\Path\\To\\AudioFile.wav"
  >
>"#;

        let item = Item::from_rpp_block(rpp_content).unwrap();

        println!("\n🎵 Parsed Simple Item:");
        println!("{}", "=".repeat(40));
        println!("Item: {}", item);
        println!(
            "  Position: {:.3}s, Length: {:.3}s",
            item.position, item.length
        );
        println!("  Name: {}", item.name);
        println!("  Takes: {}", item.takes.len());

        // Debug: show take details
        for (i, take) in item.takes.iter().enumerate() {
            println!(
                "  Take {}: name='{}', source={}",
                i,
                take.name,
                take.source.is_some()
            );
        }

        // Verify basic properties
        assert_eq!(item.position, 0.0);
        assert_eq!(item.length, 145.5);
        assert!(!item.loop_source);
        assert!(!item.play_all_takes);
        assert!(item.selected);
        assert_eq!(item.name, "Simple Item");
        assert_eq!(item.takes.len(), 1);

        // Verify the single take
        let take = &item.takes[0];
        assert!(!take.is_selected);
        assert!(take.source.is_some());
        let source = take.source.as_ref().unwrap();
        assert_eq!(source.source_type, SourceType::Wave);
        assert_eq!(source.file_path, "C:\\Path\\To\\AudioFile.wav");

        println!("✅ Successfully parsed simple item!");
    }
}
