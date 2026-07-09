//! Actions Protocol - Shared types and service definitions
//!
//! This crate defines the protocol for the actions system, allowing cells
//! to provide actions and hosts to aggregate them for command palettes.
//!
//! ## Architecture
//!
//! ```text
//!                                    ┌─────────────────────────────────────┐
//!                                    │         Command Palette             │
//!                                    │  (web app / desktop app)            │
//!                                    │                                     │
//!                                    │  ┌─────────────────────────────┐    │
//!                                    │  │ Host Selector:              │    │
//!                                    │  │  ○ Standalone               │    │
//!                                    │  │  ● REAPER @ 192.168.1.10    │    │
//!                                    │  │  ○ REAPER @ localhost       │    │
//!                                    │  └─────────────────────────────┘    │
//!                                    └──────────────┬──────────────────────┘
//!                                                   │
//!                          ┌────────────────────────┼────────────────────────┐
//!                          │                        │                        │
//!                          ▼                        ▼                        ▼
//!                 ┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
//!                 │   Standalone    │      │ REAPER Host 1   │      │ REAPER Host 2   │
//!                 │ (ActionsService)│      │ (ActionsService)│      │ (ActionsService)│
//!                 │                 │      │                 │      │                 │
//!                 │ Local actions   │      │ Aggregates from │      │ Aggregates from │
//!                 │ for web/desktop │      │ session, daw,   │      │ session, daw,   │
//!                 │                 │      │ etc. cells      │      │ etc. cells      │
//!                 └─────────────────┘      └─────────────────┘      └─────────────────┘
//! ```
//!
//! ## Key Traits
//!
//! - `DefinesActions`: Implemented by cells that provide actions
//! - `ActionsService`: Implemented by hosts to aggregate and expose all actions

#![deny(unsafe_code)]

use convert_case::{Case, Casing};
use facet::Facet;
use vox::service;

pub mod ids;
#[cfg(test)]
mod macro_tests;
pub mod macros;
pub mod search;
pub mod when;

/// Unique identifier for an action.
///
/// Action IDs follow the pattern `{namespace}.{cell}.{action}`.
/// Examples:
/// - `fts.session.log_hello`
/// - `fts.transport.play`
/// - `fts.standalone.open_settings` (for standalone app actions)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Facet)]
pub struct ActionId(pub String);

impl ActionId {
    /// Create a new ActionId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the action ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the namespace part (e.g., "fts" from "fts.session.log_hello")
    pub fn namespace(&self) -> Option<&str> {
        self.0.split('.').next()
    }

    /// Returns the cell/source part (e.g., "session" from "fts.session.log_hello")
    pub fn source(&self) -> Option<&str> {
        self.0.split('.').nth(1)
    }

    /// Returns the action name part (e.g., "log_hello" from "fts.session.log_hello")
    pub fn name(&self) -> Option<&str> {
        self.0.split('.').nth(2)
    }

    /// Convert to REAPER command ID format (e.g., "FTS_SESSION_LOG_HELLO")
    pub fn to_command_id(&self) -> String {
        self.0.replace('.', "_").to_uppercase()
    }
}

fn sanitize_symbol_segment(raw: &str) -> String {
    let normalized: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    normalized.to_case(Case::UpperSnake)
}

pub fn generated_action_id(menu_path: Option<&str>, action_name: &str) -> String {
    let mut segments = Vec::new();

    if let Some(path) = menu_path {
        for part in path.split('/').filter(|p| !p.trim().is_empty()) {
            let norm = sanitize_symbol_segment(part);
            if !norm.is_empty() {
                segments.push(norm);
            }
        }
    }

    if segments.is_empty() || segments.first().map(String::as_str) != Some("FTS") {
        segments.insert(0, "FTS".to_string());
    }

    let action = sanitize_symbol_segment(action_name);
    if !action.is_empty() {
        segments.push(action);
    }

    segments.join("_")
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ActionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ActionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Category for organizing actions in menus/palettes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Facet)]
#[repr(u8)]
pub enum ActionCategory {
    #[default]
    General,
    Transport,
    Session,
    Project,
    Tracks,
    View,
    Settings,
    Dev,
}

impl std::fmt::Display for ActionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::General => write!(f, "General"),
            Self::Transport => write!(f, "Transport"),
            Self::Session => write!(f, "Session"),
            Self::Project => write!(f, "Project"),
            Self::Tracks => write!(f, "Tracks"),
            Self::View => write!(f, "View"),
            Self::Settings => write!(f, "Settings"),
            Self::Dev => write!(f, "Dev"),
        }
    }
}

/// Definition of an action (metadata only, no execution logic).
#[derive(Debug, Clone, Facet)]
pub struct ActionDefinition {
    /// Unique identifier for the action
    pub id: ActionId,
    /// Human-readable name for display in menus/palettes
    pub name: String,
    /// Description/tooltip for the action
    pub description: String,
    /// Category for organizing in menus/palettes
    pub category: ActionCategory,
    /// Menu path (e.g., "FTS/Session") - None means don't show in menu
    pub menu_path: Option<String>,
    /// Keyboard shortcut hint (e.g., "Cmd+Shift+P") - display only
    pub shortcut_hint: Option<String>,
    /// When-clause expression string. None = always active.
    pub when: Option<String>,
}

impl ActionDefinition {
    /// Create a new action definition
    pub fn new(
        id: impl Into<ActionId>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            category: ActionCategory::General,
            menu_path: None,
            shortcut_hint: None,
            when: None,
        }
    }

    /// Set the category
    pub fn with_category(mut self, category: ActionCategory) -> Self {
        self.category = category;
        self
    }

    /// Set the menu path
    pub fn with_menu_path(mut self, path: impl Into<String>) -> Self {
        self.menu_path = Some(path.into());
        self
    }

    /// Set a keyboard shortcut hint
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut_hint = Some(shortcut.into());
        self
    }

    /// Set a when-clause expression (e.g., "tab:performance").
    pub fn with_when(mut self, expr: impl Into<String>) -> Self {
        self.when = Some(expr.into());
        self
    }

    /// Check if this action is active given the current context.
    ///
    /// Returns true if there is no when-clause or if the clause evaluates to true.
    pub fn is_active(&self, ctx: &when::ActionContext) -> bool {
        match &self.when {
            None => true,
            Some(expr_str) => match when::WhenExpr::parse(expr_str) {
                Ok(expr) => expr.evaluate(ctx),
                Err(_) => true, // Malformed when-clause: fail-open
            },
        }
    }

    /// Get the display name with developer prefix and domain hierarchy.
    ///
    /// Derives the hierarchy from `menu_path`. Examples:
    /// - menu_path `"FTS/Sync"`, name `"Toggle Ableton Link"` → `"FTS: Sync - Toggle Ableton Link"`
    /// - menu_path `"FTS/Sync/Link"`, name `"Puppet Mode"` → `"FTS: Sync - Link - Puppet Mode"`
    /// - no menu_path, name `"Hello"` → `"FTS: Hello"`
    pub fn display_name(&self) -> String {
        if let Some(ref path) = self.menu_path {
            // menu_path is like "FTS/Sync" or "FTS/Sync/Link"
            // Skip the first segment ("FTS") since we add the prefix ourselves
            let segments: Vec<&str> = path.split('/').skip(1).collect();
            if segments.is_empty() {
                format!("FTS: {}", self.name)
            } else {
                format!("FTS: {} - {}", segments.join(" - "), self.name)
            }
        } else {
            format!("FTS: {}", self.name)
        }
    }
}

/// Result of executing an action
#[derive(Debug, Clone, Facet)]
pub struct ActionResult {
    /// Whether the action executed successfully
    pub success: bool,
    /// Optional message (error message on failure, info on success)
    pub message: Option<String>,
}

impl ActionResult {
    /// Create a successful result
    pub fn success() -> Self {
        Self {
            success: true,
            message: None,
        }
    }

    /// Create a successful result with a message
    pub fn success_with_message(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
        }
    }

    /// Create a failure result
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
        }
    }
}

/// Local host-side implementation for an action definition.
#[derive(Clone)]
pub enum LocalActionImplementation {
    /// Action is supported and can be executed in-process.
    Supported(std::sync::Arc<dyn Fn() -> ActionResult + Send + Sync>),
    /// Action is intentionally unsupported in this host/runtime.
    Unsupported(&'static str),
}

/// Typed local action registration entry.
#[derive(Clone)]
pub struct LocalActionRegistration {
    pub definition: ActionDefinition,
    pub implementation: LocalActionImplementation,
}

/// Event emitted when actions change
#[derive(Debug, Clone)]
pub enum ActionEvent {
    /// Actions were added (cell/source registered)
    Added {
        source: String,
        actions: Vec<ActionDefinition>,
    },
    /// Actions were removed (cell/source unregistered)
    Removed {
        source: String,
        action_ids: Vec<ActionId>,
    },
}

// ============================================================================
// DefinesActions - Service trait for cells that provide actions
// ============================================================================

/// Service trait for cells that define actions.
///
/// Any cell that wants to provide actions should implement this trait.
/// The host (via `ActionsRegistry`) will query each connected cell for
/// actions using this interface.
///
/// ## Example
///
/// ```ignore
/// impl DefinesActions for MyServiceImpl {
///     async fn get_actions(&self, _cx: &Context) -> Vec<ActionDefinition> {
///         vec![
///             ActionDefinition::new(
///                 "fts.mycell.my_action",
///                 "My Action",
///                 "Does something cool",
///             )
///             .with_category(ActionCategory::General)
///             .with_menu_path("FTS/MyCell"),
///         ]
///     }
///
///     async fn execute_action(&self, _cx: &Context, action_id: ActionId) -> ActionResult {
///         match action_id.as_str() {
///             "fts.mycell.my_action" => {
///                 // Do the thing
///                 ActionResult::success()
///             }
///             _ => ActionResult::failure("Unknown action"),
///         }
///     }
/// }
/// ```
#[service]
pub trait DefinesActions {
    /// Get all actions defined by this cell.
    async fn get_actions(&self) -> Vec<ActionDefinition>;

    /// Execute an action by ID.
    ///
    /// Returns `ActionResult` indicating success/failure.
    async fn execute_action(&self, action_id: ActionId) -> ActionResult;
}

// ============================================================================
// ActionsService - Aggregated service exposed by hosts
// ============================================================================

/// Service trait for hosts that aggregate actions from multiple sources.
///
/// This is exposed by:
/// - REAPER extension (aggregates actions from cells like session, daw, etc.)
/// - Standalone apps (provides local actions for web/desktop)
///
/// Command palettes connect to this service to get all available actions
/// and execute them.
///
/// ## Example
///
/// ```ignore
/// // In a command palette UI
/// let actions = actions_client.get_all_actions().await?;
///
/// // Display in palette, user selects one...
/// let result = actions_client.execute(selected_action_id).await?;
/// ```
#[service]
pub trait ActionsService {
    /// Get all available actions from all sources.
    ///
    /// Returns actions aggregated from all cells/sources.
    async fn get_all_actions(&self) -> Vec<ActionDefinition>;

    /// Execute an action by ID.
    ///
    /// Routes the execution to the appropriate cell/source.
    async fn execute(&self, action_id: ActionId) -> ActionResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::when::ActionContext;

    // ── ActionId::to_command_id ──────────────────────────────────────

    #[test]
    fn action_id_to_command_id_simple() {
        let id = ActionId::new("fts.session.toggle_playback");
        assert_eq!(id.to_command_id(), "FTS_SESSION_TOGGLE_PLAYBACK");
    }

    #[test]
    fn action_id_to_command_id_dots_replaced() {
        let id = ActionId::new("fts.transport.play");
        assert_eq!(id.to_command_id(), "FTS_TRANSPORT_PLAY");
    }

    #[test]
    fn action_id_to_command_id_nested_segments() {
        let id = ActionId::new("fts.daw.mixer.solo_track");
        assert_eq!(id.to_command_id(), "FTS_DAW_MIXER_SOLO_TRACK");
    }

    #[test]
    fn action_id_to_command_id_already_upper() {
        let id = ActionId::new("FTS.SESSION.PLAY");
        assert_eq!(id.to_command_id(), "FTS_SESSION_PLAY");
    }

    #[test]
    fn action_id_namespace_source_name() {
        let id = ActionId::new("fts.session.log_hello");
        assert_eq!(id.namespace(), Some("fts"));
        assert_eq!(id.source(), Some("session"));
        assert_eq!(id.name(), Some("log_hello"));
    }

    // ── ActionDefinition builder methods ─────────────────────────────

    #[test]
    fn action_definition_defaults() {
        let def = ActionDefinition::new("fts.test.action", "Test", "A test action");
        assert_eq!(def.id.as_str(), "fts.test.action");
        assert_eq!(def.name, "Test");
        assert_eq!(def.description, "A test action");
        assert_eq!(def.category, ActionCategory::General);
        assert!(def.menu_path.is_none());
        assert!(def.shortcut_hint.is_none());
        assert!(def.when.is_none());
    }

    #[test]
    fn action_definition_with_category() {
        let def = ActionDefinition::new("fts.test.action", "Test", "desc")
            .with_category(ActionCategory::Transport);
        assert_eq!(def.category, ActionCategory::Transport);
    }

    #[test]
    fn action_definition_with_menu_path() {
        let def =
            ActionDefinition::new("fts.test.action", "Test", "desc").with_menu_path("FTS/Session");
        assert_eq!(def.menu_path.as_deref(), Some("FTS/Session"));
    }

    #[test]
    fn action_definition_with_shortcut() {
        let def =
            ActionDefinition::new("fts.test.action", "Test", "desc").with_shortcut("Cmd+Shift+P");
        assert_eq!(def.shortcut_hint.as_deref(), Some("Cmd+Shift+P"));
    }

    #[test]
    fn action_definition_with_when() {
        let def =
            ActionDefinition::new("fts.test.action", "Test", "desc").with_when("tab:performance");
        assert_eq!(def.when.as_deref(), Some("tab:performance"));
    }

    #[test]
    fn action_definition_builder_chaining() {
        let def = ActionDefinition::new("fts.test.action", "Test", "desc")
            .with_category(ActionCategory::Session)
            .with_menu_path("FTS/Session")
            .with_shortcut("Ctrl+P")
            .with_when("tab:performance && !popup_open");
        assert_eq!(def.category, ActionCategory::Session);
        assert_eq!(def.menu_path.as_deref(), Some("FTS/Session"));
        assert_eq!(def.shortcut_hint.as_deref(), Some("Ctrl+P"));
        assert_eq!(def.when.as_deref(), Some("tab:performance && !popup_open"));
    }

    // ── ActionDefinition::is_active ──────────────────────────────────

    #[test]
    fn is_active_without_when_clause() {
        let def = ActionDefinition::new("fts.test.action", "Test", "desc");
        let ctx = ActionContext::new();
        assert!(def.is_active(&ctx));
    }

    #[test]
    fn is_active_when_clause_matches() {
        let def =
            ActionDefinition::new("fts.test.action", "Test", "desc").with_when("tab:performance");
        let mut ctx = ActionContext::new();
        ctx.set_tag("tab:performance");
        assert!(def.is_active(&ctx));
    }

    #[test]
    fn is_active_when_clause_does_not_match() {
        let def =
            ActionDefinition::new("fts.test.action", "Test", "desc").with_when("tab:performance");
        let ctx = ActionContext::new();
        assert!(!def.is_active(&ctx));
    }

    #[test]
    fn is_active_complex_when_clause() {
        let def = ActionDefinition::new("fts.test.action", "Test", "desc")
            .with_when("tab:performance && mode == normal");
        let mut ctx = ActionContext::new();
        ctx.set_tag("tab:performance");
        ctx.set_var("mode", "normal");
        assert!(def.is_active(&ctx));

        ctx.set_var("mode", "insert");
        assert!(!def.is_active(&ctx));
    }

    #[test]
    fn is_active_malformed_when_fails_open() {
        // Malformed when-clause should fail-open (return true)
        let def =
            ActionDefinition::new("fts.test.action", "Test", "desc").with_when("&& broken &&");
        let ctx = ActionContext::new();
        assert!(def.is_active(&ctx));
    }

    #[test]
    fn display_name_no_menu_path() {
        let def = ActionDefinition::new("fts.test.action", "Toggle Playback", "desc");
        assert_eq!(def.display_name(), "FTS: Toggle Playback");
    }

    #[test]
    fn display_name_with_menu_path() {
        let def = ActionDefinition::new("fts.sync.toggle_link", "Toggle Ableton Link", "desc")
            .with_menu_path("FTS/Sync");
        assert_eq!(def.display_name(), "FTS: Sync - Toggle Ableton Link");
    }

    #[test]
    fn display_name_with_nested_menu_path() {
        let def = ActionDefinition::new("fts.sync.puppet", "Puppet Mode", "desc")
            .with_menu_path("FTS/Sync/Link");
        assert_eq!(def.display_name(), "FTS: Sync - Link - Puppet Mode");
    }
}
