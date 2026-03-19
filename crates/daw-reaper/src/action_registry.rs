//! REAPER Action Registry Implementation
//!
//! Registers actions with REAPER's action system using `reaper_high::Reaper::register_action`.
//! Tracks registered actions so they can be unregistered when a guest disconnects.
//!
//! When a registered action is triggered, all subscribers receive an
//! `ActionEvent::Triggered` event. Guests handle action logic — the host
//! is domain-agnostic.

use crate::main_thread;
use daw_proto::{ActionEvent, ActionRegistryService};
use reaper_high::Reaper;
use reaper_medium::{CommandId, ProjectContext};
use roam::Tx;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Tracks actions registered through this service.
///
/// Maps command_name → command_id for actions we've registered.
/// Used for unregistration and to avoid double-registering.
static REGISTERED_ACTIONS: std::sync::OnceLock<Mutex<HashMap<String, u32>>> =
    std::sync::OnceLock::new();

/// Broadcast channel for action trigger events.
///
/// Each subscriber gets their own `broadcast::Receiver` which forwards
/// events to their roam `Tx<ActionEvent>`.
static ACTION_BROADCASTER: std::sync::OnceLock<broadcast::Sender<String>> =
    std::sync::OnceLock::new();

fn registered_actions() -> &'static Mutex<HashMap<String, u32>> {
    REGISTERED_ACTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn action_broadcaster() -> &'static broadcast::Sender<String> {
    ACTION_BROADCASTER.get_or_init(|| {
        let (tx, _rx) = broadcast::channel::<String>(64);
        tx
    })
}

/// Notify all subscribers that an action was triggered.
///
/// Called from the REAPER action handler closure on the main thread.
/// Uses a broadcast channel so this is non-blocking.
fn notify_action_triggered(command_name: String) {
    let tx = action_broadcaster();
    if tx.receiver_count() == 0 {
        return;
    }
    let _ = tx.send(command_name);
}

/// REAPER action registry implementation.
#[derive(Clone)]
pub struct ReaperActionRegistry;

impl ReaperActionRegistry {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistryService for ReaperActionRegistry {
    async fn register_action(&self, command_name: String, description: String) -> u32 {
        // Check if already registered by us
        {
            let map = registered_actions().lock().unwrap();
            if let Some(&cmd_id) = map.get(&command_name) {
                debug!(
                    "Action '{}' already registered (cmd_id={})",
                    command_name, cmd_id
                );
                return cmd_id;
            }
        }

        let name_for_query = command_name.clone();
        let desc_for_query = description.clone();

        let result = main_thread::query(move || {
            let reaper = Reaper::get();

            // First check if someone else already registered this command name
            let medium = reaper.medium_reaper();
            if let Some(existing) = medium.named_command_lookup(format!("_{name_for_query}")) {
                info!(
                    "Action '{}' already exists in REAPER (cmd_id={})",
                    name_for_query,
                    existing.get()
                );
                return existing.get();
            }

            // register_action needs 'static string args — leak the strings
            // since actions live for the process lifetime anyway.
            let name_static: &'static str = Box::leak(name_for_query.clone().into_boxed_str());
            let desc_static: &'static str = Box::leak(desc_for_query.clone().into_boxed_str());

            // Capture command_name for the trigger notification
            let trigger_name = name_for_query.clone();

            let action = reaper.register_action(
                name_static,
                desc_static,
                None, // no default key binding
                move || {
                    // Notify all subscribers that this action was triggered.
                    // Guests handle the actual logic — host is domain-agnostic.
                    info!("Action triggered: {}", trigger_name);
                    notify_action_triggered(trigger_name.clone());
                },
                reaper_high::ActionKind::NotToggleable,
            );

            let cmd_id = action.command_id().get();
            info!(
                "Registered action '{}' → cmd_id={} (\"{}\")",
                name_for_query, cmd_id, desc_for_query
            );

            // Leak the RegisteredAction so it stays alive (action stays registered).
            // We'll track the command_name → cmd_id mapping ourselves.
            std::mem::forget(action);

            cmd_id
        })
        .await;

        match result {
            Some(cmd_id) if cmd_id > 0 => {
                registered_actions()
                    .lock()
                    .unwrap()
                    .insert(command_name.clone(), cmd_id);
                info!("Action '{}' registered: cmd_id={}", command_name, cmd_id);
                cmd_id
            }
            _ => {
                warn!("Failed to register action '{}'", command_name);
                0
            }
        }
    }

    async fn unregister_action(&self, command_name: String) -> bool {
        let removed = registered_actions()
            .lock()
            .unwrap()
            .remove(&command_name)
            .is_some();

        if removed {
            info!("Unregistered action '{}' (from tracking map)", command_name);
            // Note: REAPER doesn't have a public API to fully unregister a command_id.
            // The action will remain in REAPER's list until restart, but won't do
            // anything since the closure was captured by the now-forgotten RegisteredAction.
        } else {
            debug!(
                "Action '{}' not found in our registry (may not have been registered by us)",
                command_name
            );
        }

        removed
    }

    async fn is_registered(&self, command_name: String) -> bool {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            // NamedCommandLookup expects underscore prefix for custom actions
            medium
                .named_command_lookup(format!("_{command_name}"))
                .is_some()
        })
        .await
        .unwrap_or(false)
    }

    async fn lookup_command_id(&self, command_name: String) -> Option<u32> {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            medium
                .named_command_lookup(format!("_{command_name}"))
                .map(|id| id.get())
        })
        .await
        .flatten()
    }

    async fn subscribe_actions(&self, tx: Tx<ActionEvent>) {
        let mut rx = action_broadcaster().subscribe();
        info!(
            "Action event subscriber added (receivers: {})",
            action_broadcaster().receiver_count()
        );

        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(command_name) => {
                        let event = ActionEvent::Triggered { command_name };
                        if tx.send(event).await.is_err() {
                            debug!("Action event subscriber disconnected");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!("Action event subscriber lagged by {count} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Action broadcast channel closed");
                        break;
                    }
                }
            }
        });
    }

    async fn execute_command(&self, command_id: u32) {
        main_thread::query(move || {
            Reaper::get().medium_reaper().main_on_command_ex(
                CommandId::new(command_id),
                0,
                ProjectContext::CurrentProject,
            );
        })
        .await;
    }

    async fn execute_named_action(&self, command_name: String) -> bool {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let lookup = format!("_{command_name}");
            if let Some(cmd_id) = medium.named_command_lookup(lookup) {
                medium.main_on_command_ex(cmd_id, 0, ProjectContext::CurrentProject);
                true
            } else {
                warn!("Named action not found: {}", command_name);
                false
            }
        })
        .await
        .unwrap_or(false)
    }
}
