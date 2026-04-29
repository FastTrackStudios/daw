//! Domain types for Pro Tools session data.
//!
//! These types represent the parsed contents of a Pro Tools session file.
//! They are format-specific (not yet mapped to `daw_proto` types).

/// A parsed Pro Tools session.
#[derive(Debug, Clone)]
pub struct ProToolsSession {
    /// Pro Tools version that created this session (5-12).
    pub version: u16,
    /// Session sample rate as stored in the file.
    pub session_sample_rate: u32,
    /// Initial (or only) BPM of the session.
    pub bpm: f64,
    /// All tempo change events, sorted by position.
    ///
    /// Contains at least one entry (the initial tempo). If the session has
    /// no tempo changes this is a single-element Vec.
    pub tempo_events: Vec<TempoEvent>,
    /// Time-signature change events, sorted by position.
    ///
    /// Empty for sessions with no meter changes (4/4 implicit).
    pub meter_events: Vec<MeterEvent>,
    /// User-defined markers (Pro Tools Memory Locations).
    pub markers: Vec<Marker>,
    /// Audio file references.
    pub audio_files: Vec<AudioFile>,
    /// Audio regions.
    pub audio_regions: Vec<AudioRegion>,
    /// Audio tracks with their region assignments.
    pub audio_tracks: Vec<Track>,
    /// MIDI regions.
    pub midi_regions: Vec<MidiRegion>,
    /// MIDI tracks with their region assignments.
    pub midi_tracks: Vec<Track>,
    /// Session-wide registry of all plugin types used anywhere in the session.
    ///
    /// This is a global list — Pro Tools stores one list for the whole session,
    /// not per-track. The same plugin may appear multiple times if used in
    /// different configurations (e.g., mono vs. stereo instance).
    pub plugins: Vec<PluginEntry>,
    /// I/O channels configured in the session's Hardware Setup and I/O Setup.
    pub io_channels: Vec<IoChannel>,
}

/// A reference to an audio file used in the session.
#[derive(Debug, Clone)]
pub struct AudioFile {
    /// Filename (typically just the stem + extension, e.g. "kick.wav").
    pub filename: String,
    /// Index in the session's audio file list.
    pub index: u16,
    /// Length in samples.
    pub length: u64,
}

/// An audio region on the timeline.
#[derive(Debug, Clone)]
pub struct AudioRegion {
    /// Region name.
    pub name: String,
    /// Index in the session's region list.
    pub index: u16,
    /// Absolute start position on the timeline (in samples at target rate).
    pub start_pos: u64,
    /// Offset into the source audio file (in samples at target rate).
    pub sample_offset: u64,
    /// Length of the region (in samples at target rate).
    pub length: u64,
    /// Index of the source audio file in [`ProToolsSession::audio_files`].
    pub audio_file_index: u16,
}

/// A MIDI event within a MIDI region.
#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    /// Position relative to region start (in ticks).
    pub position: u64,
    /// Note duration (in ticks).
    pub duration: u64,
    /// MIDI note number (0-127).
    pub note: u8,
    /// MIDI velocity (0-127).
    pub velocity: u8,
}

/// A MIDI region on the timeline.
#[derive(Debug, Clone)]
pub struct MidiRegion {
    /// Region name.
    pub name: String,
    /// Index in the session's MIDI region list.
    pub index: u16,
    /// Absolute start position on the timeline (in samples at target rate).
    pub start_pos: u64,
    /// Offset into the source data.
    pub sample_offset: u64,
    /// Length of the region (in samples at target rate).
    pub length: u64,
    /// MIDI events in this region.
    pub events: Vec<MidiEvent>,
}

/// An alternate playlist for a track.
///
/// In Pro Tools, each track can have multiple playlists (named arrangements of
/// regions). The active playlist's regions are in [`Track::regions`]; any
/// inactive alternates are stored here.
#[derive(Debug, Clone)]
pub struct Playlist {
    /// Playlist name (e.g. `"Kick.01"`, `"Kick.02"`).
    pub name: String,
    /// Regions in this playlist.
    pub regions: Vec<TrackRegion>,
}

/// What kind of media a track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    /// Audio track — regions reference [`AudioRegion`] entries.
    Audio,
    /// MIDI track — regions reference [`MidiRegion`] entries.
    Midi,
}

/// A track (audio or MIDI) with its region assignments.
#[derive(Debug, Clone)]
pub struct Track {
    /// Track name (from the track definition block).
    pub name: String,
    /// Whether this track is audio or MIDI.
    pub kind: TrackKind,
    /// Track channel index (ch_map value, used to match region assignments).
    pub index: u16,
    /// Name of the active playlist (from the region-to-track map block).
    ///
    /// For the main playlist this matches the track name; for a comp playlist
    /// this will have a suffix like `.01`.
    pub playlist_name: String,
    /// Regions on the active playlist, in timeline order.
    pub regions: Vec<TrackRegion>,
    /// Alternate (inactive) playlists stored in the session.
    ///
    /// Empty unless the session was saved with alternate playlists.
    pub alternate_playlists: Vec<Playlist>,
}

impl Track {
    /// True if this track has any alternate (comp) playlists in addition to the active one.
    pub fn has_alternate_playlists(&self) -> bool {
        !self.alternate_playlists.is_empty()
    }

    /// Total playlist count (active + alternates).
    pub fn playlist_count(&self) -> usize {
        1 + self.alternate_playlists.len()
    }
}

/// A region placed on a track.
#[derive(Debug, Clone)]
pub struct TrackRegion {
    /// Index into either `audio_regions` or `midi_regions`.
    pub region_index: u16,
    /// Start position override (if the track assignment overrides the region's start).
    pub start_pos: u64,
}

/// A single constant-tempo segment on the session timeline.
#[derive(Debug, Clone)]
pub struct TempoEvent {
    /// Position in ticks (relative to the session start, ZERO_TICKS-based).
    pub tick_start: u64,
    /// Position in samples at the session's target sample rate.
    pub sample_start: u64,
    /// Beats per minute.
    pub bpm: f64,
    /// Ticks per beat (960,000 in all observed sessions).
    pub ticks_per_beat: u64,
}

/// A time-signature change event.
#[derive(Debug, Clone)]
pub struct MeterEvent {
    /// Position in ticks (relative to session start).
    pub tick_start: u64,
    /// Position in samples at the session's target sample rate.
    pub sample_start: u64,
    /// Bar number where this meter begins (1-based).
    pub measure: u32,
    /// Time signature numerator (e.g. 6 in 6/8).
    pub numerator: u32,
    /// Time signature denominator (e.g. 8 in 6/8).
    pub denominator: u32,
}

/// A user-defined marker (Pro Tools Memory Location).
#[derive(Debug, Clone)]
pub struct Marker {
    /// Marker name.
    pub name: String,
    /// 1-based memory location number.
    pub number: u32,
    /// Position in ticks (relative to session start).
    pub tick_pos: u64,
    /// Position in samples at the session's target sample rate.
    pub sample_pos: u64,
}

/// A plugin / insert registered in the session's global plugin list.
///
/// Pro Tools keeps a single session-wide list (block 0x1018) of all plugin
/// types used anywhere in the session.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    /// Insert slot index within a track's insert chain (0-based).
    ///
    /// Note: the same slot index can appear on multiple tracks; this field
    /// identifies the position within a single track's chain, not a unique
    /// session-wide slot ID.
    pub slot_index: u8,
    /// Display name as shown in the Pro Tools insert selector.
    pub name: String,
    /// Manufacturer 4-character code (stored byte-reversed in the file).
    /// E.g., "Digi" for Digidesign/Avid, "Srdx" for SoundRadix.
    pub manufacturer_4cc: [u8; 4],
    /// Plugin type 4CC (e.g., "Rvrb" for reverb).
    pub type_4cc: [u8; 4],
    /// Plugin subtype / variant 4CC.
    pub subtype_4cc: [u8; 4],
    /// Number of input channels (1 = mono, 2 = stereo).
    pub input_channels: u8,
    /// Number of output channels (1 = mono, 2 = stereo).
    pub output_channels: u8,
    /// AAX bundle identifier, e.g. `"com.avid.aax.dverb"`.
    pub aax_bundle_id: String,
}

impl PluginEntry {
    /// Returns the manufacturer code as a printable ASCII string.
    pub fn manufacturer_name(&self) -> String {
        self.manufacturer_4cc
            .iter()
            .map(|&b| if b.is_ascii_graphic() { b as char } else { '?' })
            .collect()
    }
}

/// A hardware or bus I/O channel configured in the session's I/O setup.
#[derive(Debug, Clone)]
pub struct IoChannel {
    /// Channel display name (e.g., "Out 1-2", "MacBook Pro Speakers 1-2").
    pub name: String,
    /// I/O class: 0x01 = physical hardware interface, 0x02 = output bus.
    pub io_class: u8,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channel_count: u8,
}

/// Strongly-typed view of [`IoChannel::io_class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoClass {
    /// Physical hardware interface (`io_class == 0x01`).
    HardwareInterface,
    /// Output bus (`io_class == 0x02`).
    OutputBus,
    /// Any other / unknown I/O class.
    Other(u8),
}

impl IoChannel {
    /// Returns a strongly-typed view of [`Self::io_class`].
    pub fn class(&self) -> IoClass {
        match self.io_class {
            0x01 => IoClass::HardwareInterface,
            0x02 => IoClass::OutputBus,
            other => IoClass::Other(other),
        }
    }
}

impl ProToolsSession {
    /// Iterate over every track in the session, audio first then MIDI.
    pub fn all_tracks(&self) -> impl Iterator<Item = &Track> {
        self.audio_tracks.iter().chain(self.midi_tracks.iter())
    }

    /// Total active region placements across every audio + MIDI track.
    pub fn total_active_region_placements(&self) -> usize {
        self.all_tracks().map(|t| t.regions.len()).sum()
    }

    /// Total alternate-playlist count across every track.
    pub fn total_alternate_playlists(&self) -> usize {
        self.all_tracks().map(|t| t.alternate_playlists.len()).sum()
    }

    /// Look up an audio file by region.
    pub fn audio_file_for(&self, region: &AudioRegion) -> Option<&AudioFile> {
        self.audio_files
            .iter()
            .find(|f| f.index == region.audio_file_index)
    }

    /// Convert a sample position (at the session sample rate) to seconds.
    pub fn samples_to_seconds(&self, samples: u64) -> f64 {
        if self.session_sample_rate == 0 {
            return 0.0;
        }
        samples as f64 / self.session_sample_rate as f64
    }
}

/// The origin tick value for MIDI positions (10^12).
pub const ZERO_TICKS: u64 = 0xe8d4a51000;

/// The "no region assigned" sentinel value.
pub const NO_REGION: u16 = 0xFFFF;
