//! Action Registry — High-level client wrapper
//!
//! Provides ergonomic access to the ActionRegistryService for registering
//! custom REAPER actions from guest processes.

use crate::DawClients;
use std::sync::Arc;

/// Handle for registering and querying REAPER actions.
pub struct ActionRegistry {
    clients: Arc<DawClients>,
}

impl ActionRegistry {
    pub(crate) fn new(clients: Arc<DawClients>) -> Self {
        Self { clients }
    }

    /// Register a custom REAPER action.
    ///
    /// Returns the numeric command ID assigned by REAPER, or 0 on failure.
    pub async fn register(&self, command_name: &str, description: &str) -> crate::Result<u32> {
        Ok(self
            .clients
            .action_registry
            .register_action(command_name.to_string(), description.to_string())
            .await?)
    }

    /// Unregister a previously registered action.
    pub async fn unregister(&self, command_name: &str) -> crate::Result<bool> {
        Ok(self
            .clients
            .action_registry
            .unregister_action(command_name.to_string())
            .await?)
    }

    /// Check if a named action is registered in REAPER.
    pub async fn is_registered(&self, command_name: &str) -> crate::Result<bool> {
        Ok(self
            .clients
            .action_registry
            .is_registered(command_name.to_string())
            .await?)
    }

    /// Look up the numeric command ID for a named action.
    pub async fn lookup_command_id(&self, command_name: &str) -> crate::Result<Option<u32>> {
        Ok(self
            .clients
            .action_registry
            .lookup_command_id(command_name.to_string())
            .await?)
    }
}
