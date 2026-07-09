//! Action Registration for REAPER
//!
//! Bridges between the generic ActionsRegistry and REAPER's native action system.

use actions_proto::{ActionDefinition, LocalActionImplementation, LocalActionRegistration};
use actions_registry::ActionsRegistry;
use reaper_high::{ActionKind, Reaper, RegisteredAction};
use reaper_medium::CommandId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::{debug, info, warn};
use vox::Caller;

/// Global storage for registered actions (keeps them alive for REAPER)
static REGISTERED_ACTIONS: OnceLock<Mutex<Vec<RegisteredAction>>> = OnceLock::new();

/// Global storage for action definitions (for menu building)
static ACTION_DEFS: OnceLock<Mutex<Vec<MenuActionDef>>> = OnceLock::new();

/// Global registry instance
static REGISTRY: OnceLock<Arc<ActionsRegistry>> = OnceLock::new();

/// Global tokio runtime for async operations from REAPER callbacks
static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Cache command IDs by stable command id string (e.g. FTS_SESSION_LOG_HELLO)
static COMMAND_IDS: OnceLock<Mutex<HashMap<String, CommandId>>> = OnceLock::new();
static REGISTERED_ACTION_IDS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static COMMAND_OWNERS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

type LocalActionHandler = Arc<dyn Fn() -> actions_proto::ActionResult + Send + Sync>;
static LOCAL_ACTION_HANDLERS: OnceLock<Mutex<HashMap<String, LocalActionHandler>>> =
    OnceLock::new();

fn get_registered_actions_storage() -> &'static Mutex<Vec<RegisteredAction>> {
    REGISTERED_ACTIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn get_action_defs_storage() -> &'static Mutex<Vec<MenuActionDef>> {
    ACTION_DEFS.get_or_init(|| Mutex::new(Vec::new()))
}

fn get_local_action_handlers_storage() -> &'static Mutex<HashMap<String, LocalActionHandler>> {
    LOCAL_ACTION_HANDLERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_command_ids_storage() -> &'static Mutex<HashMap<String, CommandId>> {
    COMMAND_IDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_registered_action_ids_storage() -> &'static Mutex<HashSet<String>> {
    REGISTERED_ACTION_IDS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn get_command_owners_storage() -> &'static Mutex<HashMap<String, String>> {
    COMMAND_OWNERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize_command_id(action: &ActionDefinition) -> String {
    let raw = action.id.to_command_id();
    if raw.starts_with("FTS_") {
        return raw;
    }

    // Shared extension actions are expected under the FTS namespace in REAPER.
    if action.id.namespace() == Some("fts") {
        format!("FTS_{}", raw)
    } else {
        raw
    }
}

fn reaper_display_name(action: &ActionDefinition) -> String {
    let Some(path) = action.menu_path.as_deref() else {
        return action.display_name();
    };

    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if matches!(parts.first(), Some(first) if first.eq_ignore_ascii_case("fts")) {
        parts.remove(0);
    }

    if parts.is_empty() {
        format!("FTS: {}", action.name)
    } else {
        format!("FTS / {}: {}", parts.join(" / "), action.name)
    }
}

/// Get the global ActionsRegistry
pub fn get_registry() -> Option<Arc<ActionsRegistry>> {
    REGISTRY.get().cloned()
}

/// Get the tokio runtime for async operations
fn get_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
    })
}

/// Initialize the actions registry.
/// Call this once at extension startup.
pub fn init_registry() -> Arc<ActionsRegistry> {
    let registry = ActionsRegistry::new();
    let _ = REGISTRY.set(registry.clone());
    registry
}

/// Get all registered action definitions (for menu building)
pub fn get_all_registered_actions() -> Vec<MenuActionDef> {
    get_action_defs_storage()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Resolve REAPER command ID for a command string.
/// Tries cache first, then NamedCommandLookup with and without underscore prefix.
pub fn get_command_id(command_id_str: &str) -> Option<CommandId> {
    if let Ok(cache) = get_command_ids_storage().lock() {
        if let Some(cmd) = cache.get(command_id_str).copied() {
            return Some(cmd);
        }
    }

    let medium = Reaper::get().medium_reaper();
    let found = medium
        .named_command_lookup(command_id_str)
        .or_else(|| medium.named_command_lookup(format!("_{}", command_id_str)));

    if let Some(cmd) = found {
        if let Ok(mut cache) = get_command_ids_storage().lock() {
            cache.insert(command_id_str.to_string(), cmd);
        }
    }

    found
}

/// Simple action definition for menu display
#[derive(Clone)]
pub struct MenuActionDef {
    /// Command ID (REAPER format, e.g., "FTS_SESSION_LOG_HELLO")
    pub command_id: String,
    /// Display name shown in REAPER
    pub display_name: String,
    /// Menu path (e.g., "FTS/Session")
    pub menu_path: Option<String>,
}

/// Register a cell with the actions registry and register its actions with REAPER.
///
/// This queries the cell for actions via `DefinesActions::get_actions()` and
/// registers each action with REAPER's action system.
pub async fn register_cell(cell_name: &str, handle: Caller) {
    let registry = match get_registry() {
        Some(r) => r,
        None => {
            warn!("ActionsRegistry not initialized");
            return;
        }
    };

    // Register the cell with the registry (queries for actions via RPC)
    registry.register_cell(cell_name, handle).await;

    // Get the actions we just registered
    let actions = registry.get_cell_actions(cell_name).await;

    // Register each action with REAPER
    for action in actions {
        if let Err(e) = register_action_with_reaper(&action) {
            warn!(action = %action.id, error = %e, "Failed to register action with REAPER");
        }
    }

    // Wake up REAPER so actions appear in the action list
    if let Err(e) = Reaper::get().wake_up() {
        warn!(error = %e, "Failed to wake up REAPER after action registration");
    }

    // Populate command lookup cache after wake-up
    refresh_command_id_cache();
}

/// Register a single action with REAPER's action system
pub fn register_action_with_reaper(action: &ActionDefinition) -> Result<(), String> {
    let action_id = action.id.clone();
    let action_id_str = action_id.as_str().to_string();
    let command_id = normalize_command_id(action);
    let display_name = reaper_display_name(action);
    let menu_path = action.menu_path.clone();

    {
        let mut ids = get_registered_action_ids_storage()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if ids.contains(&action_id_str) {
            info!(
                action_id = %action_id,
                command_id = %command_id,
                "Action already registered; skipping duplicate registration"
            );
            return Ok(());
        }
        ids.insert(action_id_str.clone());
    }

    {
        let mut owners = get_command_owners_storage()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(existing_owner) = owners.get(&command_id) {
            if existing_owner != &action_id_str {
                get_registered_action_ids_storage()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&action_id_str);
                return Err(format!(
                    "command ID collision: {} already owned by {} (new action {})",
                    command_id, existing_owner, action_id_str
                ));
            }
        } else {
            owners.insert(command_id.clone(), action_id_str.clone());
        }
    }

    debug!(
        command_id = %command_id,
        display_name = %display_name,
        "Registering action with REAPER"
    );

    // Leak the strings to get 'static lifetime (REAPER requires this)
    let command_id_static: &'static str = Box::leak(command_id.clone().into_boxed_str());
    let display_name_static: &'static str = Box::leak(display_name.clone().into_boxed_str());

    // Create the action handler closure
    let handler = move || {
        let action_id = action_id.clone();

        // Prefer local in-process handlers when available.
        if let Some(local_handler) = get_local_action_handlers_storage()
            .lock()
            .ok()
            .and_then(|handlers| handlers.get(action_id.as_str()).cloned())
        {
            let result = local_handler();
            if result.success {
                info!(
                    action_id = %action_id,
                    message = ?result.message,
                    "Local action executed successfully"
                );
            } else {
                warn!(
                    action_id = %action_id,
                    message = ?result.message,
                    "Local action execution failed"
                );
            }
            return;
        }

        let registry = match get_registry() {
            Some(r) => r,
            None => {
                warn!(action_id = %action_id, "No registry available");
                return;
            }
        };

        debug!(action_id = %action_id, "Executing action");

        // Execute async code from sync REAPER callback context
        let rt = get_runtime();
        rt.block_on(async move {
            let result = registry.execute(action_id.clone()).await;
            if result.success {
                info!(
                    action_id = %action_id,
                    message = ?result.message,
                    "Action executed successfully"
                );
            } else {
                warn!(
                    action_id = %action_id,
                    message = ?result.message,
                    "Action execution failed"
                );
            }
        });
    };

    // Register with REAPER
    let registered_action = Reaper::get().register_action(
        command_id_static,
        display_name_static,
        None, // No default key binding
        handler,
        ActionKind::NotToggleable,
    );

    // Store the RegisteredAction to keep it alive
    get_registered_actions_storage()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(registered_action);

    // Resolve and cache command ID from REAPER lookup.
    let reaper_cmd_id = get_command_id(&command_id);

    // Store action def for menu building
    get_action_defs_storage()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(MenuActionDef {
            command_id,
            display_name: action.name.clone(),
            menu_path,
        });

    info!(
        action_id = %action.id,
        reaper_cmd_id = reaper_cmd_id.map(|id| id.get()),
        "Action registered with REAPER"
    );

    Ok(())
}

/// Register an in-process action and handler directly with REAPER.
pub fn register_local_action<F>(action: ActionDefinition, handler: F) -> Result<(), String>
where
    F: Fn() -> actions_proto::ActionResult + Send + Sync + 'static,
{
    get_local_action_handlers_storage()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(action.id.as_str().to_string(), Arc::new(handler));
    let result = register_action_with_reaper(&action);

    if let Err(err) = Reaper::get().wake_up() {
        warn!(
            error = %err,
            action_id = %action.id,
            "Failed to wake up REAPER after local action registration"
        );
    }

    // Keep cache in sync with REAPER after local registration as well.
    refresh_command_id_cache();

    result
}

/// Summary of bulk local action registration.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalRegistrationSummary {
    pub registered: usize,
    pub skipped_unsupported: usize,
    pub failed: usize,
}

/// Register a batch of typed local actions.
///
/// `Unsupported` actions are intentionally skipped (with logging), while
/// `Supported` actions are registered with REAPER.
pub fn register_local_actions(
    actions: impl IntoIterator<Item = LocalActionRegistration>,
) -> LocalRegistrationSummary {
    let mut summary = LocalRegistrationSummary::default();

    for action in actions {
        let command_id = action.definition.id.to_command_id();
        let action_id = action.definition.id.clone();
        match action.implementation {
            LocalActionImplementation::Supported(handler) => {
                let handler_for_registration = {
                    let handler = handler.clone();
                    move || handler()
                };
                if let Err(err) = register_local_action(action.definition, handler_for_registration)
                {
                    summary.failed += 1;
                    warn!(
                        error = %err,
                        action_id = %action_id,
                        command_id = %command_id,
                        "Failed to register local action"
                    );
                } else {
                    summary.registered += 1;
                }
            }
            LocalActionImplementation::Unsupported(reason) => {
                summary.skipped_unsupported += 1;
                info!(
                    action_id = %action_id,
                    command_id = %command_id,
                    reason,
                    "Skipping unsupported local action"
                );
            }
        }
    }

    info!(
        registered = summary.registered,
        skipped_unsupported = summary.skipped_unsupported,
        failed = summary.failed,
        "Finished local action registration"
    );

    summary
}

/// Unregister a cell's actions from REAPER.
/// Note: REAPER doesn't support unregistering actions at runtime,
/// so this just removes from our internal tracking.
pub async fn unregister_cell(cell_name: &str) {
    if let Some(registry) = get_registry() {
        registry.unregister_cell(cell_name).await;
    }
}

/// Refresh command ID lookup cache for all known action definitions.
pub fn refresh_command_id_cache() {
    let medium = Reaper::get().medium_reaper();

    let action_defs = get_all_registered_actions();
    if action_defs.is_empty() {
        return;
    }

    if let Ok(mut cache) = get_command_ids_storage().lock() {
        for action in action_defs {
            if let Some(cmd_id) = medium
                .named_command_lookup(action.command_id.as_str())
                .or_else(|| medium.named_command_lookup(format!("_{}", action.command_id)))
            {
                cache.insert(action.command_id, cmd_id);
            }
        }
    }
}
