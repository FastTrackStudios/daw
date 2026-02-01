//! Transport handle and operations

use std::sync::Arc;

use crate::DawClients;
use eyre::Result;

/// Transport handle for a specific project
///
/// This handle provides access to transport control (play, stop) for
/// a specific project. Like reaper-rs, it's lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let transport = project.transport();
///
/// // Control playback
/// transport.play().await?;
/// transport.stop().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Transport {
    project_id: String,
    clients: Arc<DawClients>,
}

impl Transport {
    /// Create a new transport handle for a project
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    /// Play this project's transport
    ///
    /// Starts playback from the current playhead position.
    ///
    /// # Errors
    ///
    /// Returns an error if RPC communication fails.
    pub async fn play(&self) -> Result<()> {
        self.clients
            .transport
            .play(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }

    /// Stop this project's transport
    ///
    /// Stops playback and maintains the playhead position.
    pub async fn stop(&self) -> Result<()> {
        self.clients
            .transport
            .stop(Some(self.project_id.clone()))
            .await?;
        Ok(())
    }
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("project_id", &self.project_id)
            .finish()
    }
}
