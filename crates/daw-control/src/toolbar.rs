//! Toolbar — Dynamic toolbar button management
//!
//! Client-side handle for adding, updating, and removing toolbar buttons
//! in the host DAW.

use crate::DawClients;
use std::sync::Arc;

/// Handle for managing toolbar buttons in the host DAW.
pub struct Toolbar {
    clients: Arc<DawClients>,
}

impl Toolbar {
    pub(crate) fn new(clients: Arc<DawClients>) -> Self {
        Self { clients }
    }

    /// Add a toolbar button. Returns the resolved command ID on success.
    pub async fn add_button(
        &self,
        button: daw_proto::ToolbarButton,
        workflow_id: &str,
    ) -> crate::Result<daw_proto::ToolbarResult> {
        Ok(self
            .clients
            .toolbar
            .add_button(button, workflow_id.to_string())
            .await?)
    }

    /// Update a toolbar button (or add if not present).
    pub async fn update_button(
        &self,
        button: daw_proto::ToolbarButton,
        workflow_id: &str,
    ) -> crate::Result<daw_proto::ToolbarResult> {
        Ok(self
            .clients
            .toolbar
            .update_button(button, workflow_id.to_string())
            .await?)
    }

    /// Remove a single toolbar button.
    pub async fn remove_button(
        &self,
        target: daw_proto::ToolbarTarget,
        command_name: &str,
    ) -> crate::Result<daw_proto::ToolbarResult> {
        Ok(self
            .clients
            .toolbar
            .remove_button(target, command_name.to_string())
            .await?)
    }

    /// Remove all toolbar buttons belonging to a workflow.
    pub async fn remove_workflow_buttons(
        &self,
        workflow_id: &str,
    ) -> crate::Result<daw_proto::ToolbarResult> {
        Ok(self
            .clients
            .toolbar
            .remove_workflow_buttons(workflow_id.to_string())
            .await?)
    }

    /// Check if the dynamic toolbar API is available.
    pub async fn is_available(&self) -> crate::Result<bool> {
        Ok(self.clients.toolbar.is_available().await?)
    }

    /// List all tracked buttons.
    pub async fn get_tracked_buttons(&self) -> crate::Result<Vec<daw_proto::TrackedButton>> {
        Ok(self.clients.toolbar.get_tracked_buttons().await?)
    }
}
