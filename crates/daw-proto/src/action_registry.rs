//! Action Registry — Dynamic REAPER Action Registration
//!
//! Allows guest processes to register custom REAPER actions at runtime.
//! Actions appear in REAPER's action list and can be bound to keyboard shortcuts.
//!
//! When a registered action is triggered (by user hotkey, toolbar button, or
//! script), the host notifies the originating guest via a subscription stream.
//!
//! # Example (via daw-control)
//!
//! ```rust,ignore
//! let actions = daw.action_registry();
//!
//! // Register a custom action
//! let cmd_id = actions.register("fts.signal.arm", "FTS: Arm Signal Chain").await?;
//!
//! // Check if an action exists
//! let exists = actions.is_registered("fts.signal.arm").await?;
//!
//! // Look up a command ID by name
//! let id = actions.lookup("fts.signal.arm").await?;
//! ```

use roam::service;

/// Dynamic action registration for guest processes.
///
/// Guest processes use this service to register REAPER actions at runtime.
/// Registered actions appear in REAPER's action list (Actions > Show action list)
/// and can be assigned keyboard shortcuts by the user.
#[service]
pub trait ActionRegistryService {
    /// Register a new REAPER action.
    ///
    /// - `command_name`: Unique identifier (e.g., "fts.signal.arm"). Must be
    ///   globally unique across all extensions.
    /// - `description`: Human-readable label shown in REAPER's action list.
    ///
    /// Returns the numeric command ID assigned by REAPER, or 0 on failure.
    async fn register_action(&self, command_name: String, description: String) -> u32;

    /// Unregister a previously registered action.
    ///
    /// Returns `true` if the action was found and unregistered.
    async fn unregister_action(&self, command_name: String) -> bool;

    /// Check if an action is registered (either by us or any other extension).
    ///
    /// Uses REAPER's `NamedCommandLookup` — works for any registered command,
    /// not just ones registered through this service.
    async fn is_registered(&self, command_name: String) -> bool;

    /// Look up the numeric command ID for a named action.
    ///
    /// Returns `None` if the action is not registered.
    async fn lookup_command_id(&self, command_name: String) -> Option<u32>;
}
