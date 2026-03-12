//! Client API for DAW protocol
//!
//! Provides a reaper-rs style hierarchical API over the flat RPC services.
//! Supports both global singleton usage and per-instance usage for multi-host scenarios.
//!
//! # Single Host (Global)
//!
//! ```no_run
//! use daw_control::Daw;
//!
//! #[tokio::main]
//! async fn main() -> crate::Result<()> {
//!     // Initialize global connection
//!     let handle = roam::connect("unix:///tmp/fts-daw.sock").await?;
//!     Daw::init(handle)?;
//!
//!     // Use the global API
//!     let project = Daw::current_project().await?;
//!     project.transport().play().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Multiple Hosts (Instance-based)
//!
//! ```no_run
//! use daw_control::Daw;
//! use host_manager::HostManager;
//!
//! #[tokio::main]
//! async fn main() -> crate::Result<()> {
//!     let mut manager = HostManager::new();
//!     manager.connect("/tmp/guitar1.sock", 0).await?;
//!     manager.connect("/tmp/guitar2.sock", 0).await?;
//!
//!     // Get host connections
//!     let guitar1 = manager.get_host("guitar:Guitar 1", "guitar1").await?;
//!     let guitar2 = manager.get_host("guitar:Guitar 2", "guitar2").await?;
//!
//!     // Create Daw instances for each host
//!     let daw1 = Daw::new(guitar1.handle().clone());
//!     let daw2 = Daw::new(guitar2.handle().clone());
//!
//!     // Use the same API on each
//!     daw1.current_project().await?.transport().play().await?;
//!     daw2.current_project().await?.transport().play().await?;
//!
//!     Ok(())
//! }
//! ```

// Re-export daw-proto types for convenience
// Note: We selectively re-export to avoid shadowing our local modules (fx, tracks, transport, etc.)
pub use daw_proto::{
    // FX types
    AddFxAtRequest,
    CreateContainerRequest,
    // Error types
    DawError,
    DawResult,
    // Primitives
    Duration,
    EncloseInContainerRequest,
    Fx,
    FxChainContext,
    FxContainerChannelConfig,
    FxError,
    FxEvent,
    FxLatency,
    FxNode,
    FxNodeId,
    FxNodeKind,
    FxParamModulation,
    FxParameter,
    FxRef,
    FxRoutingMode,
    FxTarget,
    InstalledFx,
    FxTree,
    FxType,
    // Marker types
    Marker,
    MarkerError,
    MarkerEvent,
    MidiPosition,
    MoveFromContainerRequest,
    MoveToContainerRequest,
    MusicalPosition,
    // Transport types
    PlayState,
    Position,
    // Project types
    ProjectContext,
    ProjectEvent,
    ProjectInfo,
    RecordMode,
    // Region types
    Region,
    RegionError,
    RegionEvent,
    SetContainerChannelConfigRequest,
    SetNamedConfigRequest,
    SetParameterByNameRequest,
    SetParameterRequest,
    Tempo,
    // Tempo map types
    TempoMapError,
    TempoMapEvent,
    TempoPoint,
    TimePosition,
    TimeRange,
    TimeSignature,
    // Track types
    Track,
    TrackError,
    TrackEvent,
    TrackRef,
    TransportError,
};
// Re-export Transport struct with a different name to avoid conflict with our Transport handle
pub use daw_proto::transport::transport::Transport as TransportState;

use std::sync::Arc;

pub use daw_proto::AudioEngineServiceClient;
pub use daw_proto::AutomationServiceClient;
pub use daw_proto::ExtStateServiceClient;
pub use daw_proto::FxServiceClient;
pub use daw_proto::HealthServiceClient;
pub use daw_proto::ItemServiceClient;
pub use daw_proto::LiveMidiServiceClient;
pub use daw_proto::MarkerServiceClient;
pub use daw_proto::MidiAnalysisServiceClient;
pub use daw_proto::MidiServiceClient;
pub use daw_proto::PositionConversionServiceClient;
pub use daw_proto::ProjectServiceClient;
pub use daw_proto::RegionServiceClient;
pub use daw_proto::RoutingServiceClient;
pub use daw_proto::TakeServiceClient;
pub use daw_proto::TempoMapServiceClient;
pub use daw_proto::TrackServiceClient;
pub use daw_proto::transport::transport::TransportServiceClient;
pub use roam::ErasedCaller;

pub mod error;
pub use error::{Error, Result};

mod automation;
mod ext_state;
mod fx;
mod items;
mod markers;
mod midi_analysis;
mod midi_editor;
mod project;
mod regions;
mod routing;
mod tempo_map;
mod tracks;
mod transport;

pub use self::automation::{EnvelopeHandle, Envelopes};
pub use self::ext_state::ExtState;
pub use self::fx::{FxChain, FxHandle, FxParamHandle};
pub use self::items::{ItemHandle, Items, ProjectItems, TakeHandle, Takes};
pub use self::markers::Markers;
pub use self::midi_analysis::MidiAnalysis;
pub use self::midi_editor::MidiEditor;
pub use self::project::Project;
pub use self::regions::Regions;
pub use self::routing::{HardwareOutputs, Receives, RouteHandle, Sends};
pub use self::tempo_map::TempoMap;
pub use self::tracks::{TrackHandle, Tracks};
pub use self::transport::Transport;

/// Service clients for a DAW connection
#[derive(Clone)]
pub struct DawClients {
    pub(crate) transport: TransportServiceClient,
    pub(crate) project: ProjectServiceClient,
    pub(crate) marker: MarkerServiceClient,
    pub(crate) region: RegionServiceClient,
    pub(crate) tempo_map: TempoMapServiceClient,
    pub(crate) track: TrackServiceClient,
    pub(crate) fx: FxServiceClient,
    pub(crate) position_conversion: PositionConversionServiceClient,
    pub(crate) item: ItemServiceClient,
    pub(crate) take: TakeServiceClient,
    pub(crate) routing: RoutingServiceClient,
    pub(crate) automation: AutomationServiceClient,
    pub(crate) live_midi: LiveMidiServiceClient,
    pub(crate) midi: MidiServiceClient,
    pub(crate) midi_analysis: MidiAnalysisServiceClient,
    pub(crate) audio_engine: AudioEngineServiceClient,
    pub(crate) ext_state: ExtStateServiceClient,
    pub(crate) health: HealthServiceClient,
}

impl DawClients {
    /// Create service clients from a connection handle
    pub fn new(handle: ErasedCaller) -> Self {
        Self {
            transport: TransportServiceClient::new(handle.clone()),
            project: ProjectServiceClient::new(handle.clone()),
            marker: MarkerServiceClient::new(handle.clone()),
            region: RegionServiceClient::new(handle.clone()),
            tempo_map: TempoMapServiceClient::new(handle.clone()),
            track: TrackServiceClient::new(handle.clone()),
            fx: FxServiceClient::new(handle.clone()),
            position_conversion: PositionConversionServiceClient::new(handle.clone()),
            item: ItemServiceClient::new(handle.clone()),
            take: TakeServiceClient::new(handle.clone()),
            routing: RoutingServiceClient::new(handle.clone()),
            automation: AutomationServiceClient::new(handle.clone()),
            live_midi: LiveMidiServiceClient::new(handle.clone()),
            midi: MidiServiceClient::new(handle.clone()),
            midi_analysis: MidiAnalysisServiceClient::new(handle.clone()),
            audio_engine: AudioEngineServiceClient::new(handle.clone()),
            ext_state: ExtStateServiceClient::new(handle.clone()),
            health: HealthServiceClient::new(handle),
        }
    }
}

/// DAW API entry point
///
/// This is the main entry point for the DAW client API. It can be used in two ways:
///
/// 1. **Instance-based**: Create with `Daw::new(handle)` for multi-host scenarios
/// 2. **Global**: Initialize once with `Daw::init(handle)`, then use static methods
///
/// # Example (Instance-based)
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::ErasedCaller) -> crate::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// project.transport().play().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Daw {
    clients: Arc<DawClients>,
}

impl Daw {
    /// Create a new Daw instance from a connection handle.
    ///
    /// Use this for multi-host scenarios where you need separate Daw instances
    /// for each host connection.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(handle: roam::ErasedCaller) -> crate::Result<()> {
    /// let daw = Daw::new(handle);
    /// daw.current_project().await?.transport().play().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(handle: ErasedCaller) -> Self {
        Self {
            clients: Arc::new(DawClients::new(handle)),
        }
    }

    /// Get the current/active project
    ///
    /// Returns the project that is currently focused/active in the DAW.
    ///
    /// # Errors
    ///
    /// Returns an error if no current project is available or RPC fails.
    pub async fn current_project(&self) -> crate::Result<Project> {
        let info = self
            .clients
            .project
            .get_current()
            .await?
            .ok_or_else(|| Error::NoCurrentProject)?;

        Ok(Project::new(info.guid, self.clients.clone()))
    }

    /// Get a specific project by GUID
    ///
    /// # Errors
    ///
    /// Returns an error if the project doesn't exist or RPC fails.
    pub async fn project(&self, guid: impl Into<String>) -> crate::Result<Project> {
        let guid = guid.into();

        // Verify the project exists
        self.clients
            .project
            .get(guid.clone())
            .await?
            .ok_or_else(|| Error::ProjectNotFound(guid.clone()))?;

        Ok(Project::new(guid, self.clients.clone()))
    }

    /// List all open projects
    pub async fn projects(&self) -> crate::Result<Vec<Project>> {
        let infos = self.clients.project.list().await?;

        Ok(infos
            .into_iter()
            .map(|info| Project::new(info.guid, self.clients.clone()))
            .collect())
    }

    /// Select/switch to a specific project by GUID
    ///
    /// Makes the specified project the currently active/focused project.
    /// This is equivalent to switching tabs in a DAW that supports multiple
    /// open projects.
    ///
    /// # Arguments
    ///
    /// * `guid` - The GUID of the project to switch to
    ///
    /// # Returns
    ///
    /// Returns the selected project on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the project doesn't exist or the switch fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(daw: &Daw) -> crate::Result<()> {
    /// // Switch to a specific project
    /// let project = daw.select_project("project-guid-123").await?;
    /// println!("Now on project: {}", project.guid());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn select_project(&self, guid: impl Into<String>) -> crate::Result<Project> {
        let guid = guid.into();

        let success = self.clients.project.select(guid.clone()).await?;

        if success {
            Ok(Project::new(guid, self.clients.clone()))
        } else {
            Err(Error::InvalidOperation(format!("Failed to select project: {}", guid)))
        }
    }

    /// Create a new empty project tab.
    ///
    /// Returns the newly created project handle.
    pub async fn create_project(&self) -> crate::Result<Project> {
        let info = self
            .clients
            .project
            .create()
            .await?
            .ok_or_else(|| Error::InvalidOperation("Failed to create new project".to_string()))?;

        Ok(Project::new(info.guid, self.clients.clone()))
    }

    /// Open a project file (.rpp) in a new tab.
    ///
    /// Uses REAPER's `Main_openProject` API to properly load the project,
    /// avoiding proxy rendering issues that can occur with CLI arguments.
    pub async fn open_project(&self, path: impl Into<String>) -> crate::Result<Project> {
        let info = self
            .clients
            .project
            .open(path.into())
            .await?
            .ok_or_else(|| Error::InvalidOperation("Failed to open project".to_string()))?;

        Ok(Project::new(info.guid, self.clients.clone()))
    }

    /// Close a specific project tab by GUID.
    pub async fn close_project(&self, guid: impl Into<String>) -> crate::Result<()> {
        let guid = guid.into();
        let success = self.clients.project.close(guid.clone()).await?;
        if success {
            Ok(())
        } else {
            Err(Error::InvalidOperation(format!("Failed to close project: {}", guid)))
        }
    }

    /// Get a project by tab slot index (0-based).
    pub async fn project_by_slot(&self, slot: u32) -> crate::Result<Project> {
        let info = self
            .clients
            .project
            .get_by_slot(slot)
            .await?
            .ok_or_else(|| Error::InvalidOperation(format!("No project at slot {}", slot)))?;

        Ok(Project::new(info.guid, self.clients.clone()))
    }

    /// Subscribe to project changes (open, close, switch)
    ///
    /// Returns a receiver that streams project events:
    /// - `ProjectsChanged`: Full list of open projects
    /// - `CurrentChanged`: Active project changed
    /// - `Opened`: A project was opened
    /// - `Closed`: A project was closed
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(daw: &Daw) -> crate::Result<()> {
    /// let mut rx = daw.subscribe_projects().await?;
    /// while let Ok(Some(event)) = rx.recv().await {
    ///     match event {
    ///         daw_control::ProjectEvent::CurrentChanged(guid) => {
    ///             println!("Current project: {:?}", guid);
    ///         }
    ///         daw_control::ProjectEvent::ProjectsChanged(projects) => {
    ///             println!("Projects: {} open", projects.len());
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe_projects(&self) -> crate::Result<roam::Rx<ProjectEvent>> {
        let (tx, rx) = roam::channel::<ProjectEvent>();
        self.clients.project.subscribe(tx).await?;
        Ok(rx)
    }

    /// Get a handle to the audio engine service
    ///
    /// The audio engine provides access to global audio device state
    /// including latency information useful for synchronization.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(daw: &Daw) -> crate::Result<()> {
    /// let latency = daw.audio_engine().get_output_latency_seconds().await?;
    /// println!("Audio output latency: {}ms", latency * 1000.0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn audio_engine(&self) -> &AudioEngineServiceClient {
        &self.clients.audio_engine
    }

    /// Get a handle to the persistent key-value storage (ExtState).
    ///
    /// This provides access to REAPER's ExtState API for storing and retrieving
    /// persistent values scoped by section and key.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(daw: &Daw) -> crate::Result<()> {
    /// let ext = daw.ext_state();
    /// ext.set("MyExt", "theme", "dark", true).await?;
    /// let theme = ext.get("MyExt", "theme").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn ext_state(&self) -> ExtState {
        ExtState::new(self.clients.clone())
    }

    /// List all installed FX plugins in the DAW.
    ///
    /// Returns every plugin known to REAPER (VST2, VST3, CLAP, AU, JS, etc.)
    /// with its display name and full identifier string.
    pub async fn installed_plugins(&self) -> crate::Result<Vec<InstalledFx>> {
        Ok(self.clients.fx.list_installed_fx().await?)
    }

    /// Lightweight health check — pings the DAW and returns `true` if reachable.
    ///
    /// Returns `false` if the RPC fails (connection dead). Used by the
    /// health-check loop in `daw_registry` for fast disconnect detection.
    pub async fn healthcheck(&self) -> bool {
        self.clients.health.ping().await.unwrap_or(false)
    }

    /// Inject a MIDI message into REAPER's virtual keyboard queue.
    ///
    /// Messages reach armed tracks whose record input is set to MIDI VKB.
    /// Use `StuffMidiTarget::VirtualMidiKeyboard` (default) for most cases.
    pub async fn stuff_midi(
        &self,
        target: daw_proto::StuffMidiTarget,
        message: daw_proto::MidiMessage,
    ) -> crate::Result<()> {
        self.clients
            .live_midi
            .stuff_midi_message(target, message)
            .await?;
        Ok(())
    }
}

// ============================================================================
// Global singleton support (for backwards compatibility / single-host usage)
// ============================================================================

use std::sync::OnceLock;

static GLOBAL_DAW: OnceLock<Daw> = OnceLock::new();

impl Daw {
    /// Initialize the global DAW connection (for single-host usage)
    ///
    /// This must be called once at startup before using the global static methods.
    ///
    /// # Errors
    ///
    /// Returns an error if already initialized.
    pub fn init(handle: ErasedCaller) -> crate::Result<()> {
        GLOBAL_DAW
            .set(Daw::new(handle))
            .map_err(|_| Error::InvalidOperation("DAW already initialized".to_string()))
    }

    /// Get the global DAW instance
    ///
    /// # Panics
    ///
    /// Panics if `init()` has not been called.
    pub fn get() -> &'static Daw {
        GLOBAL_DAW
            .get()
            .expect("DAW not initialized. Call Daw::init() first.")
    }

    /// Try to get the global DAW instance without panicking.
    ///
    /// Returns `None` if `init()` has not been called yet.
    /// Useful for gracefully handling the case where DAW is not yet initialized.
    pub fn try_get() -> Option<&'static Daw> {
        GLOBAL_DAW.get()
    }

    /// Check if the DAW has been initialized.
    pub fn is_initialized() -> bool {
        GLOBAL_DAW.get().is_some()
    }
}

