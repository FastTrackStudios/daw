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
//! async fn main() -> eyre::Result<()> {
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
//! async fn main() -> eyre::Result<()> {
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
    // Primitives
    Duration,
    Fx,
    FxChainContext,
    FxError,
    FxEvent,
    FxLatency,
    FxParamModulation,
    FxParameter,
    FxRef,
    FxTarget,
    FxType,
    // Marker types
    Marker,
    MarkerError,
    MarkerEvent,
    MidiPosition,
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
pub use daw_proto::FxServiceClient;
pub use daw_proto::ItemServiceClient;
pub use daw_proto::LiveMidiServiceClient;
pub use daw_proto::MarkerServiceClient;
pub use daw_proto::MidiServiceClient;
pub use daw_proto::PositionConversionServiceClient;
pub use daw_proto::ProjectServiceClient;
pub use daw_proto::RegionServiceClient;
pub use daw_proto::RoutingServiceClient;
pub use daw_proto::TakeServiceClient;
pub use daw_proto::TempoMapServiceClient;
pub use daw_proto::TrackServiceClient;
pub use daw_proto::transport::transport::TransportServiceClient;
use roam::session::ConnectionHandle;

mod automation;
mod fx;
mod items;
mod markers;
mod midi_editor;
mod project;
mod regions;
mod routing;
mod tempo_map;
mod tracks;
mod transport;

pub use self::automation::{EnvelopeHandle, Envelopes};
pub use self::fx::{FxChain, FxHandle, FxParamHandle};
pub use self::items::{ItemHandle, Items, ProjectItems, TakeHandle, Takes};
pub use self::markers::Markers;
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
    pub(crate) audio_engine: AudioEngineServiceClient,
}

impl DawClients {
    /// Create service clients from a connection handle
    pub fn new(handle: ConnectionHandle) -> Self {
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
            audio_engine: AudioEngineServiceClient::new(handle),
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
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
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
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// daw.current_project().await?.transport().play().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(handle: ConnectionHandle) -> Self {
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
    pub async fn current_project(&self) -> eyre::Result<Project> {
        let info = self
            .clients
            .project
            .get_current()
            .await?
            .ok_or_else(|| eyre::eyre!("No current project"))?;

        Ok(Project::new(info.guid, self.clients.clone()))
    }

    /// Get a specific project by GUID
    ///
    /// # Errors
    ///
    /// Returns an error if the project doesn't exist or RPC fails.
    pub async fn project(&self, guid: impl Into<String>) -> eyre::Result<Project> {
        let guid = guid.into();

        // Verify the project exists
        self.clients
            .project
            .get(guid.clone())
            .await?
            .ok_or_else(|| eyre::eyre!("Project not found: {}", guid))?;

        Ok(Project::new(guid, self.clients.clone()))
    }

    /// List all open projects
    pub async fn projects(&self) -> eyre::Result<Vec<Project>> {
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
    /// # async fn example(daw: &Daw) -> eyre::Result<()> {
    /// // Switch to a specific project
    /// let project = daw.select_project("project-guid-123").await?;
    /// println!("Now on project: {}", project.guid());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn select_project(&self, guid: impl Into<String>) -> eyre::Result<Project> {
        let guid = guid.into();

        let success = self.clients.project.select(guid.clone()).await?;

        if success {
            Ok(Project::new(guid, self.clients.clone()))
        } else {
            Err(eyre::eyre!("Failed to select project: {}", guid))
        }
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
    /// # async fn example(daw: &Daw) -> eyre::Result<()> {
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
    pub async fn subscribe_projects(&self) -> eyre::Result<roam::Rx<ProjectEvent>> {
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
    /// # async fn example(daw: &Daw) -> eyre::Result<()> {
    /// let latency = daw.audio_engine().get_output_latency_seconds().await?;
    /// println!("Audio output latency: {}ms", latency * 1000.0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn audio_engine(&self) -> &AudioEngineServiceClient {
        &self.clients.audio_engine
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
    pub fn init(handle: ConnectionHandle) -> eyre::Result<()> {
        GLOBAL_DAW
            .set(Daw::new(handle))
            .map_err(|_| eyre::eyre!("DAW already initialized"))
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
