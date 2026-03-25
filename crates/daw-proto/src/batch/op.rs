//! Batch operation enums — one variant per service method (excluding streaming/UI/lifecycle).

use super::args::{FxChainArg, ProjectArg, TrackArg};
use crate::*;
use facet::Facet;

/// Top-level batch operation — delegates to per-service sub-enums.
#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum BatchOp {
    Project(ProjectOp),
    Transport(TransportOp),
    Track(TrackOp),
    Fx(FxOp),
    Routing(RoutingOp),
    Item(ItemOp),
    Take(TakeOp),
    Automation(AutomationOp),
    Marker(MarkerOp),
    Region(RegionOp),
    TempoMap(TempoMapOp),
    Midi(MidiOp),
    LiveMidi(LiveMidiOp),
    ExtState(ExtStateOp),
    AudioEngine(AudioEngineOp),
    PositionConversion(PositionConversionOp),
    Health(HealthOp),
    ActionRegistry(ActionRegistryOp),
    Toolbar(ToolbarOp),
    PluginLoader(PluginLoaderOp),
    Peak(PeakOp),
    Resource(ResourceOp),
    AudioAccessor(AudioAccessorOp),
    MidiAnalysis(MidiAnalysisOp),
}

// =============================================================================
// Project operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum ProjectOp {
    GetCurrent,
    Get(String),
    List,
    Select(String),
    Open(String),
    Create,
    Close(String),
    GetBySlot(u32),
    BeginUndoBlock(ProjectArg, String),
    EndUndoBlock(ProjectArg, String, Option<UndoScope>),
    Undo(ProjectArg),
    Redo(ProjectArg),
    LastUndoLabel(ProjectArg),
    LastRedoLabel(ProjectArg),
    RunCommand(ProjectArg, String),
    Save(ProjectArg),
    SaveAll,
    SetRulerLaneName(ProjectArg, u32, String),
    GetRulerLaneName(ProjectArg, u32),
    RulerLaneCount(ProjectArg),
}

// =============================================================================
// Transport operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum TransportOp {
    Play(ProjectArg),
    Pause(ProjectArg),
    Stop(ProjectArg),
    PlayPause(ProjectArg),
    PlayStop(ProjectArg),
    PlayFromLastStartPosition(ProjectArg),
    Record(ProjectArg),
    StopRecording(ProjectArg),
    ToggleRecording(ProjectArg),
    SetPosition(ProjectArg, f64),
    GetPosition(ProjectArg),
    GotoStart(ProjectArg),
    GotoEnd(ProjectArg),
    GetState(ProjectArg),
    GetPlayState(ProjectArg),
    IsPlaying(ProjectArg),
    IsRecording(ProjectArg),
    GetTempo(ProjectArg),
    SetTempo(ProjectArg, f64),
    ToggleLoop(ProjectArg),
    IsLooping(ProjectArg),
    SetLoop(ProjectArg, bool),
    GetPlayrate(ProjectArg),
    SetPlayrate(ProjectArg, f64),
    GetTimeSignature(ProjectArg),
    SetPositionMusical(ProjectArg, i32, i32, i32),
    GotoMeasure(ProjectArg, i32),
}

// =============================================================================
// Track operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum TrackOp {
    GetTracks(ProjectArg),
    GetTrack(ProjectArg, TrackArg),
    TrackCount(ProjectArg),
    GetSelectedTracks(ProjectArg),
    GetMasterTrack(ProjectArg),
    SetMuted(ProjectArg, TrackArg, bool),
    SetSoloed(ProjectArg, TrackArg, bool),
    SetSoloExclusive(ProjectArg, TrackArg),
    ClearAllSolo(ProjectArg),
    SetArmed(ProjectArg, TrackArg, bool),
    SetInputMonitoring(ProjectArg, TrackArg, InputMonitoringMode),
    SetRecordInput(ProjectArg, TrackArg, RecordInput),
    SetVolume(ProjectArg, TrackArg, f64),
    SetPan(ProjectArg, TrackArg, f64),
    SetSelected(ProjectArg, TrackArg, bool),
    SelectExclusive(ProjectArg, TrackArg),
    ClearSelection(ProjectArg),
    MuteAll(ProjectArg),
    UnmuteAll(ProjectArg),
    SetVisibleInTcp(ProjectArg, TrackArg, bool),
    SetVisibleInMixer(ProjectArg, TrackArg, bool),
    AddTrack(ProjectArg, String, Option<u32>),
    RemoveTrack(ProjectArg, TrackArg),
    RenameTrack(ProjectArg, TrackArg, String),
    SetTrackColor(ProjectArg, TrackArg, u32),
    SetTrackChunk(ProjectArg, TrackArg, String),
    GetTrackChunk(ProjectArg, TrackArg),
    SetFolderDepth(ProjectArg, TrackArg, i32),
    SetNumChannels(ProjectArg, TrackArg, u32),
    RemoveAllTracks(ProjectArg),
    MoveTrack(ProjectArg, TrackArg, u32),
    ApplyHierarchy(ProjectArg, TrackHierarchy),
    GetExtState(ProjectArg, TrackArg, TrackExtStateRequest),
    SetExtState(ProjectArg, TrackArg, TrackExtStateRequest),
    DeleteExtState(ProjectArg, TrackArg, TrackExtStateRequest),
}

// =============================================================================
// FX operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum FxOp {
    ListInstalledFx,
    GetLastTouchedFx,
    GetFxList(ProjectArg, FxChainArg),
    GetFx(ProjectArg, FxChainArg, FxRef),
    FxCount(ProjectArg, FxChainArg),
    SetFxEnabled(ProjectArg, FxChainArg, FxRef, bool),
    SetFxOffline(ProjectArg, FxChainArg, FxRef, bool),
    AddFx(ProjectArg, FxChainArg, String),
    AddFxAt(ProjectArg, AddFxAtRequest),
    RemoveFx(ProjectArg, FxChainArg, FxRef),
    MoveFx(ProjectArg, FxChainArg, FxRef, u32),
    GetParameters(ProjectArg, FxChainArg, FxRef),
    GetParameter(ProjectArg, FxChainArg, FxRef, u32),
    SetParameter(ProjectArg, SetParameterRequest),
    GetParameterByName(ProjectArg, FxChainArg, FxRef, String),
    SetParameterByName(ProjectArg, SetParameterByNameRequest),
    GetPresetIndex(ProjectArg, FxChainArg, FxRef),
    NextPreset(ProjectArg, FxChainArg, FxRef),
    PrevPreset(ProjectArg, FxChainArg, FxRef),
    SetPreset(ProjectArg, FxChainArg, FxRef, u32),
    OpenFxUi(ProjectArg, FxChainArg, FxRef),
    CloseFxUi(ProjectArg, FxChainArg, FxRef),
    ToggleFxUi(ProjectArg, FxChainArg, FxRef),
    GetNamedConfig(ProjectArg, FxChainArg, FxRef, String),
    SetNamedConfig(ProjectArg, SetNamedConfigRequest),
    GetFxLatency(ProjectArg, FxChainArg, FxRef),
    GetParamModulation(ProjectArg, FxChainArg, FxRef, u32),
    GetFxChannelConfig(ProjectArg, FxChainArg, FxRef),
    SetFxChannelConfig(ProjectArg, FxChainArg, FxRef, FxChannelConfig),
    SilenceFxOutput(ProjectArg, FxChainArg, FxRef),
    RestoreFxOutput(ProjectArg, FxChainArg, FxRef, FxPinMappings),
    GetFxStateChunk(ProjectArg, FxChainArg, FxRef),
    SetFxStateChunk(ProjectArg, FxChainArg, FxRef, Vec<u8>),
    GetFxStateChunkEncoded(ProjectArg, FxChainArg, FxRef),
    SetFxStateChunkEncoded(ProjectArg, FxChainArg, FxRef, String),
    GetFxChainState(ProjectArg, FxChainArg),
    SetFxChainState(ProjectArg, FxChainArg, Vec<FxStateChunk>),
    GetFxTree(ProjectArg, FxChainArg),
    CreateContainer(ProjectArg, CreateContainerRequest),
    MoveToContainer(ProjectArg, MoveToContainerRequest),
    MoveFromContainer(ProjectArg, MoveFromContainerRequest),
    SetRoutingMode(ProjectArg, FxChainArg, FxNodeId, FxRoutingMode),
    GetContainerChannelConfig(ProjectArg, FxChainArg, FxNodeId),
    SetContainerChannelConfig(ProjectArg, SetContainerChannelConfigRequest),
    EncloseInContainer(ProjectArg, EncloseInContainerRequest),
    ExplodeContainer(ProjectArg, FxChainArg, FxNodeId),
    RenameContainer(ProjectArg, FxChainArg, FxNodeId, String),
    GetFxChainChunkText(ProjectArg, FxChainArg),
    InsertFxChainChunk(ProjectArg, FxChainArg, String),
}

// =============================================================================
// Routing operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum RoutingOp {
    GetSends(ProjectArg, TrackArg),
    GetReceives(ProjectArg, TrackArg),
    GetHardwareOutputs(ProjectArg, TrackArg),
    GetRoute(ProjectArg, RouteLocation),
    AddSend(ProjectArg, TrackArg, TrackArg),
    AddHardwareOutput(ProjectArg, TrackArg, u32),
    RemoveRoute(ProjectArg, RouteLocation),
    SetVolume(ProjectArg, RouteLocation, f64),
    SetPan(ProjectArg, RouteLocation, f64),
    SetMuted(ProjectArg, RouteLocation, bool),
    SetMono(ProjectArg, RouteLocation, bool),
    SetPhase(ProjectArg, RouteLocation, bool),
    SetSendMode(ProjectArg, TrackArg, RouteRef, SendMode),
    SetSourceChannels(ProjectArg, RouteLocation, u32, u32),
    SetDestChannels(ProjectArg, RouteLocation, u32, u32),
    GetParentSendEnabled(ProjectArg, TrackArg),
    SetParentSendEnabled(ProjectArg, TrackArg, bool),
}

// =============================================================================
// Item operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum ItemOp {
    GetItems(ProjectArg, TrackArg),
    GetItem(ProjectArg, ItemRef),
    GetAllItems(ProjectArg),
    GetSelectedItems(ProjectArg),
    ItemCount(ProjectArg, TrackArg),
    AddItem(ProjectArg, TrackArg, PositionInSeconds, Duration),
    DeleteItem(ProjectArg, ItemRef),
    DuplicateItem(ProjectArg, ItemRef),
    SetPosition(ProjectArg, ItemRef, PositionInSeconds),
    SetLength(ProjectArg, ItemRef, Duration),
    MoveToTrack(ProjectArg, ItemRef, TrackArg),
    SetSnapOffset(ProjectArg, ItemRef, Duration),
    SetMuted(ProjectArg, ItemRef, bool),
    SetSelected(ProjectArg, ItemRef, bool),
    SetLocked(ProjectArg, ItemRef, bool),
    SelectAllItems(ProjectArg, bool),
    SetVolume(ProjectArg, ItemRef, f64),
    SetFadeIn(ProjectArg, ItemRef, Duration, FadeShape),
    SetFadeOut(ProjectArg, ItemRef, Duration, FadeShape),
    SetLoopSource(ProjectArg, ItemRef, bool),
    SetBeatAttachMode(ProjectArg, ItemRef, BeatAttachMode),
    SetAutoStretch(ProjectArg, ItemRef, bool),
    SetColor(ProjectArg, ItemRef, Option<u32>),
    SetGroupId(ProjectArg, ItemRef, Option<u32>),
}

// =============================================================================
// Take operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum TakeOp {
    GetTakes(ProjectArg, ItemRef),
    GetTake(ProjectArg, ItemRef, TakeRef),
    GetActiveTake(ProjectArg, ItemRef),
    TakeCount(ProjectArg, ItemRef),
    AddTake(ProjectArg, ItemRef),
    DeleteTake(ProjectArg, ItemRef, TakeRef),
    SetActiveTake(ProjectArg, ItemRef, TakeRef),
    SetName(ProjectArg, ItemRef, TakeRef, String),
    SetColor(ProjectArg, ItemRef, TakeRef, Option<u32>),
    SetVolume(ProjectArg, ItemRef, TakeRef, f64),
    SetPlayRate(ProjectArg, ItemRef, TakeRef, f64),
    SetPitch(ProjectArg, ItemRef, TakeRef, f64),
    SetPreservePitch(ProjectArg, ItemRef, TakeRef, bool),
    SetStartOffset(ProjectArg, ItemRef, TakeRef, Duration),
    SetSourceFile(ProjectArg, ItemRef, TakeRef, String),
    GetSourceType(ProjectArg, ItemRef, TakeRef),
}

// =============================================================================
// Automation operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum AutomationOp {
    GetEnvelopes(ProjectArg, TrackArg),
    GetEnvelope(ProjectArg, EnvelopeLocation),
    SetVisible(ProjectArg, EnvelopeLocation, bool),
    SetArmed(ProjectArg, EnvelopeLocation, bool),
    SetAutomationMode(ProjectArg, EnvelopeLocation, AutomationMode),
    GetPoints(ProjectArg, EnvelopeLocation),
    GetPointsInRange(ProjectArg, EnvelopeLocation, TimeRangeParams),
    GetValueAt(ProjectArg, EnvelopeLocation, PositionInSeconds),
    AddPoint(ProjectArg, EnvelopeLocation, AddPointParams),
    DeletePoint(ProjectArg, EnvelopeLocation, u32),
    SetPoint(ProjectArg, EnvelopeLocation, SetPointParams),
    DeletePointsInRange(ProjectArg, EnvelopeLocation, TimeRangeParams),
    GetGlobalAutomationOverride(ProjectArg),
    SetGlobalAutomationOverride(ProjectArg, Option<AutomationMode>),
}

// =============================================================================
// Marker operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum MarkerOp {
    GetMarkers(ProjectArg),
    GetMarker(ProjectArg, u32),
    GetMarkersInRange(ProjectArg, f64, f64),
    GetNextMarker(ProjectArg, f64),
    GetPreviousMarker(ProjectArg, f64),
    MarkerCount(ProjectArg),
    AddMarker(ProjectArg, f64, String),
    RemoveMarker(ProjectArg, u32),
    MoveMarker(ProjectArg, u32, f64),
    RenameMarker(ProjectArg, u32, String),
    SetMarkerColor(ProjectArg, u32, u32),
    GotoNextMarker(ProjectArg),
    GotoPreviousMarker(ProjectArg),
    GotoMarker(ProjectArg, u32),
    AddMarkerInLane(ProjectArg, f64, String, u32),
    SetMarkerLane(ProjectArg, u32, Option<u32>),
    GetMarkersInLane(ProjectArg, u32),
}

// =============================================================================
// Region operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum RegionOp {
    GetRegions(ProjectArg),
    GetRegion(ProjectArg, u32),
    GetRegionsInRange(ProjectArg, f64, f64),
    GetRegionAt(ProjectArg, f64),
    RegionCount(ProjectArg),
    AddRegion(ProjectArg, f64, f64, String),
    RemoveRegion(ProjectArg, u32),
    SetRegionBounds(ProjectArg, u32, f64, f64),
    RenameRegion(ProjectArg, u32, String),
    SetRegionColor(ProjectArg, u32, u32),
    AddRegionInLane(ProjectArg, AddRegionInLaneRequest),
    SetRegionLane(ProjectArg, u32, Option<u32>),
    GetRegionsInLane(ProjectArg, u32),
    GotoRegionStart(ProjectArg, u32),
    GotoRegionEnd(ProjectArg, u32),
}

// =============================================================================
// Tempo map operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum TempoMapOp {
    GetTempoPoints(ProjectArg),
    GetTempoPoint(ProjectArg, u32),
    TempoPointCount(ProjectArg),
    GetTempoAt(ProjectArg, f64),
    GetTimeSignatureAt(ProjectArg, f64),
    TimeToQn(ProjectArg, f64),
    QnToTime(ProjectArg, f64),
    TimeToMusical(ProjectArg, f64),
    MusicalToTime(ProjectArg, i32, i32, f64),
    AddTempoPoint(ProjectArg, f64, f64),
    RemoveTempoPoint(ProjectArg, u32),
    SetTempoAtPoint(ProjectArg, u32, f64),
    SetTimeSignatureAtPoint(ProjectArg, u32, i32, i32),
    MoveTempoPoint(ProjectArg, u32, f64),
    GetDefaultTempo(ProjectArg),
    SetDefaultTempo(ProjectArg, f64),
    GetDefaultTimeSignature(ProjectArg),
    SetDefaultTimeSignature(ProjectArg, i32, i32),
}

// =============================================================================
// MIDI operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum MidiOp {
    GetNotes(MidiTakeLocation),
    GetNotesInRange(MidiTakeLocation, PpqRange),
    GetSelectedNotes(MidiTakeLocation),
    NoteCount(MidiTakeLocation),
    CreateMidiItem(ProjectArg, TrackArg, f64, f64),
    AddNote(MidiTakeLocation, MidiNoteCreate),
    AddNotes(MidiTakeLocation, Vec<MidiNoteCreate>),
    DeleteNote(MidiTakeLocation, u32),
    DeleteNotes(MidiTakeLocation, Vec<u32>),
    DeleteSelectedNotes(MidiTakeLocation),
    SetNotePitch(MidiTakeLocation, u32, u8),
    SetNoteVelocity(MidiTakeLocation, u32, u8),
    SetNotePosition(MidiTakeLocation, u32, f64),
    SetNoteLength(MidiTakeLocation, u32, f64),
    SetNoteChannel(MidiTakeLocation, u32, u8),
    SetNoteSelected(MidiTakeLocation, u32, bool),
    SetNoteMuted(MidiTakeLocation, u32, bool),
    SelectAllNotes(MidiTakeLocation, bool),
    TransposeNotes(MidiTakeLocation, Vec<u32>, i8),
    QuantizeNotes(MidiTakeLocation, QuantizeParams),
    HumanizeNotes(MidiTakeLocation, HumanizeParams),
    GetCcs(MidiTakeLocation, Option<u8>),
    AddCc(MidiTakeLocation, MidiCCCreate),
    DeleteCc(MidiTakeLocation, u32),
    SetCcValue(MidiTakeLocation, u32, u8),
    GetPitchBends(MidiTakeLocation),
    AddPitchBend(MidiTakeLocation, MidiPitchBendCreate),
    GetProgramChanges(MidiTakeLocation),
    GetSysex(MidiTakeLocation),
}

// =============================================================================
// Live MIDI operations (excluding subscribe_input — streaming)
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum LiveMidiOp {
    GetInputDevices,
    GetOutputDevices,
    GetInputDevice(u32),
    GetOutputDevice(u32),
    OpenInputDevice(u32),
    CloseInputDevice(u32),
    OpenOutputDevice(u32),
    CloseOutputDevice(u32),
    SendMidi(u32, MidiMessage, SendMidiTiming),
    SendMidiBatch(u32, Vec<LiveMidiEvent>),
    StuffMidiMessage(StuffMidiTarget, MidiMessage),
}

// =============================================================================
// ExtState operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum ExtStateOp {
    GetExtState(String, String),
    SetExtState(String, String, String, bool),
    DeleteExtState(String, String, bool),
    HasExtState(String, String),
    GetProjectExtState(ProjectArg, String, String),
    SetProjectExtState(ProjectArg, String, String, String),
    DeleteProjectExtState(ProjectArg, String, String),
    HasProjectExtState(ProjectArg, String, String),
}

// =============================================================================
// Audio engine operations (excluding init/quit — lifecycle)
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum AudioEngineOp {
    GetState,
    GetLatency,
    GetOutputLatencySeconds,
    IsRunning,
    GetAudioInputs,
}

// =============================================================================
// Position conversion operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum PositionConversionOp {
    TimeToBeats(ProjectArg, PositionInSeconds, MeasureMode),
    BeatsToTime(ProjectArg, PositionInBeats, MeasureMode),
    TimeToQuarterNotes(ProjectArg, PositionInSeconds),
    QuarterNotesToTime(ProjectArg, PositionInQuarterNotes),
    QuarterNotesToMeasure(ProjectArg, PositionInQuarterNotes),
    BeatsToQuarterNotes(ProjectArg, PositionInBeats),
    QuarterNotesToBeats(ProjectArg, PositionInQuarterNotes),
}

// =============================================================================
// Health operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum HealthOp {
    Ping,
    ShowConsoleMsg(String),
}

// =============================================================================
// Action registry operations (excluding subscribe_actions — streaming)
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum ActionRegistryOp {
    RegisterAction(String, String, bool, bool),
    UnregisterAction(String),
    IsRegistered(String),
    LookupCommandId(String),
    IsInActionList(String),
    ExecuteCommand(u32),
    ExecuteNamedAction(String),
    SetToggleState(String, bool),
    GetToggleState(String),
}

// =============================================================================
// Toolbar operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum ToolbarOp {
    AddButton(ToolbarButton, String),
    UpdateButton(ToolbarButton, String),
    RemoveButton(ToolbarTarget, String),
    RemoveWorkflowButtons(String),
    IsAvailable,
    GetTrackedButtons,
}

// =============================================================================
// Plugin loader operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum PluginLoaderOp {
    LoadPlugin(String),
    ListLoaded,
    IsLoaded(String),
}

// =============================================================================
// Peak operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum PeakOp {
    GetTrackPeak(ProjectArg, TrackArg, u32),
    GetTakePeaks(ProjectArg, ItemRef, TakeRef, u32),
}

// =============================================================================
// Resource operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum ResourceOp {
    GetResourcePath,
    GetIniFilePath,
    GetColorThemePath,
}

// =============================================================================
// Audio accessor operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum AudioAccessorOp {
    CreateTrackAccessor(ProjectArg, TrackArg),
    CreateTakeAccessor(ProjectArg, ItemRef, TakeRef),
    HasStateChanged(String),
    GetSamples(GetSamplesRequest),
    DestroyAccessor(String),
}

// =============================================================================
// MIDI analysis operations
// =============================================================================

#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum MidiAnalysisOp {
    SourceFingerprint(MidiChartRequest),
    GenerateChartData(MidiChartRequest),
}

// =============================================================================
// Helper: Extract step references from ops (for dependency tracking)
// =============================================================================

impl BatchOp {
    /// Returns all step indices that this op depends on via `FromStep` references.
    pub fn step_dependencies(&self) -> Vec<u32> {
        let mut deps = Vec::new();
        self.collect_project_arg_deps(&mut deps);
        self.collect_track_arg_deps(&mut deps);
        self.collect_fx_chain_arg_deps(&mut deps);
        deps.sort_unstable();
        deps.dedup();
        deps
    }

    fn collect_project_arg_deps(&self, deps: &mut Vec<u32>) {
        // Visitor pattern — scan each sub-enum for ProjectArg::FromStep
        macro_rules! check_project {
            ($arg:expr) => {
                if let ProjectArg::FromStep(n) = $arg {
                    deps.push(*n);
                }
            };
        }

        match self {
            // Project ops
            BatchOp::Project(op) => match op {
                ProjectOp::BeginUndoBlock(p, _)
                | ProjectOp::Save(p)
                | ProjectOp::Undo(p)
                | ProjectOp::Redo(p)
                | ProjectOp::LastUndoLabel(p)
                | ProjectOp::LastRedoLabel(p)
                | ProjectOp::RulerLaneCount(p) => check_project!(p),
                ProjectOp::EndUndoBlock(p, _, _)
                | ProjectOp::RunCommand(p, _)
                | ProjectOp::SetRulerLaneName(p, _, _)
                | ProjectOp::GetRulerLaneName(p, _) => check_project!(p),
                _ => {}
            },
            // Transport ops
            BatchOp::Transport(op) => {
                let p = match op {
                    TransportOp::Play(p)
                    | TransportOp::Pause(p)
                    | TransportOp::Stop(p)
                    | TransportOp::PlayPause(p)
                    | TransportOp::PlayStop(p)
                    | TransportOp::PlayFromLastStartPosition(p)
                    | TransportOp::Record(p)
                    | TransportOp::StopRecording(p)
                    | TransportOp::ToggleRecording(p)
                    | TransportOp::GetPosition(p)
                    | TransportOp::GotoStart(p)
                    | TransportOp::GotoEnd(p)
                    | TransportOp::GetState(p)
                    | TransportOp::GetPlayState(p)
                    | TransportOp::IsPlaying(p)
                    | TransportOp::IsRecording(p)
                    | TransportOp::GetTempo(p)
                    | TransportOp::ToggleLoop(p)
                    | TransportOp::IsLooping(p)
                    | TransportOp::GetPlayrate(p)
                    | TransportOp::GetTimeSignature(p) => p,
                    TransportOp::SetPosition(p, _)
                    | TransportOp::SetTempo(p, _)
                    | TransportOp::SetLoop(p, _)
                    | TransportOp::SetPlayrate(p, _) => p,
                    TransportOp::SetPositionMusical(p, _, _, _)
                    | TransportOp::GotoMeasure(p, _) => p,
                };
                check_project!(p);
            }
            // Track ops
            BatchOp::Track(op) => {
                let p = track_op_project_arg(op);
                check_project!(p);
            }
            // FX ops
            BatchOp::Fx(op) => {
                if let Some(p) = fx_op_project_arg(op) {
                    check_project!(p);
                }
            }
            // Routing ops
            BatchOp::Routing(op) => {
                let p = routing_op_project_arg(op);
                check_project!(p);
            }
            // Item ops
            BatchOp::Item(op) => {
                let p = item_op_project_arg(op);
                check_project!(p);
            }
            // Take ops
            BatchOp::Take(op) => {
                let p = take_op_project_arg(op);
                check_project!(p);
            }
            // Automation ops
            BatchOp::Automation(op) => {
                let p = automation_op_project_arg(op);
                check_project!(p);
            }
            // Marker ops
            BatchOp::Marker(op) => {
                let p = marker_op_project_arg(op);
                check_project!(p);
            }
            // Region ops
            BatchOp::Region(op) => {
                let p = region_op_project_arg(op);
                check_project!(p);
            }
            // TempoMap ops
            BatchOp::TempoMap(op) => {
                let p = tempo_map_op_project_arg(op);
                check_project!(p);
            }
            // Midi ops - use MidiTakeLocation (literal), only CreateMidiItem has ProjectArg
            BatchOp::Midi(op) => {
                if let MidiOp::CreateMidiItem(p, _, _, _) = op {
                    check_project!(p);
                }
            }
            // ExtState project variants
            BatchOp::ExtState(op) => match op {
                ExtStateOp::GetProjectExtState(p, _, _)
                | ExtStateOp::SetProjectExtState(p, _, _, _)
                | ExtStateOp::DeleteProjectExtState(p, _, _)
                | ExtStateOp::HasProjectExtState(p, _, _) => check_project!(p),
                _ => {}
            },
            // Peak ops
            BatchOp::Peak(op) => match op {
                PeakOp::GetTrackPeak(p, _, _) | PeakOp::GetTakePeaks(p, _, _, _) => {
                    check_project!(p)
                }
            },
            // AudioAccessor ops
            BatchOp::AudioAccessor(op) => match op {
                AudioAccessorOp::CreateTrackAccessor(p, _)
                | AudioAccessorOp::CreateTakeAccessor(p, _, _) => check_project!(p),
                _ => {}
            },
            // PositionConversion ops
            BatchOp::PositionConversion(op) => {
                let p = position_conversion_op_project_arg(op);
                check_project!(p);
            }
            // No project args in these
            BatchOp::LiveMidi(_)
            | BatchOp::AudioEngine(_)
            | BatchOp::Health(_)
            | BatchOp::ActionRegistry(_)
            | BatchOp::Toolbar(_)
            | BatchOp::PluginLoader(_)
            | BatchOp::Resource(_)
            | BatchOp::MidiAnalysis(_) => {}
        }
    }

    fn collect_track_arg_deps(&self, deps: &mut Vec<u32>) {
        macro_rules! check_track {
            ($arg:expr) => {
                match $arg {
                    TrackArg::FromStep(n) => deps.push(*n),
                    TrackArg::FromStepIndex(n, _) => deps.push(*n),
                    TrackArg::Literal(_) => {}
                }
            };
        }

        match self {
            BatchOp::Track(op) => {
                if let Some(t) = track_op_track_arg(op) {
                    check_track!(t);
                }
            }
            BatchOp::Routing(op) => {
                for t in routing_op_track_args(op) {
                    check_track!(t);
                }
            }
            BatchOp::Item(op) => {
                if let Some(t) = item_op_track_arg(op) {
                    check_track!(t);
                }
            }
            BatchOp::Automation(op) => {
                if let Some(t) = automation_op_track_arg(op) {
                    check_track!(t);
                }
            }
            BatchOp::Midi(op) => {
                if let MidiOp::CreateMidiItem(_, t, _, _) = op {
                    check_track!(t);
                }
            }
            BatchOp::Peak(op) => {
                if let PeakOp::GetTrackPeak(_, t, _) = op {
                    check_track!(t);
                }
            }
            BatchOp::AudioAccessor(op) => {
                if let AudioAccessorOp::CreateTrackAccessor(_, t) = op {
                    check_track!(t);
                }
            }
            _ => {}
        }
    }

    fn collect_fx_chain_arg_deps(&self, deps: &mut Vec<u32>) {
        if let BatchOp::Fx(op) = self {
            if let Some(c) = fx_op_chain_arg(op) {
                if let FxChainArg::TrackFromStep(n) = c {
                    deps.push(*n);
                }
            }
        }
    }
}

// Helper functions to extract ProjectArg from various op enums

fn track_op_project_arg(op: &TrackOp) -> &ProjectArg {
    match op {
        TrackOp::GetTracks(p)
        | TrackOp::TrackCount(p)
        | TrackOp::GetSelectedTracks(p)
        | TrackOp::GetMasterTrack(p)
        | TrackOp::ClearAllSolo(p)
        | TrackOp::ClearSelection(p)
        | TrackOp::MuteAll(p)
        | TrackOp::UnmuteAll(p)
        | TrackOp::RemoveAllTracks(p) => p,
        TrackOp::GetTrack(p, _)
        | TrackOp::SetMuted(p, _, _)
        | TrackOp::SetSoloed(p, _, _)
        | TrackOp::SetSoloExclusive(p, _)
        | TrackOp::SetArmed(p, _, _)
        | TrackOp::SetInputMonitoring(p, _, _)
        | TrackOp::SetRecordInput(p, _, _)
        | TrackOp::SetVolume(p, _, _)
        | TrackOp::SetPan(p, _, _)
        | TrackOp::SetSelected(p, _, _)
        | TrackOp::SelectExclusive(p, _)
        | TrackOp::SetVisibleInTcp(p, _, _)
        | TrackOp::SetVisibleInMixer(p, _, _)
        | TrackOp::RemoveTrack(p, _)
        | TrackOp::RenameTrack(p, _, _)
        | TrackOp::SetTrackColor(p, _, _)
        | TrackOp::SetTrackChunk(p, _, _)
        | TrackOp::GetTrackChunk(p, _)
        | TrackOp::SetFolderDepth(p, _, _)
        | TrackOp::SetNumChannels(p, _, _)
        | TrackOp::MoveTrack(p, _, _)
        | TrackOp::GetExtState(p, _, _)
        | TrackOp::SetExtState(p, _, _)
        | TrackOp::DeleteExtState(p, _, _) => p,
        TrackOp::AddTrack(p, _, _) | TrackOp::ApplyHierarchy(p, _) => p,
    }
}

fn track_op_track_arg(op: &TrackOp) -> Option<&TrackArg> {
    match op {
        TrackOp::GetTrack(_, t)
        | TrackOp::SetMuted(_, t, _)
        | TrackOp::SetSoloed(_, t, _)
        | TrackOp::SetSoloExclusive(_, t)
        | TrackOp::SetArmed(_, t, _)
        | TrackOp::SetInputMonitoring(_, t, _)
        | TrackOp::SetRecordInput(_, t, _)
        | TrackOp::SetVolume(_, t, _)
        | TrackOp::SetPan(_, t, _)
        | TrackOp::SetSelected(_, t, _)
        | TrackOp::SelectExclusive(_, t)
        | TrackOp::SetVisibleInTcp(_, t, _)
        | TrackOp::SetVisibleInMixer(_, t, _)
        | TrackOp::RemoveTrack(_, t)
        | TrackOp::RenameTrack(_, t, _)
        | TrackOp::SetTrackColor(_, t, _)
        | TrackOp::SetTrackChunk(_, t, _)
        | TrackOp::GetTrackChunk(_, t)
        | TrackOp::SetFolderDepth(_, t, _)
        | TrackOp::SetNumChannels(_, t, _)
        | TrackOp::MoveTrack(_, t, _)
        | TrackOp::GetExtState(_, t, _)
        | TrackOp::SetExtState(_, t, _)
        | TrackOp::DeleteExtState(_, t, _) => Some(t),
        _ => None,
    }
}

fn fx_op_project_arg(op: &FxOp) -> Option<&ProjectArg> {
    match op {
        FxOp::ListInstalledFx | FxOp::GetLastTouchedFx => None,
        FxOp::GetFxList(p, _)
        | FxOp::FxCount(p, _)
        | FxOp::GetFxChainState(p, _)
        | FxOp::SetFxChainState(p, _, _)
        | FxOp::GetFxTree(p, _)
        | FxOp::GetFxChainChunkText(p, _)
        | FxOp::InsertFxChainChunk(p, _, _) => Some(p),
        FxOp::GetFx(p, _, _)
        | FxOp::SetFxEnabled(p, _, _, _)
        | FxOp::SetFxOffline(p, _, _, _)
        | FxOp::AddFx(p, _, _)
        | FxOp::RemoveFx(p, _, _)
        | FxOp::MoveFx(p, _, _, _)
        | FxOp::GetParameters(p, _, _)
        | FxOp::GetParameter(p, _, _, _)
        | FxOp::GetParameterByName(p, _, _, _)
        | FxOp::GetPresetIndex(p, _, _)
        | FxOp::NextPreset(p, _, _)
        | FxOp::PrevPreset(p, _, _)
        | FxOp::SetPreset(p, _, _, _)
        | FxOp::OpenFxUi(p, _, _)
        | FxOp::CloseFxUi(p, _, _)
        | FxOp::ToggleFxUi(p, _, _)
        | FxOp::GetNamedConfig(p, _, _, _)
        | FxOp::GetFxLatency(p, _, _)
        | FxOp::GetParamModulation(p, _, _, _)
        | FxOp::GetFxChannelConfig(p, _, _)
        | FxOp::SetFxChannelConfig(p, _, _, _)
        | FxOp::SilenceFxOutput(p, _, _)
        | FxOp::RestoreFxOutput(p, _, _, _)
        | FxOp::GetFxStateChunk(p, _, _)
        | FxOp::SetFxStateChunk(p, _, _, _)
        | FxOp::GetFxStateChunkEncoded(p, _, _)
        | FxOp::SetFxStateChunkEncoded(p, _, _, _)
        | FxOp::SetRoutingMode(p, _, _, _)
        | FxOp::GetContainerChannelConfig(p, _, _)
        | FxOp::ExplodeContainer(p, _, _)
        | FxOp::RenameContainer(p, _, _, _) => Some(p),
        FxOp::AddFxAt(p, _)
        | FxOp::SetParameter(p, _)
        | FxOp::SetParameterByName(p, _)
        | FxOp::SetNamedConfig(p, _)
        | FxOp::CreateContainer(p, _)
        | FxOp::MoveToContainer(p, _)
        | FxOp::MoveFromContainer(p, _)
        | FxOp::SetContainerChannelConfig(p, _)
        | FxOp::EncloseInContainer(p, _) => Some(p),
    }
}

fn fx_op_chain_arg(op: &FxOp) -> Option<&FxChainArg> {
    match op {
        FxOp::ListInstalledFx | FxOp::GetLastTouchedFx => None,
        FxOp::GetFxList(_, c)
        | FxOp::FxCount(_, c)
        | FxOp::GetFxChainState(_, c)
        | FxOp::GetFxTree(_, c)
        | FxOp::GetFxChainChunkText(_, c) => Some(c),
        FxOp::GetFx(_, c, _)
        | FxOp::SetFxEnabled(_, c, _, _)
        | FxOp::SetFxOffline(_, c, _, _)
        | FxOp::AddFx(_, c, _)
        | FxOp::RemoveFx(_, c, _)
        | FxOp::MoveFx(_, c, _, _)
        | FxOp::GetParameters(_, c, _)
        | FxOp::GetParameter(_, c, _, _)
        | FxOp::GetParameterByName(_, c, _, _)
        | FxOp::GetPresetIndex(_, c, _)
        | FxOp::NextPreset(_, c, _)
        | FxOp::PrevPreset(_, c, _)
        | FxOp::SetPreset(_, c, _, _)
        | FxOp::OpenFxUi(_, c, _)
        | FxOp::CloseFxUi(_, c, _)
        | FxOp::ToggleFxUi(_, c, _)
        | FxOp::GetNamedConfig(_, c, _, _)
        | FxOp::GetFxLatency(_, c, _)
        | FxOp::GetParamModulation(_, c, _, _)
        | FxOp::GetFxChannelConfig(_, c, _)
        | FxOp::SetFxChannelConfig(_, c, _, _)
        | FxOp::SilenceFxOutput(_, c, _)
        | FxOp::RestoreFxOutput(_, c, _, _)
        | FxOp::GetFxStateChunk(_, c, _)
        | FxOp::SetFxStateChunk(_, c, _, _)
        | FxOp::GetFxStateChunkEncoded(_, c, _)
        | FxOp::SetFxStateChunkEncoded(_, c, _, _)
        | FxOp::SetFxChainState(_, c, _)
        | FxOp::InsertFxChainChunk(_, c, _)
        | FxOp::SetRoutingMode(_, c, _, _)
        | FxOp::GetContainerChannelConfig(_, c, _)
        | FxOp::ExplodeContainer(_, c, _)
        | FxOp::RenameContainer(_, c, _, _) => Some(c),
        // These use request structs that contain the chain context
        FxOp::AddFxAt(_, _)
        | FxOp::SetParameter(_, _)
        | FxOp::SetParameterByName(_, _)
        | FxOp::SetNamedConfig(_, _)
        | FxOp::CreateContainer(_, _)
        | FxOp::MoveToContainer(_, _)
        | FxOp::MoveFromContainer(_, _)
        | FxOp::SetContainerChannelConfig(_, _)
        | FxOp::EncloseInContainer(_, _) => None,
    }
}

fn routing_op_project_arg(op: &RoutingOp) -> &ProjectArg {
    match op {
        RoutingOp::GetSends(p, _)
        | RoutingOp::GetReceives(p, _)
        | RoutingOp::GetHardwareOutputs(p, _)
        | RoutingOp::GetRoute(p, _)
        | RoutingOp::AddSend(p, _, _)
        | RoutingOp::AddHardwareOutput(p, _, _)
        | RoutingOp::RemoveRoute(p, _)
        | RoutingOp::SetVolume(p, _, _)
        | RoutingOp::SetPan(p, _, _)
        | RoutingOp::SetMuted(p, _, _)
        | RoutingOp::SetMono(p, _, _)
        | RoutingOp::SetPhase(p, _, _)
        | RoutingOp::SetSendMode(p, _, _, _)
        | RoutingOp::SetSourceChannels(p, _, _, _)
        | RoutingOp::SetDestChannels(p, _, _, _)
        | RoutingOp::GetParentSendEnabled(p, _)
        | RoutingOp::SetParentSendEnabled(p, _, _) => p,
    }
}

fn routing_op_track_args(op: &RoutingOp) -> Vec<&TrackArg> {
    match op {
        RoutingOp::GetSends(_, t)
        | RoutingOp::GetReceives(_, t)
        | RoutingOp::GetHardwareOutputs(_, t)
        | RoutingOp::AddHardwareOutput(_, t, _)
        | RoutingOp::SetSendMode(_, t, _, _)
        | RoutingOp::GetParentSendEnabled(_, t)
        | RoutingOp::SetParentSendEnabled(_, t, _) => vec![t],
        RoutingOp::AddSend(_, src, dst) => vec![src, dst],
        // RouteLocation contains TrackRef (literal), not TrackArg
        RoutingOp::GetRoute(_, _)
        | RoutingOp::RemoveRoute(_, _)
        | RoutingOp::SetVolume(_, _, _)
        | RoutingOp::SetPan(_, _, _)
        | RoutingOp::SetMuted(_, _, _)
        | RoutingOp::SetMono(_, _, _)
        | RoutingOp::SetPhase(_, _, _)
        | RoutingOp::SetSourceChannels(_, _, _, _)
        | RoutingOp::SetDestChannels(_, _, _, _) => vec![],
    }
}

fn item_op_project_arg(op: &ItemOp) -> &ProjectArg {
    match op {
        ItemOp::GetItems(p, _)
        | ItemOp::GetItem(p, _)
        | ItemOp::GetAllItems(p)
        | ItemOp::GetSelectedItems(p)
        | ItemOp::ItemCount(p, _)
        | ItemOp::AddItem(p, _, _, _)
        | ItemOp::DeleteItem(p, _)
        | ItemOp::DuplicateItem(p, _)
        | ItemOp::SetPosition(p, _, _)
        | ItemOp::SetLength(p, _, _)
        | ItemOp::MoveToTrack(p, _, _)
        | ItemOp::SetSnapOffset(p, _, _)
        | ItemOp::SetMuted(p, _, _)
        | ItemOp::SetSelected(p, _, _)
        | ItemOp::SetLocked(p, _, _)
        | ItemOp::SelectAllItems(p, _)
        | ItemOp::SetVolume(p, _, _)
        | ItemOp::SetFadeIn(p, _, _, _)
        | ItemOp::SetFadeOut(p, _, _, _)
        | ItemOp::SetLoopSource(p, _, _)
        | ItemOp::SetBeatAttachMode(p, _, _)
        | ItemOp::SetAutoStretch(p, _, _)
        | ItemOp::SetColor(p, _, _)
        | ItemOp::SetGroupId(p, _, _) => p,
    }
}

fn item_op_track_arg(op: &ItemOp) -> Option<&TrackArg> {
    match op {
        ItemOp::GetItems(_, t) | ItemOp::ItemCount(_, t) | ItemOp::AddItem(_, t, _, _) => Some(t),
        ItemOp::MoveToTrack(_, _, t) => Some(t),
        _ => None,
    }
}

fn take_op_project_arg(op: &TakeOp) -> &ProjectArg {
    match op {
        TakeOp::GetTakes(p, _)
        | TakeOp::GetTake(p, _, _)
        | TakeOp::GetActiveTake(p, _)
        | TakeOp::TakeCount(p, _)
        | TakeOp::AddTake(p, _)
        | TakeOp::DeleteTake(p, _, _)
        | TakeOp::SetActiveTake(p, _, _)
        | TakeOp::SetName(p, _, _, _)
        | TakeOp::SetColor(p, _, _, _)
        | TakeOp::SetVolume(p, _, _, _)
        | TakeOp::SetPlayRate(p, _, _, _)
        | TakeOp::SetPitch(p, _, _, _)
        | TakeOp::SetPreservePitch(p, _, _, _)
        | TakeOp::SetStartOffset(p, _, _, _)
        | TakeOp::SetSourceFile(p, _, _, _)
        | TakeOp::GetSourceType(p, _, _) => p,
    }
}

fn automation_op_project_arg(op: &AutomationOp) -> &ProjectArg {
    match op {
        AutomationOp::GetEnvelopes(p, _)
        | AutomationOp::GetEnvelope(p, _)
        | AutomationOp::SetVisible(p, _, _)
        | AutomationOp::SetArmed(p, _, _)
        | AutomationOp::SetAutomationMode(p, _, _)
        | AutomationOp::GetPoints(p, _)
        | AutomationOp::GetPointsInRange(p, _, _)
        | AutomationOp::GetValueAt(p, _, _)
        | AutomationOp::AddPoint(p, _, _)
        | AutomationOp::DeletePoint(p, _, _)
        | AutomationOp::SetPoint(p, _, _)
        | AutomationOp::DeletePointsInRange(p, _, _)
        | AutomationOp::GetGlobalAutomationOverride(p)
        | AutomationOp::SetGlobalAutomationOverride(p, _) => p,
    }
}

fn automation_op_track_arg(op: &AutomationOp) -> Option<&TrackArg> {
    match op {
        AutomationOp::GetEnvelopes(_, t) => Some(t),
        _ => None,
    }
}

fn marker_op_project_arg(op: &MarkerOp) -> &ProjectArg {
    match op {
        MarkerOp::GetMarkers(p)
        | MarkerOp::GetMarker(p, _)
        | MarkerOp::GetMarkersInRange(p, _, _)
        | MarkerOp::GetNextMarker(p, _)
        | MarkerOp::GetPreviousMarker(p, _)
        | MarkerOp::MarkerCount(p)
        | MarkerOp::AddMarker(p, _, _)
        | MarkerOp::RemoveMarker(p, _)
        | MarkerOp::MoveMarker(p, _, _)
        | MarkerOp::RenameMarker(p, _, _)
        | MarkerOp::SetMarkerColor(p, _, _)
        | MarkerOp::GotoNextMarker(p)
        | MarkerOp::GotoPreviousMarker(p)
        | MarkerOp::GotoMarker(p, _)
        | MarkerOp::AddMarkerInLane(p, _, _, _)
        | MarkerOp::SetMarkerLane(p, _, _)
        | MarkerOp::GetMarkersInLane(p, _) => p,
    }
}

fn region_op_project_arg(op: &RegionOp) -> &ProjectArg {
    match op {
        RegionOp::GetRegions(p)
        | RegionOp::GetRegion(p, _)
        | RegionOp::GetRegionsInRange(p, _, _)
        | RegionOp::GetRegionAt(p, _)
        | RegionOp::RegionCount(p)
        | RegionOp::AddRegion(p, _, _, _)
        | RegionOp::RemoveRegion(p, _)
        | RegionOp::SetRegionBounds(p, _, _, _)
        | RegionOp::RenameRegion(p, _, _)
        | RegionOp::SetRegionColor(p, _, _)
        | RegionOp::AddRegionInLane(p, _)
        | RegionOp::SetRegionLane(p, _, _)
        | RegionOp::GetRegionsInLane(p, _)
        | RegionOp::GotoRegionStart(p, _)
        | RegionOp::GotoRegionEnd(p, _) => p,
    }
}

fn tempo_map_op_project_arg(op: &TempoMapOp) -> &ProjectArg {
    match op {
        TempoMapOp::GetTempoPoints(p)
        | TempoMapOp::GetTempoPoint(p, _)
        | TempoMapOp::TempoPointCount(p)
        | TempoMapOp::GetTempoAt(p, _)
        | TempoMapOp::GetTimeSignatureAt(p, _)
        | TempoMapOp::TimeToQn(p, _)
        | TempoMapOp::QnToTime(p, _)
        | TempoMapOp::TimeToMusical(p, _)
        | TempoMapOp::MusicalToTime(p, _, _, _)
        | TempoMapOp::AddTempoPoint(p, _, _)
        | TempoMapOp::RemoveTempoPoint(p, _)
        | TempoMapOp::SetTempoAtPoint(p, _, _)
        | TempoMapOp::SetTimeSignatureAtPoint(p, _, _, _)
        | TempoMapOp::MoveTempoPoint(p, _, _)
        | TempoMapOp::GetDefaultTempo(p)
        | TempoMapOp::SetDefaultTempo(p, _)
        | TempoMapOp::GetDefaultTimeSignature(p)
        | TempoMapOp::SetDefaultTimeSignature(p, _, _) => p,
    }
}

fn position_conversion_op_project_arg(op: &PositionConversionOp) -> &ProjectArg {
    match op {
        PositionConversionOp::TimeToBeats(p, _, _)
        | PositionConversionOp::BeatsToTime(p, _, _)
        | PositionConversionOp::TimeToQuarterNotes(p, _)
        | PositionConversionOp::QuarterNotesToTime(p, _)
        | PositionConversionOp::QuarterNotesToMeasure(p, _)
        | PositionConversionOp::BeatsToQuarterNotes(p, _)
        | PositionConversionOp::QuarterNotesToBeats(p, _) => p,
    }
}
