//! In-process DAW service registration for CLAP plugins.
//!
//! Creates a `RoutedHandler` with all REAPER service implementations.
//! Used by `PluginHost` to create a local `Daw` instance without SHM.

use daw_proto::{
    ActionRegistryServiceDispatcher, AudioEngineServiceDispatcher, ExtStateServiceDispatcher,
    FxServiceDispatcher, HealthServiceDispatcher, InputServiceDispatcher, ItemServiceDispatcher,
    LiveMidiServiceDispatcher, MarkerServiceDispatcher, MidiAnalysisServiceDispatcher,
    MidiServiceDispatcher, PluginLoaderServiceDispatcher, ProjectServiceDispatcher,
    RegionServiceDispatcher, RoutingServiceDispatcher, TakeServiceDispatcher,
    TempoMapServiceDispatcher, ToolbarServiceDispatcher, TrackServiceDispatcher,
    TransportServiceDispatcher,
};

use daw_proto::{
    action_registry_service_service_descriptor, audio_engine_service_service_descriptor,
    ext_state_service_service_descriptor, fx_service_service_descriptor,
    health_service_service_descriptor, input_service_service_descriptor,
    item_service_service_descriptor, live_midi_service_service_descriptor,
    marker_service_service_descriptor, midi_analysis_service_service_descriptor,
    midi_service_service_descriptor, plugin_loader_service_service_descriptor,
    project_service_service_descriptor, region_service_service_descriptor,
    routing_service_service_descriptor, take_service_service_descriptor,
    tempo_map_service_service_descriptor, toolbar_service_service_descriptor,
    track_service_service_descriptor, transport_service_service_descriptor,
};

use crate::routed_handler::RoutedHandler;

/// Create a `RoutedHandler` with all REAPER DAW service implementations.
///
/// This is the same set of services that daw-bridge registers, but
/// returned as a handler for use with `LocalCaller` (in-process).
///
/// Call after REAPER API is initialized (`PluginHost::init` handles this).
pub fn create_daw_handler() -> RoutedHandler {
    // Initialize broadcasters (idempotent if already done by daw-bridge)
    crate::init_transport_broadcaster();
    crate::init_fx_broadcaster();
    crate::init_track_broadcaster();
    crate::init_item_broadcaster();
    crate::init_routing_broadcaster();
    crate::init_tempo_map_broadcaster();

    // Create REAPER implementations
    let transport = crate::ReaperTransport::new();
    let project = crate::ReaperProject::new();
    let marker = crate::ReaperMarker::new();
    let region = crate::ReaperRegion::new();
    let tempo_map = crate::ReaperTempoMap::new();
    let audio_engine = crate::ReaperAudioEngine::new();
    let midi = crate::ReaperMidi::new();
    let midi_analysis = crate::ReaperMidiAnalysis::new();
    let fx = crate::ReaperFx::new();
    let track = crate::ReaperTrack::new();
    let routing = crate::ReaperRouting::new();
    let live_midi = crate::ReaperLiveMidi::new();
    let ext_state = crate::ReaperExtState::new();
    let item = crate::ReaperItem::new();
    let take = crate::ReaperTake::new();
    let health = crate::ReaperHealth::new();
    let action_registry = crate::ReaperActionRegistry::new();
    let input = crate::ReaperInput::new();
    let toolbar = crate::ReaperToolbar::new();
    let plugin_loader = crate::ReaperPluginLoader::new();

    RoutedHandler::new()
        .with(
            transport_service_service_descriptor(),
            TransportServiceDispatcher::new(transport),
        )
        .with(
            project_service_service_descriptor(),
            ProjectServiceDispatcher::new(project),
        )
        .with(
            marker_service_service_descriptor(),
            MarkerServiceDispatcher::new(marker),
        )
        .with(
            region_service_service_descriptor(),
            RegionServiceDispatcher::new(region),
        )
        .with(
            tempo_map_service_service_descriptor(),
            TempoMapServiceDispatcher::new(tempo_map),
        )
        .with(
            audio_engine_service_service_descriptor(),
            AudioEngineServiceDispatcher::new(audio_engine),
        )
        .with(
            midi_service_service_descriptor(),
            MidiServiceDispatcher::new(midi),
        )
        .with(
            midi_analysis_service_service_descriptor(),
            MidiAnalysisServiceDispatcher::new(midi_analysis),
        )
        .with(
            fx_service_service_descriptor(),
            FxServiceDispatcher::new(fx),
        )
        .with(
            track_service_service_descriptor(),
            TrackServiceDispatcher::new(track),
        )
        .with(
            routing_service_service_descriptor(),
            RoutingServiceDispatcher::new(routing),
        )
        .with(
            live_midi_service_service_descriptor(),
            LiveMidiServiceDispatcher::new(live_midi),
        )
        .with(
            ext_state_service_service_descriptor(),
            ExtStateServiceDispatcher::new(ext_state),
        )
        .with(
            health_service_service_descriptor(),
            HealthServiceDispatcher::new(health),
        )
        .with(
            item_service_service_descriptor(),
            ItemServiceDispatcher::new(item),
        )
        .with(
            take_service_service_descriptor(),
            TakeServiceDispatcher::new(take),
        )
        .with(
            action_registry_service_service_descriptor(),
            ActionRegistryServiceDispatcher::new(action_registry),
        )
        .with(
            input_service_service_descriptor(),
            InputServiceDispatcher::new(input),
        )
        .with(
            toolbar_service_service_descriptor(),
            ToolbarServiceDispatcher::new(toolbar),
        )
        .with(
            plugin_loader_service_service_descriptor(),
            PluginLoaderServiceDispatcher::new(plugin_loader),
        )
}
