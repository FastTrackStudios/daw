//! Project handle

use std::sync::Arc;

use crate::{DawClients, Transport};

/// Project handle - lightweight wrapper around project GUID
///
/// This handle represents a specific DAW project. It stores only the project GUID
/// and provides methods to access project subsystems (transport, tracks, etc.).
///
/// Like reaper-rs, this handle is lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// println!("Project GUID: {}", project.guid());
///
/// // Access transport
/// project.transport().play().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Project {
    guid: String,
    clients: Arc<DawClients>,
}

impl Project {
    /// Create a new project handle
    pub(crate) fn new(guid: String, clients: Arc<DawClients>) -> Self {
        Self { guid, clients }
    }

    /// Get the project GUID
    pub fn guid(&self) -> &str {
        &self.guid
    }

    /// Get transport accessor for this project
    ///
    /// Returns a handle to control and monitor the transport (playback, recording, etc.)
    /// for this specific project.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// // Control transport
    /// project.transport().play().await?;
    /// project.transport().stop().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transport(&self) -> Transport {
        Transport::new(self.guid.clone(), self.clients.clone())
    }
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project").field("guid", &self.guid).finish()
    }
}
