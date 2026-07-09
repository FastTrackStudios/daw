//! Actions Registry - In-process action management
//!
//! This module provides an in-process registry for actions from cells.
//! It can be used by:
//! - REAPER extension (queries cells, registers with REAPER)
//! - Desktop/web apps (queries cells, exposes via ActionsService)
//!
//! ## Usage
//!
//! ```ignore
//! use actions_registry::ActionsRegistry;
//!
//! // Create registry
//! let registry = ActionsRegistry::new();
//!
//! // Register a cell that provides actions
//! registry.register_cell("session", connection_handle).await;
//!
//! // Get all actions (for command palette)
//! let actions = registry.get_all_actions().await;
//!
//! // Execute an action
//! let result = registry.execute("fts.session.log_hello").await;
//! ```

use actions_proto::{
    ActionDefinition, ActionEvent, ActionId, ActionResult, ActionsService,
    ActionsServiceDispatcher, DefinesActionsClient,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{info, warn};
use vox::Caller;

/// A registered cell with its cached actions and RPC client
struct RegisteredCell {
    /// Cached action definitions from this cell
    actions: Vec<ActionDefinition>,
    /// RPC client to call the cell's DefinesActions service
    client: DefinesActionsClient,
}

/// In-process registry for actions from all connected cells.
///
/// This is designed to run in-process, querying cells via vox RPC
/// and caching their action definitions.
pub struct ActionsRegistry {
    /// Map of source_name -> registered cell data
    cells: RwLock<HashMap<String, RegisteredCell>>,
    /// Broadcast channel for action change events
    event_tx: broadcast::Sender<ActionEvent>,
}

impl ActionsRegistry {
    /// Create a new empty registry.
    pub fn new() -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            cells: RwLock::new(HashMap::new()),
            event_tx,
        })
    }

    /// Register a cell that implements `DefinesActions`.
    ///
    /// This queries the cell for its actions and caches them.
    /// Emits an `ActionEvent::Added` event.
    pub async fn register_cell(&self, cell_name: &str, handle: Caller) {
        let client = DefinesActionsClient::new(handle);

        // Query the cell for its actions
        let actions = match client.get_actions().await {
            Ok(actions) => actions,
            Err(e) => {
                warn!(cell = cell_name, error = ?e, "Failed to get actions from cell");
                return;
            }
        };

        let count = actions.len();

        // Store the cell
        {
            let mut cells = self.cells.write().await;
            cells.insert(
                cell_name.to_string(),
                RegisteredCell {
                    actions: actions.clone(),
                    client,
                },
            );
        }

        // Notify subscribers
        let _ = self.event_tx.send(ActionEvent::Added {
            source: cell_name.to_string(),
            actions,
        });

        info!(cell = cell_name, count, "Registered cell actions");
    }

    /// Unregister a cell and its actions.
    ///
    /// Emits an `ActionEvent::Removed` event.
    pub async fn unregister_cell(&self, cell_name: &str) {
        let removed = {
            let mut cells = self.cells.write().await;
            cells.remove(cell_name)
        };

        if let Some(cell) = removed {
            let action_ids: Vec<ActionId> = cell.actions.iter().map(|a| a.id.clone()).collect();
            let count = action_ids.len();

            // Notify subscribers
            let _ = self.event_tx.send(ActionEvent::Removed {
                source: cell_name.to_string(),
                action_ids,
            });

            info!(cell = cell_name, count, "Unregistered cell actions");
        }
    }

    /// Refresh actions from a cell (re-query and update cache).
    pub async fn refresh_cell(&self, cell_name: &str) {
        let client = {
            let cells = self.cells.read().await;
            cells.get(cell_name).map(|c| c.client.clone())
        };

        if let Some(client) = client {
            match client.get_actions().await {
                Ok(new_actions) => {
                    let mut cells = self.cells.write().await;
                    if let Some(cell) = cells.get_mut(cell_name) {
                        cell.actions = new_actions;
                        info!(cell = cell_name, "Refreshed cell actions");
                    }
                }
                Err(e) => {
                    warn!(cell = cell_name, error = ?e, "Failed to refresh actions");
                }
            }
        }
    }

    /// Get all actions from all registered cells.
    pub async fn get_all_actions(&self) -> Vec<ActionDefinition> {
        let cells = self.cells.read().await;
        cells
            .values()
            .flat_map(|cell| cell.actions.iter().cloned())
            .collect()
    }

    /// Get actions from a specific cell.
    pub async fn get_cell_actions(&self, cell_name: &str) -> Vec<ActionDefinition> {
        let cells = self.cells.read().await;
        cells
            .get(cell_name)
            .map(|c| c.actions.clone())
            .unwrap_or_default()
    }

    /// Find which cell owns an action.
    pub async fn find_action_source(&self, action_id: &ActionId) -> Option<String> {
        let cells = self.cells.read().await;
        for (name, cell) in cells.iter() {
            if cell.actions.iter().any(|a| &a.id == action_id) {
                return Some(name.clone());
            }
        }
        None
    }

    /// Execute an action by ID.
    ///
    /// Routes the execution to the cell that defines the action.
    pub async fn execute(&self, action_id: impl Into<ActionId>) -> ActionResult {
        let action_id = action_id.into();

        // Find which cell owns this action
        let (cell_name, client) = {
            let cells = self.cells.read().await;
            let mut found = None;
            for (name, cell) in cells.iter() {
                if cell.actions.iter().any(|a| a.id == action_id) {
                    found = Some((name.clone(), cell.client.clone()));
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => {
                    warn!(action_id = %action_id, "Action not found");
                    return ActionResult::failure(format!("Action not found: {}", action_id));
                }
            }
        };

        // Execute via RPC
        info!(action_id = %action_id, cell = %cell_name, "Executing action");
        match client.execute_action(action_id.clone()).await {
            Ok(result) => result,
            Err(e) => {
                warn!(action_id = %action_id, error = ?e, "Action execution failed");
                ActionResult::failure(format!("Execution failed: {:?}", e))
            }
        }
    }

    /// Subscribe to action change events.
    ///
    /// Returns a receiver that will receive `ActionEvent::Added` and
    /// `ActionEvent::Removed` events as cells register/unregister.
    pub fn subscribe(&self) -> broadcast::Receiver<ActionEvent> {
        self.event_tx.subscribe()
    }

    /// Get list of registered cell names.
    pub async fn registered_cells(&self) -> Vec<String> {
        let cells = self.cells.read().await;
        cells.keys().cloned().collect()
    }

    /// Create a dispatcher for exposing this registry as an ActionsService.
    ///
    /// Use this when you want to expose the registry over RPC (e.g., via gateway).
    pub fn dispatcher(self: &Arc<Self>) -> ActionsServiceDispatcher<ActionsServiceImpl> {
        ActionsServiceDispatcher::new(ActionsServiceImpl {
            registry: self.clone(),
        })
    }
}

impl Default for ActionsRegistry {
    fn default() -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            cells: RwLock::new(HashMap::new()),
            event_tx,
        }
    }
}

/// Implementation of ActionsService that wraps an ActionsRegistry.
///
/// This allows the registry to be exposed over RPC for command palettes.
#[derive(Clone)]
pub struct ActionsServiceImpl {
    registry: Arc<ActionsRegistry>,
}

impl ActionsService for ActionsServiceImpl {
    async fn get_all_actions(&self) -> Vec<ActionDefinition> {
        self.registry.get_all_actions().await
    }

    async fn execute(&self, action_id: ActionId) -> ActionResult {
        self.registry.execute(action_id).await
    }
}
