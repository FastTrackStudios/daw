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

/// A track (audio or MIDI) with its region assignments.
#[derive(Debug, Clone)]
pub struct Track {
    /// Track name (from the track definition block).
    pub name: String,
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

/// A region placed on a track.
#[derive(Debug, Clone)]
pub struct TrackRegion {
    /// Index into either `audio_regions` or `midi_regions`.
    pub region_index: u16,
    /// Start position override (if the track assignment overrides the region's start).
    pub start_pos: u64,
}

/// The origin tick value for MIDI positions (10^12).
pub const ZERO_TICKS: u64 = 0xe8d4a51000;

/// The "no region assigned" sentinel value.
pub const NO_REGION: u16 = 0xFFFF;
