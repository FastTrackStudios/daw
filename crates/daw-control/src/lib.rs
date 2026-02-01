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

use std::sync::Arc;

use daw_proto::ProjectServiceClient;
use daw_proto::transport::transport::TransportServiceClient;
use roam::session::ConnectionHandle;

mod project;
mod transport;

pub use self::project::Project;
pub use self::transport::Transport;

/// Service clients for a DAW connection
#[derive(Clone)]
pub struct DawClients {
    pub(crate) transport: TransportServiceClient,
    pub(crate) project: ProjectServiceClient,
}

impl DawClients {
    /// Create service clients from a connection handle
    pub fn new(handle: ConnectionHandle) -> Self {
        Self {
            transport: TransportServiceClient::new(handle.clone()),
            project: ProjectServiceClient::new(handle),
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
}
