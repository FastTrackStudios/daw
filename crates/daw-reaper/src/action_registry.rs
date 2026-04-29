//! REAPER Action Registry Implementation
//!
//! Registers actions with REAPER's action system using `reaper_high::Reaper::register_action`.
//! Tracks registered actions so they can be unregistered when a guest disconnects.
//!
//! When a registered action is triggered, all subscribers receive an
//! `ActionEvent::Triggered` event. Guests handle action logic — the host
//! is domain-agnostic.
//!
//! Actions registered with `show_in_menu: true` are automatically added to the
//! Extensions > FastTrackStudio menu. The menu hierarchy is derived from the
//! command name prefix (e.g., `FTS_SESSION_*` → Session submenu).

use crate::main_thread;
use daw_proto::{ActionEvent, ActionRegistryService};
use reaper_high::Reaper;
use reaper_medium::{
    CommandId, Hmenu, HookCustomMenu, MenuHookFlag, OwnedGaccelRegister, ProjectContext, ReaperStr,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use vox::Tx;

/// Tracks actions registered through this service.
///
/// Maps command_name → command_id for actions we've registered.
/// Used for unregistration and to avoid double-registering.
static REGISTERED_ACTIONS: std::sync::OnceLock<Mutex<HashMap<String, u32>>> =
    std::sync::OnceLock::new();

/// Broadcast channel for action trigger events.
///
/// Each subscriber gets their own `broadcast::Receiver` which forwards
/// events to their vox `Tx<ActionEvent>`.
static ACTION_BROADCASTER: std::sync::OnceLock<broadcast::Sender<String>> =
    std::sync::OnceLock::new();

/// Menu metadata for actions that should appear in the Extensions menu.
static MENU_ACTIONS: std::sync::OnceLock<Mutex<Vec<MenuActionDef>>> = std::sync::OnceLock::new();

/// Toggle state for toggleable actions.
///
/// Maps command_name → current on/off state. REAPER queries toggle state
/// synchronously on the main thread, so we store it here for instant access.
/// Guests update this via `set_toggle_state`.
static TOGGLE_STATES: std::sync::OnceLock<Mutex<HashMap<String, bool>>> =
    std::sync::OnceLock::new();

/// Action metadata stored for menu building.
#[derive(Clone)]
struct MenuActionDef {
    /// REAPER command name (e.g., "FTS_SESSION_TOGGLE_PLAYBACK")
    command_name: String,
    /// Display name shown in menu (the description from registration)
    display_name: String,
    /// Menu group derived from command name (e.g., "Session")
    group: String,
}

pub(crate) fn registered_actions() -> &'static Mutex<HashMap<String, u32>> {
    REGISTERED_ACTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Subscribe directly to action trigger broadcasts.
///
/// Returns a receiver that yields command names whenever an action is triggered.
/// Useful for in-process extensions (LocalCaller) that want to avoid the vox
/// streaming round-trip.
pub fn subscribe_action_broadcasts() -> broadcast::Receiver<String> {
    action_broadcaster().subscribe()
}

fn action_broadcaster() -> &'static broadcast::Sender<String> {
    ACTION_BROADCASTER.get_or_init(|| {
        let (tx, _rx) = broadcast::channel::<String>(64);
        tx
    })
}

fn menu_actions() -> &'static Mutex<Vec<MenuActionDef>> {
    MENU_ACTIONS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn toggle_states() -> &'static Mutex<HashMap<String, bool>> {
    TOGGLE_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Read toggle state for a command. Called from REAPER's main thread
/// via the `ActionKind::Toggleable` closure.
fn read_toggle_state(command_name: &str) -> bool {
    toggle_states()
        .lock()
        .unwrap()
        .get(command_name)
        .copied()
        .unwrap_or(false)
}

/// Derive a menu group name from a REAPER command name.
///
/// `FTS_SESSION_TOGGLE_PLAYBACK` → "Session"
/// `FTS_TRANSPORT_PLAY_STOP` → "Transport"
/// `FTS_MARKERS_REGIONS_INSERT_MARKER` → "Markers Regions"
///
/// Convention: strip "FTS_" prefix, then take segments until we hit
/// a lowercase-starting word (action names are all-caps in the prefix part,
/// but after titlecasing they become mixed). Since the raw command name is
/// ALL_CAPS, we use a heuristic: known domain prefixes get titlecased.
fn derive_menu_group(command_name: &str) -> String {
    let name = command_name.strip_prefix("FTS_").unwrap_or(command_name);

    // Known domain prefixes (order matters — longest match first)
    let known_domains = [
        "MARKERS_REGIONS",
        "DYNAMIC_TEMPLATE",
        "VISIBILITY_MANAGER",
        "AUTO_COLOR",
        "REAPER_EXTENSION",
        "TRANSPORT",
        "SESSION",
        "SIGNAL",
        "SYNC",
        "DAW",
    ];

    for domain in &known_domains {
        if name.starts_with(domain) {
            return titlecase_underscored(domain);
        }
    }

    // Fallback: use first segment
    name.split('_')
        .next()
        .map(titlecase_underscored)
        .unwrap_or_default()
}

/// Titlecase an underscored string: "MARKERS_REGIONS" → "Markers Regions"
fn titlecase_underscored(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    let lower: String = chars.as_str().to_lowercase();
                    format!("{upper}{lower}")
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

// ============================================================================
// Extensions Menu
// ============================================================================

/// Register the Extensions menu hook with REAPER.
///
/// Call once during plugin initialization after `ReaperSession` is created.
/// The hook is invoked each time REAPER shows the Extensions menu,
/// dynamically building it from all registered actions with `show_in_menu`.
pub fn register_extension_menu(session: &mut reaper_medium::ReaperSession) {
    Reaper::get().medium_reaper().add_extensions_main_menu();
    if let Err(e) = session.plugin_register_add_hook_custom_menu::<FtsMenuHook>() {
        warn!("Failed to register menu hook: {:?}", e);
    } else {
        info!("Extensions menu hook registered");
    }
}

/// REAPER menu hook implementation for FastTrackStudio.
struct FtsMenuHook;

impl HookCustomMenu for FtsMenuHook {
    fn call(menuidstr: &ReaperStr, hmenu: Hmenu, flag: MenuHookFlag) {
        let result = std::panic::catch_unwind(|| {
            if flag != MenuHookFlag::Init || menuidstr.to_str() != "Main extensions" {
                return;
            }
            build_extension_menu(hmenu);
        });
        if let Err(e) = result {
            warn!("Panic in menu hook: {:?}", e);
        }
    }
}

/// Build the Extensions > FastTrackStudio menu from registered actions.
fn build_extension_menu(hmenu: Hmenu) {
    let actions = menu_actions().lock().unwrap().clone();
    if actions.is_empty() {
        return;
    }

    let swell = reaper_low::Swell::get();

    // Group actions by their derived menu group
    let mut groups: BTreeMap<String, Vec<MenuActionDef>> = BTreeMap::new();
    for action in &actions {
        groups
            .entry(action.group.clone())
            .or_default()
            .push(action.clone());
    }

    // Create "FastTrackStudio" submenu
    let fts_menu = swell.CreatePopupMenu();

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    for (group_name, group_actions) in &mut groups {
        // Create submenu for this group
        let submenu = swell.CreatePopupMenu();

        // Sort actions by display name
        group_actions.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        for action in group_actions.iter() {
            let lookup = format!("_{}", action.command_name);
            if let Some(cmd_id) = medium.named_command_lookup(lookup) {
                let mut text_buf: Vec<u8> = action.display_name.as_bytes().to_vec();
                text_buf.push(0);
                let mut mii = reaper_low::raw::MENUITEMINFO {
                    fMask: 0x40 | 0x04, // MIIM_TYPE | MIIM_ID
                    fType: 0,           // MFT_STRING
                    wID: cmd_id.get(),
                    hSubMenu: std::ptr::null_mut(),
                    dwTypeData: text_buf.as_mut_ptr() as *mut _,
                    ..unsafe { std::mem::zeroed() }
                };
                let count = unsafe { swell.GetMenuItemCount(submenu) };
                unsafe {
                    swell.InsertMenuItem(submenu, count, 1, &mut mii);
                }
            }
        }

        // Add the group submenu to the FTS menu
        let mut label_buf: Vec<u8> = group_name.as_bytes().to_vec();
        label_buf.push(0);
        let mut mii = reaper_low::raw::MENUITEMINFO {
            fMask: 0x10 | 0x40 | 0x04, // MIIM_SUBMENU | MIIM_TYPE | MIIM_ID
            fType: 0,
            wID: 0,
            hSubMenu: submenu,
            dwTypeData: label_buf.as_mut_ptr() as *mut _,
            ..unsafe { std::mem::zeroed() }
        };
        let count = unsafe { swell.GetMenuItemCount(fts_menu) };
        unsafe {
            swell.InsertMenuItem(fts_menu, count, 1, &mut mii);
        }
    }

    // Insert "FastTrackStudio" into the Extensions menu
    let parent = hmenu.as_ptr();
    let mut label = b"FastTrackStudio\0".to_vec();

    // Add separator before our menu if there are already items
    let existing = unsafe { swell.GetMenuItemCount(parent) };
    if existing > 0 {
        let mut sep = reaper_low::raw::MENUITEMINFO {
            fMask: 0x40,  // MIIM_TYPE
            fType: 0x800, // MFT_SEPARATOR
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            swell.InsertMenuItem(parent, existing, 1, &mut sep);
        }
    }

    let mut mii = reaper_low::raw::MENUITEMINFO {
        fMask: 0x10 | 0x40 | 0x04, // MIIM_SUBMENU | MIIM_TYPE | MIIM_ID
        fType: 0,
        wID: 0,
        hSubMenu: fts_menu,
        dwTypeData: label.as_mut_ptr() as *mut _,
        ..unsafe { std::mem::zeroed() }
    };
    let pos = unsafe { swell.GetMenuItemCount(parent) };
    unsafe {
        swell.InsertMenuItem(parent, pos, 1, &mut mii);
    }

    debug!(
        "Built FastTrackStudio menu ({} actions in {} groups)",
        actions.len(),
        groups.len()
    );
}

// ============================================================================
// Action Registry Service
// ============================================================================

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
    async fn register_action(
        &self,
        command_name: String,
        description: String,
        show_in_menu: bool,
        toggleable: bool,
    ) -> u32 {
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

        // If toggleable, initialize toggle state to off
        if toggleable {
            toggle_states()
                .lock()
                .unwrap()
                .insert(command_name.clone(), false);
        }

        let name_for_query = command_name.clone();
        let desc_for_query = description.clone();

        let result = main_thread::query(move || {
            let reaper = Reaper::get();
            let desc_static: &'static str = Box::leak(desc_for_query.clone().into_boxed_str());

            // First check if someone else already registered this command name
            let medium = reaper.medium_reaper();
            if let Some(existing) = medium.named_command_lookup(format!("_{name_for_query}")) {
                debug!(
                    "Action '{}' already exists in REAPER (cmd_id={})",
                    name_for_query,
                    existing.get()
                );
                let already_listed = {
                    let section = reaper.main_section();
                    section
                        .with_raw(|s| {
                            (0..s.action_list_cnt()).any(|i| {
                                s.get_action_by_index(i)
                                    .map(|a| a.cmd() == existing)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                };
                if !already_listed {
                    let gaccel = OwnedGaccelRegister::without_key_binding(existing, desc_static);
                    let mut session = reaper.medium_session();
                    if let Err(e) = session.plugin_register_add_gaccel(gaccel) {
                        warn!(
                            "Failed to repair gaccel for existing action '{}': {:?}",
                            name_for_query, e
                        );
                    } else {
                        debug!(
                            "Repaired missing gaccel for existing action '{}' (cmd_id={})",
                            name_for_query,
                            existing.get()
                        );
                    }
                }
                return existing.get();
            }

            // register_action needs 'static string args — leak the strings
            // since actions live for the process lifetime anyway.
            let name_static: &'static str = Box::leak(name_for_query.clone().into_boxed_str());

            // Capture command_name for the trigger notification
            let trigger_name = name_for_query.clone();

            let kind = if toggleable {
                // For toggleable actions, read state from the shared toggle map.
                // Guests update this via set_toggle_state().
                let state_key = name_for_query.clone();
                reaper_high::ActionKind::Toggleable(Box::new(move || read_toggle_state(&state_key)))
            } else {
                reaper_high::ActionKind::NotToggleable
            };

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
                kind,
            );

            let cmd_id = action.command_id();

            // reaper_high::register_action only stores the command in its internal
            // map and calls plugin_register_add_command_id. It does NOT register the
            // gaccel (action list entry) when the session is already awake — that
            // only happens during wake_up(). Since daw-bridge calls wake_up() at
            // init time, any actions registered later by guests never appear in
            // REAPER's action list. Fix: register the gaccel ourselves.
            {
                let gaccel = OwnedGaccelRegister::without_key_binding(cmd_id, desc_static);
                let mut session = reaper.medium_session();
                if let Err(e) = session.plugin_register_add_gaccel(gaccel) {
                    warn!(
                        "Failed to register gaccel for '{}': {:?}",
                        name_for_query, e
                    );
                }
            }

            if toggleable {
                // REAPER can be slow to surface newly registered toggle actions in the
                // action list unless their toolbar state is refreshed at least once.
                unsafe {
                    reaper
                        .medium_reaper()
                        .low()
                        .RefreshToolbar2(0, cmd_id.get() as i32);
                }
            }

            let cmd_id_val = cmd_id.get();
            debug!(
                "Registered action '{}' → cmd_id={} (\"{}\")",
                name_for_query, cmd_id_val, desc_for_query
            );

            // Leak the RegisteredAction so it stays alive (action stays registered).
            // We'll track the command_name → cmd_id mapping ourselves.
            std::mem::forget(action);

            cmd_id_val
        })
        .await;

        match result {
            Some(cmd_id) if cmd_id > 0 => {
                registered_actions()
                    .lock()
                    .unwrap()
                    .insert(command_name.clone(), cmd_id);

                // Store menu metadata if this action should appear in the menu
                if show_in_menu {
                    let group = derive_menu_group(&command_name);
                    menu_actions().lock().unwrap().push(MenuActionDef {
                        command_name: command_name.clone(),
                        display_name: description.clone(),
                        group,
                    });
                }

                debug!("Action '{}' registered: cmd_id={}", command_name, cmd_id);
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
            // Also remove from menu metadata
            menu_actions()
                .lock()
                .unwrap()
                .retain(|a| a.command_name != command_name);

            // Clear any toggle state recorded for this action so a later
            // re-register starts fresh (no stale on/off carrying over).
            toggle_states().lock().unwrap().remove(&command_name);

            info!("Unregistered action '{}' (from tracking map)", command_name);
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
            medium
                .named_command_lookup(format!("_{command_name}"))
                .is_some()
        })
        .await
        .unwrap_or(false)
    }

    async fn is_in_action_list(&self, command_name: String) -> bool {
        main_thread::query(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // First resolve the command name to a command ID
            let cmd_id = match medium.named_command_lookup(format!("_{command_name}")) {
                Some(id) => id,
                None => return false,
            };

            // Enumerate the main section's action list to see if this command ID
            // actually has a gaccel entry (i.e., appears in Actions > Show action list)
            let section = reaper.main_section();
            section
                .with_raw(|s| {
                    (0..s.action_list_cnt()).any(|i| {
                        s.get_action_by_index(i)
                            .map(|a| a.cmd() == cmd_id)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
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

    async fn set_toggle_state(&self, command_name: String, is_on: bool) {
        let mut states = toggle_states().lock().unwrap();
        if states.contains_key(&command_name) {
            states.insert(command_name.clone(), is_on);
            debug!("Toggle state for '{}' set to {}", command_name, is_on);
        } else {
            debug!(
                "Ignoring set_toggle_state for '{}' — not registered as toggleable",
                command_name
            );
        }
    }

    async fn get_toggle_state(&self, command_name: String) -> Option<bool> {
        toggle_states().lock().unwrap().get(&command_name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_menu_group_session() {
        assert_eq!(derive_menu_group("FTS_SESSION_TOGGLE_PLAYBACK"), "Session");
    }

    #[test]
    fn derive_menu_group_transport() {
        assert_eq!(derive_menu_group("FTS_TRANSPORT_PLAY"), "Transport");
    }

    #[test]
    fn derive_menu_group_markers_regions() {
        assert_eq!(
            derive_menu_group("FTS_MARKERS_REGIONS_INSERT_MARKER"),
            "Markers Regions"
        );
    }

    #[test]
    fn derive_menu_group_signal() {
        assert_eq!(derive_menu_group("FTS_SIGNAL_NEXT_SONG"), "Signal");
    }

    #[test]
    fn derive_menu_group_sync() {
        assert_eq!(derive_menu_group("FTS_SYNC_TOGGLE_LINK"), "Sync");
    }

    #[test]
    fn derive_menu_group_dynamic_template() {
        assert_eq!(
            derive_menu_group("FTS_DYNAMIC_TEMPLATE_SORT_ALL"),
            "Dynamic Template"
        );
    }

    #[test]
    fn derive_menu_group_visibility_manager() {
        assert_eq!(
            derive_menu_group("FTS_VISIBILITY_MANAGER_TOGGLE_DRUMS"),
            "Visibility Manager"
        );
    }

    #[test]
    fn derive_menu_group_auto_color() {
        assert_eq!(derive_menu_group("FTS_AUTO_COLOR_COLOR_ALL"), "Auto Color");
    }

    #[test]
    fn derive_menu_group_daw() {
        assert_eq!(derive_menu_group("FTS_DAW_SOMETHING"), "Daw");
    }

    #[test]
    fn derive_menu_group_unknown_prefix_falls_back_to_first_segment() {
        assert_eq!(derive_menu_group("UNKNOWN_PREFIX"), "Unknown");
    }

    #[test]
    fn titlecase_underscored_multi_word() {
        assert_eq!(titlecase_underscored("MARKERS_REGIONS"), "Markers Regions");
    }

    #[test]
    fn titlecase_underscored_single_word() {
        assert_eq!(titlecase_underscored("TRANSPORT"), "Transport");
    }

    #[test]
    fn titlecase_underscored_two_words() {
        assert_eq!(titlecase_underscored("AUTO_COLOR"), "Auto Color");
    }

    #[test]
    fn titlecase_underscored_empty() {
        assert_eq!(titlecase_underscored(""), "");
    }
}
