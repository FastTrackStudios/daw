//! Actions Standalone - Local actions for web and desktop apps
//!
//! This module provides a standalone actions implementation for apps that
//! don't have a host process (like the REAPER extension). It allows:
//!
//! 1. Registering local actions (app-specific functionality)
//! 2. Implementing `ActionsService` for command palette integration
//! 3. Combining with remote hosts for multi-source action palettes
//!
//! ## Usage
//!
//! ```ignore
//! use actions_standalone::StandaloneActions;
//!
//! // Create standalone actions
//! let standalone = StandaloneActions::new();
//!
//! // Register local actions
//! standalone.register_action(
//!     ActionDefinition::new(
//!         "fts.standalone.open_settings",
//!         "Open Settings",
//!         "Opens the application settings",
//!     )
//!     .with_category(ActionCategory::Settings)
//!     .with_shortcut("Cmd+,"),
//!     |_action_id| {
//!         // Open settings logic here
//!         ActionResult::success()
//!     },
//! );
//!
//! // Get all actions (for command palette)
//! let actions = standalone.get_all_actions();
//!
//! // Execute an action
//! let result = standalone.execute("fts.standalone.open_settings").await;
//! ```

use actions_proto::{
    ActionDefinition, ActionId, ActionResult, ActionsService, ActionsServiceDispatcher,
};
use async_lock::RwLock;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{info, warn};

/// Type alias for sync action handler functions (backward-compatible).
///
/// Handlers receive the action ID and return an ActionResult.
pub type ActionHandler = Arc<dyn Fn(&ActionId) -> ActionResult + Send + Sync>;

/// Type alias for async action handler functions.
///
/// Handlers receive a cloned `ActionId` and return a future resolving to `ActionResult`.
pub type AsyncActionHandler =
    Arc<dyn Fn(ActionId) -> Pin<Box<dyn Future<Output = ActionResult> + Send>> + Send + Sync>;

/// Internal handler enum supporting both sync and async handlers.
enum Handler {
    Sync(ActionHandler),
    Async(AsyncActionHandler),
}

/// A registered local action with its definition and handler.
struct RegisteredAction {
    definition: ActionDefinition,
    handler: Handler,
}

/// Standalone actions registry for local app actions.
///
/// This provides actions that are local to the app (not from remote cells).
/// Use this for web/desktop app functionality like "Open Settings",
/// "Toggle Dark Mode", etc.
pub struct StandaloneActions {
    /// Map of action_id -> registered action
    actions: RwLock<HashMap<String, RegisteredAction>>,
}

impl StandaloneActions {
    /// Create a new empty standalone actions registry.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            actions: RwLock::new(HashMap::new()),
        })
    }

    /// Register a local action with a sync handler.
    ///
    /// The handler will be called when the action is executed.
    pub async fn register<F>(&self, definition: ActionDefinition, handler: F)
    where
        F: Fn(&ActionId) -> ActionResult + Send + Sync + 'static,
    {
        let action_id = definition.id.as_str().to_string();
        let action_name = definition.name.clone();

        {
            let mut actions = self.actions.write().await;
            actions.insert(
                action_id.clone(),
                RegisteredAction {
                    definition: definition.clone(),
                    handler: Handler::Sync(Arc::new(handler)),
                },
            );
        }

        info!(action_id = %action_id, name = %action_name, "Registered standalone action");
    }

    /// Register a local action with an async handler.
    ///
    /// The handler receives a cloned `ActionId` and returns a future.
    pub async fn register_async<F, Fut>(&self, definition: ActionDefinition, handler: F)
    where
        F: Fn(ActionId) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ActionResult> + Send + 'static,
    {
        let action_id = definition.id.as_str().to_string();
        let action_name = definition.name.clone();

        let handler: AsyncActionHandler = Arc::new(move |id| Box::pin(handler(id)));

        {
            let mut actions = self.actions.write().await;
            actions.insert(
                action_id.clone(),
                RegisteredAction {
                    definition: definition.clone(),
                    handler: Handler::Async(handler),
                },
            );
        }

        info!(action_id = %action_id, name = %action_name, "Registered async standalone action");
    }

    /// Unregister a local action.
    pub async fn unregister(&self, action_id: impl Into<ActionId>) {
        let action_id = action_id.into();
        let id_str = action_id.as_str().to_string();

        let removed = {
            let mut actions = self.actions.write().await;
            actions.remove(&id_str)
        };

        if removed.is_some() {
            info!(action_id = %action_id, "Unregistered standalone action");
        }
    }

    /// Get all registered actions.
    pub async fn get_all_actions(&self) -> Vec<ActionDefinition> {
        let actions = self.actions.read().await;
        actions.values().map(|a| a.definition.clone()).collect()
    }

    /// Execute an action by ID (supports both sync and async handlers).
    pub async fn execute(&self, action_id: impl Into<ActionId>) -> ActionResult {
        let action_id = action_id.into();
        let id_str = action_id.as_str();

        // We need to call the handler outside the read lock to avoid
        // holding the lock across an await point.
        enum Callable {
            Sync(ActionHandler),
            Async(AsyncActionHandler),
        }

        let callable = {
            let actions = self.actions.read().await;
            actions.get(id_str).map(|a| match &a.handler {
                Handler::Sync(f) => Callable::Sync(f.clone()),
                Handler::Async(f) => Callable::Async(f.clone()),
            })
        };

        match callable {
            Some(Callable::Sync(handler)) => {
                info!(action_id = %action_id, "Executing standalone action");
                handler(&action_id)
            }
            Some(Callable::Async(handler)) => {
                info!(action_id = %action_id, "Executing async standalone action");
                handler(action_id).await
            }
            None => {
                warn!(action_id = %action_id, "Standalone action not found");
                ActionResult::failure(format!("Action not found: {}", action_id))
            }
        }
    }

    /// Check if an action is registered.
    pub async fn has_action(&self, action_id: &ActionId) -> bool {
        let actions = self.actions.read().await;
        actions.contains_key(action_id.as_str())
    }

    /// Create a dispatcher for exposing these actions as an ActionsService.
    pub fn dispatcher(self: &Arc<Self>) -> ActionsServiceDispatcher<StandaloneActionsServiceImpl> {
        ActionsServiceDispatcher::new(StandaloneActionsServiceImpl {
            standalone: self.clone(),
        })
    }
}

impl Default for StandaloneActions {
    fn default() -> Self {
        Self {
            actions: RwLock::new(HashMap::new()),
        }
    }
}

/// Implementation of ActionsService for standalone actions.
#[derive(Clone)]
pub struct StandaloneActionsServiceImpl {
    standalone: Arc<StandaloneActions>,
}

impl ActionsService for StandaloneActionsServiceImpl {
    async fn get_all_actions(&self) -> Vec<ActionDefinition> {
        self.standalone.get_all_actions().await
    }

    async fn execute(&self, action_id: ActionId) -> ActionResult {
        self.standalone.execute(action_id).await
    }
}

// ============================================================================
// Common standalone actions that apps might want to use
// ============================================================================

/// Get action definitions for common standalone actions.
///
/// Returns definitions for:
/// - `fts.standalone.open_settings` (Cmd+,)
/// - `fts.standalone.toggle_dark_mode` (Cmd+Shift+D)
/// - `fts.standalone.show_about`
/// - `fts.standalone.command_palette` (Cmd+Shift+P)
///
/// Use this to register keybindings for standalone actions in the dispatcher,
/// then register handlers separately.
pub fn common_action_definitions() -> Vec<ActionDefinition> {
    use actions_proto::ActionCategory;
    use actions_proto::ids::standalone as ids;

    vec![
        ActionDefinition::new(
            ids::OPEN_SETTINGS.to_id(),
            "Open Settings",
            "Opens the application settings",
        )
        .with_category(ActionCategory::Settings)
        .with_shortcut("Cmd+,"),
        ActionDefinition::new(
            ids::TOGGLE_DARK_MODE.to_id(),
            "Toggle Dark Mode",
            "Switches between light and dark themes",
        )
        .with_category(ActionCategory::View)
        .with_shortcut("Cmd+Shift+D"),
        ActionDefinition::new(
            ids::SHOW_ABOUT.to_id(),
            "About FastTrackStudio",
            "Shows information about the application",
        )
        .with_category(ActionCategory::General),
        ActionDefinition::new(
            ids::COMMAND_PALETTE.to_id(),
            "Command Palette",
            "Opens the command palette",
        )
        .with_category(ActionCategory::General)
        .with_shortcut("Cmd+Shift+P"),
    ]
}

/// Register common standalone actions for a desktop/web app.
///
/// Registers both action definitions and handlers into a [`StandaloneActions`] registry.
/// For keybinding integration, use [`common_action_definitions()`] instead and register
/// handlers via [`ActionDispatcher`](actions_keybindings::ActionDispatcher).
pub async fn register_common_actions<F>(standalone: &StandaloneActions, handler: F)
where
    F: Fn(&str) -> ActionResult + Send + Sync + Clone + 'static,
{
    for action in common_action_definitions() {
        let action_id_str = action.id.as_str().to_string();
        let handler_clone = handler.clone();
        standalone
            .register(action, move |_id| handler_clone(&action_id_str))
            .await;
    }
}
