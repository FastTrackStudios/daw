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
    /// The action will NOT appear in the Extensions menu — use
    /// `register_in_menu` if you want it visible in menus.
    pub async fn register(&self, command_name: &str, description: &str) -> crate::Result<u32> {
        Ok(self
            .clients
            .action_registry
            .register_action(command_name.to_string(), description.to_string(), false, false)
            .await?)
    }

    /// Register a custom REAPER action and show it in the Extensions menu.
    ///
    /// Returns the numeric command ID assigned by REAPER, or 0 on failure.
    /// The menu hierarchy is derived from the command name prefix
    /// (e.g., `FTS_SESSION_*` → Session submenu).
    pub async fn register_in_menu(
        &self,
        command_name: &str,
        description: &str,
    ) -> crate::Result<u32> {
        Ok(self
            .clients
            .action_registry
            .register_action(command_name.to_string(), description.to_string(), true, false)
            .await?)
    }

    /// Register a toggleable REAPER action (on/off state shown in action list).
    ///
    /// Returns the numeric command ID assigned by REAPER, or 0 on failure.
    /// Use [`set_toggle_state`] to update the on/off indicator.
    pub async fn register_toggle(
        &self,
        command_name: &str,
        description: &str,
    ) -> crate::Result<u32> {
        Ok(self
            .clients
            .action_registry
            .register_action(command_name.to_string(), description.to_string(), false, true)
            .await?)
    }

    /// Register a toggleable REAPER action and show it in the Extensions menu.
    pub async fn register_toggle_in_menu(
        &self,
        command_name: &str,
        description: &str,
    ) -> crate::Result<u32> {
        Ok(self
            .clients
            .action_registry
            .register_action(command_name.to_string(), description.to_string(), true, true)
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

    /// Check if an action is actually in REAPER's action list (main section).
    ///
    /// Unlike `is_registered` which only checks the command ID registry,
    /// this verifies the action has a gaccel entry and appears in
    /// Actions > Show action list.
    pub async fn is_in_action_list(&self, command_name: &str) -> crate::Result<bool> {
        Ok(self
            .clients
            .action_registry
            .is_in_action_list(command_name.to_string())
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

    /// Subscribe to action trigger events.
    ///
    /// Returns a stream of `ActionEvent::Triggered` events whenever a REAPER
    /// action registered through this service is triggered by the user.
    pub async fn subscribe_actions(&self) -> crate::Result<roam::Rx<daw_proto::ActionEvent>> {
        let (tx, rx) = roam::channel::<daw_proto::ActionEvent>();
        self.clients.action_registry.subscribe_actions(tx).await?;
        Ok(rx)
    }

    /// Execute a native DAW command by numeric ID.
    ///
    /// Maps to `Main_OnCommandEx(command_id, 0, current_project)` in REAPER.
    pub async fn execute_command(&self, command_id: u32) -> crate::Result<()> {
        self.clients
            .action_registry
            .execute_command(command_id)
            .await?;
        Ok(())
    }

    /// Execute a named action (custom or native).
    ///
    /// Returns `true` if the command was found and executed.
    pub async fn execute_named_action(&self, command_name: &str) -> crate::Result<bool> {
        Ok(self
            .clients
            .action_registry
            .execute_named_action(command_name.to_string())
            .await?)
    }

    /// Set the toggle state for a toggleable action.
    ///
    /// REAPER will show the action as "on" (checked) or "off" in the action list
    /// and toolbar buttons. Has no effect for non-toggleable actions.
    pub async fn set_toggle_state(
        &self,
        command_name: &str,
        is_on: bool,
    ) -> crate::Result<()> {
        self.clients
            .action_registry
            .set_toggle_state(command_name.to_string(), is_on)
            .await?;
        Ok(())
    }

    /// Get the current toggle state for a toggleable action.
    ///
    /// Returns `None` if the action is not registered or not toggleable.
    pub async fn get_toggle_state(&self, command_name: &str) -> crate::Result<Option<bool>> {
        Ok(self
            .clients
            .action_registry
            .get_toggle_state(command_name.to_string())
            .await?)
    }
}
