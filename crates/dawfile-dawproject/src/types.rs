//! Domain types for DawProject data.
//!
//! These types represent the parsed contents of a `.dawproject` file.
//! They are format-specific (not yet mapped to `daw_proto` types).

use std::path::PathBuf;

// ─── Top-level ───────────────────────────────────────────────────────────────

/// A parsed DawProject file.
#[derive(Debug, Clone)]
pub struct DawProject {
    /// DawProject format version (e.g. "1.0").
    pub version: String,
    /// Application that created this project.
    pub application: Option<Application>,
    /// Project metadata (from metadata.xml, if present).
    pub metadata: Option<ProjectMetadata>,
    /// Global transport settings (tempo, time signature).
    pub transport: Transport,
    /// Track hierarchy (the `<Structure>` element).
    pub tracks: Vec<Track>,
    /// The main arrangement timeline.
    pub arrangement: Option<Arrangement>,
    /// Clip launcher scenes.
    pub scenes: Vec<Scene>,
}

/// Application info embedded in the project.
#[derive(Debug, Clone)]
pub struct Application {
    pub name: String,
    pub version: String,
}

/// Project metadata from `metadata.xml`.
#[derive(Debug, Clone, Default)]
pub struct ProjectMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub composer: Option<String>,
    pub songwriter: Option<String>,
    pub producer: Option<String>,
    pub original_artist: Option<String>,
    pub arranger: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub copyright: Option<String>,
    pub website: Option<String>,
    pub comment: Option<String>,
}

// ─── Transport ───────────────────────────────────────────────────────────────

/// Global transport settings.
#[derive(Debug, Clone)]
pub struct Transport {
    /// Project tempo in BPM.
    pub tempo: f64,
    /// Time signature numerator.
    pub numerator: u8,
    /// Time signature denominator (power of 2: 2, 4, 8, 16).
    pub denominator: u8,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            tempo: 120.0,
            numerator: 4,
            denominator: 4,
        }
    }
}

// ─── Track hierarchy ─────────────────────────────────────────────────────────

/// A track in the project structure.
#[derive(Debug, Clone)]
pub struct Track {
    /// Unique XML ID used for cross-references.
    pub id: String,
    /// Track name.
    pub name: String,
    /// Track color (hex RGB string, e.g. "#FF8800").
    pub color: Option<String>,
    /// Content type this track holds.
    pub content_type: ContentType,
    /// Mixer channel for this track (absent on group/folder-only tracks).
    pub channel: Option<Channel>,
    /// Child tracks (for group/folder tracks).
    pub children: Vec<Track>,
}

/// What kind of content a track holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentType {
    Audio,
    Notes,
    Automation,
    Video,
    Markers,
    /// Mixed or unknown.
    Unknown,
}

impl ContentType {
    pub fn from_str(s: &str) -> Self {
        // The spec allows space-separated list of content types; use the first.
        let first = s.split_whitespace().next().unwrap_or("");
        match first {
            "audio" => Self::Audio,
            "notes" => Self::Notes,
            "automation" => Self::Automation,
            "video" => Self::Video,
            "markers" => Self::Markers,
            _ => Self::Unknown,
        }
    }
}

/// Mixer channel strip.
#[derive(Debug, Clone)]
pub struct Channel {
    /// Channel ID for automation targeting.
    pub id: String,
    /// Channel role.
    pub role: ChannelRole,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub audio_channels: u32,
    /// Volume (linear amplitude: 1.0 = 0 dB).
    pub volume: f64,
    /// Pan (-1.0 = full left, 0.0 = center, 1.0 = full right).
    pub pan: f64,
    /// Whether the channel is muted.
    pub muted: bool,
    /// Send routing entries.
    pub sends: Vec<Send>,
    /// Devices (plugins and built-in effects).
    pub devices: Vec<Device>,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            id: String::new(),
            role: ChannelRole::Regular,
            audio_channels: 2,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            sends: Vec::new(),
            devices: Vec::new(),
        }
    }
}

/// Role of a mixer channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRole {
    /// Regular track channel.
    Regular,
    /// Master output channel.
    Master,
    /// Effect return channel.
    Effect,
}

impl ChannelRole {
    pub fn from_str(s: &str) -> Self {
        match s {
            "master" => Self::Master,
            "effect" => Self::Effect,
            _ => Self::Regular,
        }
    }
}

/// A send from one channel to another.
#[derive(Debug, Clone)]
pub struct Send {
    /// ID of the destination channel.
    pub target: String,
    /// Send level (linear amplitude).
    pub volume: f64,
    /// Whether this is a pre-fader send.
    pub pre_fader: bool,
}

// ─── Devices ─────────────────────────────────────────────────────────────────

/// A device (plugin or built-in effect) on a channel.
#[derive(Debug, Clone)]
pub struct Device {
    /// Device name.
    pub name: String,
    /// Device format.
    pub format: DeviceFormat,
    /// Plugin file path (for external plugins).
    pub plugin_path: Option<PathBuf>,
    /// Whether the device is enabled.
    pub enabled: bool,
    /// Raw state blob (base64 or file reference, if any).
    pub state: Option<DeviceState>,
}

/// The format/type of a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceFormat {
    /// VST2 plugin.
    Vst2,
    /// VST3 plugin.
    Vst3,
    /// CLAP plugin.
    Clap,
    /// Audio Unit plugin (macOS).
    Au,
    /// Built-in DAW device.
    Builtin,
    /// Unknown format.
    Unknown,
}

/// Opaque device state blob.
#[derive(Debug, Clone)]
pub enum DeviceState {
    /// Inline base64-encoded state.
    Base64(String),
    /// Path to a state file inside the archive.
    File(String),
}

// ─── Arrangement ─────────────────────────────────────────────────────────────

/// The main arrangement timeline.
#[derive(Debug, Clone)]
pub struct Arrangement {
    pub id: String,
    /// Time unit for the arrangement lanes ("beats" or "seconds").
    pub time_unit: TimeUnit,
    /// Lanes holding clips/notes/automation.
    pub lanes: Vec<Lane>,
}

/// Time unit used for positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Beats,
    Seconds,
}

impl TimeUnit {
    pub fn from_str(s: &str) -> Self {
        match s {
            "seconds" => Self::Seconds,
            _ => Self::Beats,
        }
    }
}

/// A lane within the arrangement, associated with a track.
#[derive(Debug, Clone)]
pub struct Lane {
    pub id: String,
    /// ID of the track this lane belongs to.
    pub track: String,
    /// Time unit override for this lane (falls back to arrangement).
    pub time_unit: Option<TimeUnit>,
    /// Content stored in this lane.
    pub content: LaneContent,
}

/// Content stored in a lane.
#[derive(Debug, Clone)]
pub enum LaneContent {
    /// Clips (audio or MIDI).
    Clips(Vec<Clip>),
    /// MIDI notes directly in the lane (rare).
    Notes(Vec<Note>),
    /// Automation points.
    Automation(AutomationLane),
    /// Markers.
    Markers(Vec<Marker>),
}

/// A clip on the arrangement timeline.
#[derive(Debug, Clone)]
pub struct Clip {
    pub id: String,
    /// Timeline position.
    pub time: f64,
    /// Clip duration.
    pub duration: f64,
    /// Time unit for `time` and `duration`.
    pub time_unit: Option<TimeUnit>,
    /// Content time unit (may differ from timeline unit).
    pub content_time_unit: Option<TimeUnit>,
    /// Clip name.
    pub name: Option<String>,
    /// Clip color.
    pub color: Option<String>,
    /// Fade-in length.
    pub fade_in: Option<f64>,
    /// Fade-out length.
    pub fade_out: Option<f64>,
    /// Loop parameters.
    pub loop_settings: Option<LoopSettings>,
    /// Clip content.
    pub content: ClipContent,
}

/// Loop settings for a clip.
#[derive(Debug, Clone, Copy)]
pub struct LoopSettings {
    pub loop_start: f64,
    pub loop_end: f64,
    pub play_start: f64,
}

/// Content inside a clip.
#[derive(Debug, Clone)]
pub enum ClipContent {
    /// Audio file reference.
    Audio(AudioContent),
    /// MIDI notes.
    Notes(Vec<Note>),
    /// Empty/unknown.
    Empty,
}

/// An audio file reference inside a clip.
#[derive(Debug, Clone)]
pub struct AudioContent {
    /// Path to the audio file (relative to the archive or absolute).
    pub path: Option<String>,
    /// Whether the file is embedded inside the archive.
    pub embedded: bool,
    /// Sample rate of the audio file.
    pub sample_rate: Option<u32>,
    /// Number of channels.
    pub channels: Option<u32>,
    /// Duration in samples.
    pub duration: Option<u64>,
    /// Algorithm for time-stretching.
    pub algorithm: Option<String>,
    /// Warp/time-stretch anchors.
    pub warps: Vec<Warp>,
}

/// A warp anchor mapping content time to arrangement time.
#[derive(Debug, Clone, Copy)]
pub struct Warp {
    /// Time in the arrangement (beats or seconds).
    pub time: f64,
    /// Corresponding time in the content (beats or seconds).
    pub content_time: f64,
}

/// A MIDI note.
#[derive(Debug, Clone, Copy)]
pub struct Note {
    /// Time position relative to clip start.
    pub time: f64,
    /// Note duration.
    pub duration: f64,
    /// MIDI channel (0-15).
    pub channel: u8,
    /// MIDI note number (0-127).
    pub key: u8,
    /// Velocity (0.0-1.0).
    pub velocity: f64,
    /// Release velocity (0.0-1.0).
    pub release_velocity: Option<f64>,
}

// ─── Automation ──────────────────────────────────────────────────────────────

/// An automation lane targeting a specific parameter.
#[derive(Debug, Clone)]
pub struct AutomationLane {
    pub id: String,
    /// ID of the parameter being automated.
    pub target: String,
    /// Automation point data.
    pub points: Vec<AutomationPoint>,
}

/// A single automation point.
#[derive(Debug, Clone, Copy)]
pub struct AutomationPoint {
    /// Time position.
    pub time: f64,
    /// Parameter value.
    pub value: f64,
    /// Interpolation mode to the next point.
    pub interpolation: Interpolation,
}

/// Interpolation mode between automation points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Hold,
    Linear,
}

impl Interpolation {
    pub fn from_str(s: &str) -> Self {
        match s {
            "linear" => Self::Linear,
            _ => Self::Hold,
        }
    }
}

// ─── Scenes ──────────────────────────────────────────────────────────────────

/// A scene in the clip launcher.
#[derive(Debug, Clone)]
pub struct Scene {
    pub id: String,
    pub name: Option<String>,
    pub color: Option<String>,
    /// Clip slots in this scene (one per track).
    pub slots: Vec<ClipSlot>,
}

/// A clip slot in the clip launcher.
#[derive(Debug, Clone)]
pub struct ClipSlot {
    pub id: String,
    pub has_stop: bool,
    pub clip: Option<Clip>,
}

// ─── Markers ─────────────────────────────────────────────────────────────────

/// A timeline marker.
#[derive(Debug, Clone)]
pub struct Marker {
    /// Time position.
    pub time: f64,
    /// Marker name.
    pub name: String,
    pub color: Option<String>,
}
