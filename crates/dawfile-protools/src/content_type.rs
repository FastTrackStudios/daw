//! Pro Tools block content type identifiers.
//!
//! Each block in a Pro Tools session file has a `content_type` field that
//! identifies what kind of data the block contains. These were reverse-engineered
//! from the binary format by the ptformat project.

/// Known content types found in Pro Tools session files.
///
/// Values are the raw `u16` content_type from block headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ContentType {
    // ── Session metadata ────────────────────────────────────────────────
    /// Old-format version info (PT 5-9)
    VersionInfoOld = 0x0003,
    /// Product name and version string
    ProductVersion = 0x0030,
    /// Session sample rate
    SessionSampleRate = 0x1028,
    /// Session path info / new-format version (PT 10+)
    SessionInfo = 0x2067,

    // ── Audio files ─────────────────────────────────────────────────────
    /// WAV sample rate and size
    WavInfo = 0x1001,
    /// WAV metadata container
    WavMetadata = 0x1003,
    /// WAV file list (full)
    WavList = 0x1004,
    /// WAV names sub-list
    WavNames = 0x103a,

    // ── Regions (PT 5-9) ────────────────────────────────────────────────
    /// Region name + number (generic)
    RegionName = 0x1007,
    /// Audio region name + number (v5-9)
    AudioRegionOld = 0x1008,
    /// Audio region list (v5-9)
    AudioRegionListOld = 0x100b,

    // ── Regions (PT 10+) ────────────────────────────────────────────────
    /// Audio region name + number (v10+)
    AudioRegionNew = 0x2629,
    /// Audio region list (v10+)
    AudioRegionListNew = 0x262a,

    // ── Region-to-track mapping (old) ───────────────────────────────────
    /// Region-to-track assignment entry
    RegionTrackEntry = 0x100e,
    /// Audio region-to-track entry
    AudioRegionTrackEntry = 0x100f,
    /// Audio region-to-track map entries
    AudioRegionTrackMapEntries = 0x1011,
    /// Audio region-to-track full map
    AudioRegionTrackMap = 0x1012,

    // ── Region-to-track mapping (v8+) ───────────────────────────────────
    /// Audio region-to-track sub-entry (v8+)
    AudioRegionTrackSubEntryNew = 0x104f,
    /// Audio region-to-track entry (v8+)
    AudioRegionTrackEntryNew = 0x1050,
    /// Audio region-to-track map entries (v8+)
    AudioRegionTrackMapEntriesNew = 0x1052,
    /// Audio region-to-track full map (v8+)
    AudioRegionTrackMapNew = 0x1054,

    // ── Tracks ──────────────────────────────────────────────────────────
    /// Audio track name + number
    AudioTrackInfo = 0x1014,
    /// Audio tracks container
    AudioTrackList = 0x1015,

    // ── FX / Plugins ────────────────────────────────────────────────────
    /// Plugin entry
    PluginEntry = 0x1017,
    /// Plugin full list
    PluginList = 0x1018,

    // ── I/O Routing ─────────────────────────────────────────────────────
    /// I/O channel entry
    IoChannelEntry = 0x1021,
    /// I/O channel list
    IoChannelList = 0x1022,
    /// I/O route entry
    IoRoute = 0x2602,
    /// I/O routing table
    IoRoutingTable = 0x2603,

    // ── MIDI events ─────────────────────────────────────────────────────
    /// MIDI events data block
    MidiEventsBlock = 0x2000,

    // ── MIDI regions (PT 5-9) ───────────────────────────────────────────
    /// MIDI region name + number (v5-9)
    MidiRegionOld = 0x2001,
    /// MIDI regions map (v5-9)
    MidiRegionMapOld = 0x2002,

    // ── MIDI regions (PT 10+) ───────────────────────────────────────────
    /// MIDI region name + number (v10+)
    MidiRegionNew = 0x2633,
    /// MIDI regions map (v10+)
    MidiRegionMapNew = 0x2634,

    // ── MIDI tracks ─────────────────────────────────────────────────────
    /// MIDI track full list
    MidiTrackList = 0x2519,
    /// MIDI track name + number
    MidiTrackInfo = 0x251a,

    // ── MIDI region-to-track ────────────────────────────────────────────
    /// MIDI region-to-track entry
    MidiRegionTrackEntry = 0x1056,
    /// MIDI region-to-track map entries
    MidiRegionTrackMapEntries = 0x1057,
    /// MIDI region-to-track full map
    MidiRegionTrackMap = 0x1058,

    // ── Compound regions ────────────────────────────────────────────────
    /// Compound region element
    CompoundRegionElement = 0x2523,
    /// Compound region group
    CompoundRegionGroup = 0x2628,
    /// Compound MIDI region container
    CompoundMidiRegionContainer = 0x262b,
    /// Compound MIDI region full map
    CompoundMidiRegionMap = 0x262c,

    // ── Alternate playlists ─────────────────────────────────────────────
    /// Alternate playlist map container (wraps a 0x1054 for inactive playlists)
    AlternatePlaylistMap = 0x2428,
    /// Alternate playlist map container, secondary variant
    AlternatePlaylistMapAlt = 0x2429,

    // ── Markers ─────────────────────────────────────────────────────────
    /// Marker list
    MarkerList = 0x271a,

    // ── Snaps ───────────────────────────────────────────────────────────
    /// Snaps block
    SnapsBlock = 0x2511,
}

impl ContentType {
    /// Try to parse a raw u16 into a known content type.
    pub fn from_raw(raw: u16) -> Option<Self> {
        // Safety: We match exhaustively rather than transmute
        match raw {
            0x0003 => Some(Self::VersionInfoOld),
            0x0030 => Some(Self::ProductVersion),
            0x1028 => Some(Self::SessionSampleRate),
            0x2067 => Some(Self::SessionInfo),

            0x1001 => Some(Self::WavInfo),
            0x1003 => Some(Self::WavMetadata),
            0x1004 => Some(Self::WavList),
            0x103a => Some(Self::WavNames),

            0x1007 => Some(Self::RegionName),
            0x1008 => Some(Self::AudioRegionOld),
            0x100b => Some(Self::AudioRegionListOld),
            0x2629 => Some(Self::AudioRegionNew),
            0x262a => Some(Self::AudioRegionListNew),

            0x100e => Some(Self::RegionTrackEntry),
            0x100f => Some(Self::AudioRegionTrackEntry),
            0x1011 => Some(Self::AudioRegionTrackMapEntries),
            0x1012 => Some(Self::AudioRegionTrackMap),
            0x104f => Some(Self::AudioRegionTrackSubEntryNew),
            0x1050 => Some(Self::AudioRegionTrackEntryNew),
            0x1052 => Some(Self::AudioRegionTrackMapEntriesNew),
            0x1054 => Some(Self::AudioRegionTrackMapNew),

            0x1014 => Some(Self::AudioTrackInfo),
            0x1015 => Some(Self::AudioTrackList),

            0x1017 => Some(Self::PluginEntry),
            0x1018 => Some(Self::PluginList),

            0x1021 => Some(Self::IoChannelEntry),
            0x1022 => Some(Self::IoChannelList),
            0x2602 => Some(Self::IoRoute),
            0x2603 => Some(Self::IoRoutingTable),

            0x2000 => Some(Self::MidiEventsBlock),
            0x2001 => Some(Self::MidiRegionOld),
            0x2002 => Some(Self::MidiRegionMapOld),
            0x2633 => Some(Self::MidiRegionNew),
            0x2634 => Some(Self::MidiRegionMapNew),

            0x2519 => Some(Self::MidiTrackList),
            0x251a => Some(Self::MidiTrackInfo),

            0x1056 => Some(Self::MidiRegionTrackEntry),
            0x1057 => Some(Self::MidiRegionTrackMapEntries),
            0x1058 => Some(Self::MidiRegionTrackMap),

            0x2523 => Some(Self::CompoundRegionElement),
            0x2628 => Some(Self::CompoundRegionGroup),
            0x262b => Some(Self::CompoundMidiRegionContainer),
            0x262c => Some(Self::CompoundMidiRegionMap),

            0x2428 => Some(Self::AlternatePlaylistMap),
            0x2429 => Some(Self::AlternatePlaylistMapAlt),

            0x271a => Some(Self::MarkerList),
            0x2511 => Some(Self::SnapsBlock),

            _ => None,
        }
    }

    /// Get the raw u16 value.
    pub fn as_raw(self) -> u16 {
        self as u16
    }
}

impl core::fmt::Display for ContentType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?} (0x{:04x})", self.as_raw())
    }
}
