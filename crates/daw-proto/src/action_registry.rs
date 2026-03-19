//! Action Registry — Dynamic REAPER Action Registration
//!
//! Allows guest processes to register custom REAPER actions at runtime.
//! Actions appear in REAPER's action list and can be bound to keyboard shortcuts.
//!
//! When a registered action is triggered (by user hotkey, toolbar button, or
//! script), the host notifies the originating guest via a subscription stream.
//! Guests handle the action locally — the host has no knowledge of what signal,
//! session, or sync domains do.
//!
//! # Example (via daw-control)
//!
//! ```rust,ignore
//! let actions = daw.action_registry();
//!
//! // Register a custom action
//! let cmd_id = actions.register("FTS_SIGNAL_ARM", "FTS: Arm Signal Chain").await?;
//!
//! // Subscribe to action triggers
//! let (tx, rx) = daw_control::channel();
//! actions.subscribe_actions(tx).await;
//! while let Some(event) = rx.recv().await {
//!     match event {
//!         ActionEvent::Triggered { command_name } => {
//!             // Handle the action
//!         }
//!     }
//! }
//! ```

use facet::Facet;
use roam::{Tx, service};

/// Events pushed to guests when their registered actions are triggered.
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum ActionEvent {
    /// A registered action was triggered by the user (hotkey, toolbar, script).
    Triggered {
        /// The command name that was registered (e.g., "FTS_SIGNAL_NEXT_SONG").
        command_name: String,
    },
}

/// Dynamic action registration for guest processes.
///
/// Guest processes use this service to register REAPER actions at runtime.
/// Registered actions appear in REAPER's action list (Actions > Show action list)
/// and can be assigned keyboard shortcuts by the user.
///
/// After registering, call [`subscribe_actions`] to receive trigger events.
/// The guest handles all action logic — the host is domain-agnostic.
#[service]
pub trait ActionRegistryService {
    /// Register a new REAPER action.
    ///
    /// - `command_name`: Unique identifier (e.g., "FTS_SIGNAL_ARM"). Must be
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

    /// Subscribe to action trigger events.
    ///
    /// Streams `ActionEvent::Triggered` whenever a REAPER action registered
    /// through this service is triggered by the user. The guest receives
    /// events for ALL actions registered through this service, not just
    /// its own — filter by `command_name` if needed.
    async fn subscribe_actions(&self, tx: Tx<ActionEvent>);

    /// Execute a native DAW command by numeric ID.
    ///
    /// This maps to `Main_OnCommandEx(command_id, 0, current_project)`.
    /// Use for REAPER built-in actions (e.g., 40044 for play/stop,
    /// 40157 for insert marker, 40306 for insert region).
    async fn execute_command(&self, command_id: u32);

    /// Execute a named action (custom or native).
    ///
    /// Looks up the command by name (e.g., "_FTS_SIGNAL_ARM") and executes it.
    /// Returns `true` if the command was found and executed.
    async fn execute_named_action(&self, command_name: String) -> bool;
}
