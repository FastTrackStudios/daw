//! Step output types — all possible return value types from batch operations.

use crate::*;
use facet::Facet;
use std::path::PathBuf;

/// All possible return value types from a batch step.
#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum StepOutput {
    /// No return value (unit operations like play, stop, set_muted, etc.)
    Unit,
    /// Boolean result
    Bool(bool),
    /// Unsigned 32-bit integer (marker ID, note index, command ID, etc.)
    U32(u32),
    /// Floating-point result (position, tempo, volume, etc.)
    F64(f64),
    /// Unsigned size (count operations)
    Usize(u64),
    /// String result (GUID, chunk text, ext state value, etc.)
    Str(String),
    /// Optional string
    OptStr(Option<String>),
    /// Vec<u8> result (raw state chunks)
    Bytes(Vec<u8>),
    /// Optional bytes
    OptBytes(Option<Vec<u8>>),
    /// Path result
    Path(PathBuf),
    /// Optional path
    OptPath(Option<PathBuf>),

    // -- Project types --
    /// Single project info
    ProjectInfo(ProjectInfo),
    /// Optional project info
    OptProjectInfo(Option<ProjectInfo>),
    /// List of projects
    ProjectInfoList(Vec<ProjectInfo>),

    // -- Track types --
    /// Single track
    Track(Track),
    /// Optional track
    OptTrack(Option<Track>),
    /// List of tracks
    TrackList(Vec<Track>),

    // -- FX types --
    /// Single FX
    Fx(Fx),
    /// Optional FX
    OptFx(Option<Fx>),
    /// List of FX
    FxList(Vec<Fx>),
    /// FX parameter
    FxParameter(FxParameter),
    /// Optional FX parameter
    OptFxParameter(Option<FxParameter>),
    /// List of FX parameters
    FxParameterList(Vec<FxParameter>),
    /// FX preset index
    OptFxPresetIndex(Option<FxPresetIndex>),
    /// FX latency
    OptFxLatency(Option<FxLatency>),
    /// FX param modulation
    OptFxParamModulation(Option<FxParamModulation>),
    /// FX channel config
    OptFxChannelConfig(Option<FxChannelConfig>),
    /// FX pin mappings (from silence_fx_output)
    FxPinMappings(FxPinMappings),
    /// FX state chunks
    FxStateChunkList(Vec<FxStateChunk>),
    /// FX tree
    FxTree(FxTree),
    /// Optional FX node ID
    OptFxNodeId(Option<FxNodeId>),
    /// Optional FX container channel config
    OptFxContainerChannelConfig(Option<FxContainerChannelConfig>),
    /// Installed FX list
    InstalledFxList(Vec<InstalledFx>),
    /// Last touched FX
    OptLastTouchedFx(Option<LastTouchedFx>),

    // -- Transport types --
    /// Transport state
    Transport(transport::transport::Transport),
    /// Play state
    PlayState(PlayState),
    /// Time signature
    TimeSignature(TimeSignature),

    // -- Routing types --
    /// List of routes
    RouteList(Vec<TrackRoute>),
    /// Optional route
    OptRoute(Option<TrackRoute>),
    /// Optional route index (from add_send/add_hardware_output)
    OptU32(Option<u32>),

    // -- Item types --
    /// Single item
    Item(Item),
    /// Optional item
    OptItem(Option<Item>),
    /// List of items
    ItemList(Vec<Item>),

    // -- Take types --
    /// Single take
    Take(Take),
    /// Optional take
    OptTake(Option<Take>),
    /// List of takes
    TakeList(Vec<Take>),
    /// Source type
    SourceType(SourceType),

    // -- Automation types --
    /// Envelope
    Envelope(Envelope),
    /// Optional envelope
    OptEnvelope(Option<Envelope>),
    /// List of envelopes
    EnvelopeList(Vec<Envelope>),
    /// Envelope point list
    EnvelopePointList(Vec<EnvelopePoint>),
    /// Optional automation mode
    OptAutomationMode(Option<AutomationMode>),

    // -- Marker types --
    /// Single marker
    Marker(Marker),
    /// Optional marker
    OptMarker(Option<Marker>),
    /// List of markers
    MarkerList(Vec<Marker>),

    // -- Region types --
    /// Single region
    Region(Region),
    /// Optional region
    OptRegion(Option<Region>),
    /// List of regions
    RegionList(Vec<Region>),

    // -- Tempo map types --
    /// Tempo point
    TempoPoint(TempoPoint),
    /// Optional tempo point
    OptTempoPoint(Option<TempoPoint>),
    /// List of tempo points
    TempoPointList(Vec<TempoPoint>),
    /// Tuple of two i32 (time signature numerator/denominator)
    I32Pair(i32, i32),
    /// Tuple of (i32, i32, f64) for musical position
    MusicalTime(i32, i32, f64),

    // -- MIDI types --
    /// List of MIDI notes
    MidiNoteList(Vec<MidiNote>),
    /// List of u32 (note indices from add_notes)
    U32List(Vec<u32>),
    /// Optional MIDI take location (from create_midi_item)
    OptMidiTakeLocation(Option<MidiTakeLocation>),
    /// List of MIDI CCs
    MidiCCList(Vec<MidiCC>),
    /// List of pitch bends
    MidiPitchBendList(Vec<MidiPitchBend>),
    /// List of program changes
    MidiProgramChangeList(Vec<MidiProgramChange>),
    /// List of sysex events
    MidiSysExList(Vec<MidiSysEx>),

    // -- Live MIDI types --
    /// MIDI input device list
    MidiInputDeviceList(Vec<MidiInputDevice>),
    /// MIDI output device list
    MidiOutputDeviceList(Vec<MidiOutputDevice>),
    /// Optional MIDI input device
    OptMidiInputDevice(Option<MidiInputDevice>),
    /// Optional MIDI output device
    OptMidiOutputDevice(Option<MidiOutputDevice>),

    // -- Audio engine types --
    /// Audio engine state
    AudioEngineState(AudioEngineState),
    /// Audio latency
    AudioLatency(AudioLatency),
    /// Audio input info
    AudioInputInfo(AudioInputInfo),

    // -- Audio accessor types --
    /// Audio sample data
    AudioSampleData(AudioSampleData),

    // -- Peak types --
    /// Track peak
    TrackPeak(TrackPeak),
    /// Take peak data
    TakePeakData(TakePeakData),

    // -- Position conversion types --
    /// Time to beats result
    TimeToBeatsResult(TimeToBeatsResult),
    /// Time to quarter notes result
    TimeToQuarterNotesResult(TimeToQuarterNotesResult),
    /// Quarter notes to measure result
    QuarterNotesToMeasureResult(QuarterNotesToMeasureResult),
    /// Position in seconds
    PositionInSeconds(PositionInSeconds),
    /// Position in beats
    PositionInBeats(PositionInBeats),
    /// Position in quarter notes
    PositionInQuarterNotes(PositionInQuarterNotes),

    // -- Plugin loader types --
    /// Plugin load result
    PluginLoadResult(PluginLoadResult),
    /// Loaded plugin info list
    LoadedPluginInfoList(Vec<LoadedPluginInfo>),

    // -- Action registry types --
    /// Optional bool (toggle state)
    OptBool(Option<bool>),

    // -- Toolbar types --
    /// Toolbar result
    ToolbarResult(ToolbarResult),
    /// Tracked button list
    TrackedButtonList(Vec<TrackedButton>),

    // -- Result types (for operations returning Result<(), String>) --
    /// Result<(), String> — Ok
    ResultOk,
    /// Result<(), String> — Err
    ResultErr(String),
    /// Result<String, String> from operations like get_track_chunk
    ResultStr(Result<String, String>),
    /// Result<FxPinMappings, String> from silence_fx_output
    ResultFxPinMappings(Result<FxPinMappings, String>),
    /// Result<MidiChartData, String>
    ResultMidiChartData(Result<MidiChartData, String>),
    /// Result<String, String> from source_fingerprint
    ResultString(Result<String, String>),

    // -- Input types --
    /// Key filter
    KeyFilter(KeyFilter),

    // -- Midi analysis --
    /// Midi chart data
    MidiChartData(MidiChartData),
}
