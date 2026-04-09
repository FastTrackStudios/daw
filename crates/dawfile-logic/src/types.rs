//! Domain types for a parsed Logic Pro session.
//!
//! These types represent the parsed contents of a `.logicx` bundle.
//! They are format-specific and not yet mapped to `daw_proto` types.

/// A fully parsed Logic Pro session.
#[derive(Debug, Clone)]
pub struct LogicSession {
    /// Logic Pro version string that last saved the file (e.g. `"Logic Pro 12.0.1 (6590)"`).
    pub creator_version: String,
    /// Variant (alternative) name (e.g. `"FileDecrypt"`).
    pub variant_name: String,
    /// Session sample rate in Hz (e.g. 48000).
    pub sample_rate: u32,
    /// Initial tempo in beats per minute.
    pub bpm: f64,
    /// Time signature numerator (beats per bar).
    pub time_sig_numerator: u32,
    /// Time signature denominator (beat unit, e.g. 4 = quarter note).
    pub time_sig_denominator: u32,
    /// Root key name (e.g. `"C"`).
    pub key: String,
    /// Scale / mode (e.g. `"major"`).
    pub key_gender: String,
    /// All tracks in the project, in mixer order.
    pub tracks: Vec<LogicTrack>,
    /// Arrangement markers.
    pub markers: Vec<LogicMarker>,
    /// Tempo change events (beyond the initial `bpm`).
    pub tempo_events: Vec<LogicTempoEvent>,
    /// Summing groups (Logic "Summing Stacks").
    pub summing_groups: Vec<LogicSummingGroup>,
    /// Raw chunk inventory from ProjectData — always populated, useful for debugging.
    pub chunks: Vec<LogicChunk>,
}

/// A track in the Logic Pro mixer.
#[derive(Debug, Clone)]
pub struct LogicTrack {
    /// User-visible track name.
    pub name: String,
    /// What kind of data this track carries.
    pub kind: TrackKind,
    /// Channel number in the mixer (1-based).
    pub channel: u32,
    /// Fader level in dB (decoded from Logic's internal encoding).
    pub fader_db: Option<f64>,
    /// Whether the track is muted.
    pub muted: bool,
    /// Whether the track is soloed.
    pub soloed: bool,
    /// Optional parent summing group index (into `LogicSession::summing_groups`).
    pub parent_group: Option<usize>,
    /// Clips placed on this track.
    pub clips: Vec<LogicClip>,
}

/// The data type carried by a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackKind {
    /// Audio waveform data.
    Audio,
    /// Software instrument (MIDI-driven).
    SoftwareInstrument,
    /// External MIDI track.
    Midi,
    /// Aux / bus channel.
    Aux,
    /// Master output.
    Master,
    /// Unknown / uncategorised track type.
    Other,
}

/// A clip (audio region or MIDI region) placed on a track.
#[derive(Debug, Clone)]
pub struct LogicClip {
    /// Start position in the project, in beats (quarter notes from bar 1 beat 1).
    pub position_beats: f64,
    /// Duration in beats.
    pub length_beats: f64,
    /// What the clip contains.
    pub kind: ClipKind,
}

/// The content of a clip.
#[derive(Debug, Clone)]
pub enum ClipKind {
    /// A reference to an audio file on disk.
    Audio {
        /// Resolved path to the audio file, if available.
        file_path: Option<String>,
    },
    /// A MIDI region (sequence of notes / CC events).
    Midi,
    /// A clip type we don't yet model.
    Other,
}

/// An arrangement marker.
#[derive(Debug, Clone)]
pub struct LogicMarker {
    /// Position in the project, in beats.
    pub position_beats: f64,
    /// Marker label text.
    pub name: String,
}

/// A tempo change event on the tempo map.
#[derive(Debug, Clone)]
pub struct LogicTempoEvent {
    /// Position in the project, in beats.
    pub position_beats: f64,
    /// Tempo in BPM at this position.
    pub bpm: f64,
}

/// A summing group ("Summing Stack" in Logic Pro).
///
/// A summing group collects one or more tracks and routes them through a
/// shared bus channel before the master output.
#[derive(Debug, Clone)]
pub struct LogicSummingGroup {
    /// User-visible name for the group bus channel.
    pub name: String,
    /// Names / channel numbers of member tracks.
    pub member_names: Vec<String>,
}

// ── Raw chunk representation ──────────────────────────────────────────────────

/// A raw chunk read from the `ProjectData` binary.
///
/// Provides full access to the header metadata and payload bytes for
/// debugging, reverse-engineering, and future parser extensions.
#[derive(Debug, Clone)]
pub struct LogicChunk {
    /// Byte offset of this chunk's header within the `ProjectData` file.
    pub offset: usize,
    /// 4-character type tag as stored on disk (little-endian, e.g. `"gnoS"`).
    pub tag: [u8; 4],
    /// Human-readable form of the tag (reversed, e.g. `"Song"`).
    pub type_name: String,
    /// 28 bytes of header metadata preceding the size field (indices 4–31).
    pub header_meta: [u8; 28],
    /// Length of the data payload in bytes.
    pub data_len: u64,
    /// The payload bytes.
    pub data: Vec<u8>,
}

impl LogicChunk {
    /// Returns the tag as a UTF-8 string (best-effort, replaces invalid bytes with `?`).
    pub fn tag_str(&self) -> &str {
        std::str::from_utf8(&self.tag).unwrap_or("????")
    }
}
