//! Keybinding and action dispatching for the actions system.
//!
//! Two dispatcher levels:
//!
//! - [`KeybindingDispatcher`] — Resolves keyboard events to action IDs. Low-level.
//! - [`ActionDispatcher`] — Combines keybinding resolution with handler execution.
//!   Registers action definitions (for keybindings) and handler closures (for execution).
//!   The recommended high-level API for UI apps.
//!
//! ## Usage (ActionDispatcher — recommended)
//!
//! ```ignore
//! use actions_keybindings::{ActionDispatcher, KeyCode, Modifiers};
//! use actions_proto::when::ActionContext;
//!
//! let mut dispatcher = ActionDispatcher::new();
//! dispatcher.register_actions(&session_actions::definitions());
//! dispatcher.on(session_actions::TOGGLE_PLAYBACK, || {
//!     // handler called when Space is pressed on performance tab
//! });
//!
//! // On keyboard event — single call resolves key → action → handler:
//! let ctx = ActionContext::new();
//! dispatcher.handle_key_event(&key, &mods, &ctx);
//! ```

mod parse;

pub use parse::ParseError;

use std::collections::HashMap;

use actions_proto::ActionDefinition;
use actions_proto::ids::StaticActionId;
use actions_proto::when::{ActionContext, WhenExpr};

// ============================================================================
// Key Types
// ============================================================================

/// A keyboard key code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A printable character key (lowercase). Space is `" "`.
    Character(String),
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    /// Function key (1-12).
    F(u8),
}

/// Modifier key state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
}

/// A parsed key binding (key + modifiers).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: KeyCode,
    pub modifiers: Modifiers,
}

impl KeyBinding {
    /// Parse a shortcut string like "Cmd+Shift+P", "Space", "Right", "F5".
    pub fn parse(shortcut: &str) -> Result<Self, ParseError> {
        parse::parse_shortcut(shortcut)
    }
}

impl std::fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.ctrl {
            parts.push("Ctrl");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        if self.modifiers.shift {
            parts.push("Shift");
        }
        if self.modifiers.meta {
            parts.push("Cmd");
        }
        let key_str = match &self.key {
            KeyCode::Character(c) if c == " " => "Space".to_string(),
            KeyCode::Character(c) => c.to_uppercase(),
            KeyCode::ArrowUp => "Up".to_string(),
            KeyCode::ArrowDown => "Down".to_string(),
            KeyCode::ArrowLeft => "Left".to_string(),
            KeyCode::ArrowRight => "Right".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Escape => "Escape".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::F(n) => format!("F{}", n),
        };
        parts.push(&key_str);
        write!(f, "{}", parts.join("+"))
    }
}

// ============================================================================
// Keybinding Dispatcher
// ============================================================================

struct BoundAction {
    key: KeyBinding,
    action_id: StaticActionId,
    when_expr: WhenExpr,
}

/// Dispatches keyboard events to matching actions, respecting when-clauses.
pub struct KeybindingDispatcher {
    bindings: Vec<BoundAction>,
}

impl KeybindingDispatcher {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Register bindings from action definitions.
    ///
    /// Parses `shortcut_hint` and `when` from each definition.
    /// Actions without a shortcut_hint are silently skipped.
    /// Invalid shortcut or when-clause strings are logged as warnings.
    pub fn register_actions(&mut self, actions: &[ActionDefinition]) {
        for action in actions {
            if let Some(ref shortcut) = action.shortcut_hint {
                match KeyBinding::parse(shortcut) {
                    Ok(key_binding) => {
                        let when_expr = match action.when.as_deref() {
                            Some(w) => match WhenExpr::parse(w) {
                                Ok(expr) => expr,
                                Err(e) => {
                                    tracing::warn!(
                                        action_id = %action.id,
                                        when_clause = %w,
                                        error = %e,
                                        "Invalid when-clause, action will always be active"
                                    );
                                    WhenExpr::Always
                                }
                            },
                            None => WhenExpr::Always,
                        };

                        self.bindings.push(BoundAction {
                            key: key_binding,
                            action_id: StaticActionId::new(
                                // Leak the string to get 'static — these are registered once at startup
                                Box::leak(action.id.as_str().to_string().into_boxed_str()),
                            ),
                            when_expr,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            action_id = %action.id,
                            shortcut = %shortcut,
                            error = %e,
                            "Failed to parse shortcut, skipping keybinding"
                        );
                    }
                }
            }
        }
    }

    /// Register a single binding manually.
    pub fn register(&mut self, key: KeyBinding, action_id: StaticActionId, when: WhenExpr) {
        self.bindings.push(BoundAction {
            key,
            action_id,
            when_expr: when,
        });
    }

    /// Match a keyboard event against all registered bindings.
    ///
    /// Returns the first matching action ID whose when-clause is satisfied.
    /// Later registrations take priority over earlier ones (last-wins for same key).
    pub fn match_event(
        &self,
        key: &KeyCode,
        modifiers: &Modifiers,
        ctx: &ActionContext,
    ) -> Option<StaticActionId> {
        // Iterate in reverse so later (more specific) bindings win
        for binding in self.bindings.iter().rev() {
            if &binding.key.key == key
                && &binding.key.modifiers == modifiers
                && binding.when_expr.evaluate(ctx)
            {
                return Some(binding.action_id);
            }
        }
        None
    }
}

impl Default for KeybindingDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Action Dispatcher (keybinding resolution + handler execution)
// ============================================================================

/// Combined keybinding + handler dispatcher.
///
/// Resolves keyboard events to action IDs via [`KeybindingDispatcher`],
/// then executes the matched action's registered handler closure.
///
/// This is the recommended high-level API for UI apps. Register action
/// definitions (for keybinding parsing) and handler closures (for execution)
/// at initialization, then call [`handle_key_event`](ActionDispatcher::handle_key_event)
/// on each keyboard event.
pub struct ActionDispatcher {
    keybindings: KeybindingDispatcher,
    handlers: HashMap<String, Box<dyn Fn()>>,
}

impl ActionDispatcher {
    pub fn new() -> Self {
        Self {
            keybindings: KeybindingDispatcher::new(),
            handlers: HashMap::new(),
        }
    }

    /// Register action definitions (parses shortcuts and when-clauses into keybindings).
    pub fn register_actions(&mut self, actions: &[ActionDefinition]) {
        self.keybindings.register_actions(actions);
    }

    /// Register a handler closure for an action ID.
    ///
    /// The handler is called when a keyboard event matches this action's keybinding
    /// and when-clause. Handlers typically call `spawn(async { ... })` for async work.
    pub fn on(&mut self, action_id: StaticActionId, handler: impl Fn() + 'static) {
        self.handlers
            .insert(action_id.as_str().to_string(), Box::new(handler));
    }

    /// Register a handler closure for a string action ID.
    pub fn on_str(&mut self, action_id: &str, handler: impl Fn() + 'static) {
        self.handlers
            .insert(action_id.to_string(), Box::new(handler));
    }

    /// Handle a keyboard event: resolve keybinding, then execute the handler.
    ///
    /// Returns `true` if a matching keybinding was found AND a handler was registered
    /// and executed. Returns `false` if no keybinding matched or no handler was found.
    pub fn handle_key_event(
        &self,
        key: &KeyCode,
        modifiers: &Modifiers,
        ctx: &ActionContext,
    ) -> bool {
        if let Some(action_id) = self.keybindings.match_event(key, modifiers, ctx) {
            tracing::debug!(action_id = %action_id, "Keyboard shortcut matched");
            if let Some(handler) = self.handlers.get(action_id.as_str()) {
                handler();
                return true;
            }
            tracing::warn!(action_id = %action_id, "No handler registered for matched action");
        }
        false
    }

    /// Check if a handler is registered for the given action ID.
    pub fn has_handler(&self, action_id: &str) -> bool {
        self.handlers.contains_key(action_id)
    }
}

impl Default for ActionDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use actions_proto::ActionDefinition;

    #[test]
    fn parse_simple_keys() {
        let kb = KeyBinding::parse("Space").unwrap();
        assert_eq!(kb.key, KeyCode::Character(" ".into()));
        assert_eq!(kb.modifiers, Modifiers::NONE);

        let kb = KeyBinding::parse("Right").unwrap();
        assert_eq!(kb.key, KeyCode::ArrowRight);

        let kb = KeyBinding::parse("Left").unwrap();
        assert_eq!(kb.key, KeyCode::ArrowLeft);

        let kb = KeyBinding::parse("Up").unwrap();
        assert_eq!(kb.key, KeyCode::ArrowUp);

        let kb = KeyBinding::parse("Down").unwrap();
        assert_eq!(kb.key, KeyCode::ArrowDown);
    }

    #[test]
    fn parse_modified_keys() {
        let kb = KeyBinding::parse("Cmd+Shift+P").unwrap();
        assert_eq!(kb.key, KeyCode::Character("p".into()));
        assert!(kb.modifiers.meta);
        assert!(kb.modifiers.shift);
        assert!(!kb.modifiers.ctrl);
        assert!(!kb.modifiers.alt);

        let kb = KeyBinding::parse("Ctrl+L").unwrap();
        assert_eq!(kb.key, KeyCode::Character("l".into()));
        assert!(kb.modifiers.ctrl);
    }

    #[test]
    fn parse_function_keys() {
        let kb = KeyBinding::parse("F5").unwrap();
        assert_eq!(kb.key, KeyCode::F(5));

        let kb = KeyBinding::parse("Ctrl+F12").unwrap();
        assert_eq!(kb.key, KeyCode::F(12));
        assert!(kb.modifiers.ctrl);
    }

    #[test]
    fn dispatcher_basic_match() {
        let mut dispatcher = KeybindingDispatcher::new();
        let toggle = StaticActionId::new("fts.session.toggle_playback");

        dispatcher.register(
            KeyBinding {
                key: KeyCode::Character(" ".into()),
                modifiers: Modifiers::NONE,
            },
            toggle,
            WhenExpr::Always,
        );

        let ctx = ActionContext::new();
        let result =
            dispatcher.match_event(&KeyCode::Character(" ".into()), &Modifiers::NONE, &ctx);
        assert_eq!(result, Some(toggle));
    }

    #[test]
    fn dispatcher_respects_when_clause() {
        let mut dispatcher = KeybindingDispatcher::new();
        let toggle = StaticActionId::new("fts.session.toggle_playback");

        dispatcher.register(
            KeyBinding {
                key: KeyCode::Character(" ".into()),
                modifiers: Modifiers::NONE,
            },
            toggle,
            WhenExpr::Tag("tab:performance".into()),
        );

        let mut ctx = ActionContext::new();

        // No tag set — should not match
        let result =
            dispatcher.match_event(&KeyCode::Character(" ".into()), &Modifiers::NONE, &ctx);
        assert_eq!(result, None);

        // Set the tag — should match
        ctx.set_tag("tab:performance");
        let result =
            dispatcher.match_event(&KeyCode::Character(" ".into()), &Modifiers::NONE, &ctx);
        assert_eq!(result, Some(toggle));
    }

    #[test]
    fn dispatcher_from_definitions() {
        let actions = vec![
            ActionDefinition::new("a", "Toggle", "desc")
                .with_shortcut("Space")
                .with_when("tab:performance"),
            ActionDefinition::new("b", "Next", "desc").with_shortcut("Right"),
        ];

        let mut dispatcher = KeybindingDispatcher::new();
        dispatcher.register_actions(&actions);

        let mut ctx = ActionContext::new();
        ctx.set_tab("performance");

        // Space should match action "a" (when clause satisfied)
        let result =
            dispatcher.match_event(&KeyCode::Character(" ".into()), &Modifiers::NONE, &ctx);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), "a");

        // Right should match action "b" (no when clause = always)
        let result = dispatcher.match_event(&KeyCode::ArrowRight, &Modifiers::NONE, &ctx);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), "b");
    }

    #[test]
    fn display_keybinding() {
        let kb = KeyBinding::parse("Cmd+Shift+P").unwrap();
        assert_eq!(kb.to_string(), "Shift+Cmd+P");

        let kb = KeyBinding::parse("Space").unwrap();
        assert_eq!(kb.to_string(), "Space");
    }

    // ActionDispatcher tests

    #[test]
    fn action_dispatcher_registers_and_executes() {
        use std::cell::Cell;
        use std::rc::Rc;

        let called = Rc::new(Cell::new(false));
        let called_clone = called.clone();

        let mut dispatcher = ActionDispatcher::new();
        dispatcher.register_actions(&[
            ActionDefinition::new("test.action", "Test", "desc").with_shortcut("Space")
        ]);
        dispatcher.on(StaticActionId::new("test.action"), move || {
            called_clone.set(true);
        });

        let ctx = ActionContext::new();
        let handled =
            dispatcher.handle_key_event(&KeyCode::Character(" ".into()), &Modifiers::NONE, &ctx);
        assert!(handled);
        assert!(called.get());
    }

    #[test]
    fn action_dispatcher_returns_false_for_unmatched_key() {
        let mut dispatcher = ActionDispatcher::new();
        dispatcher.register_actions(&[
            ActionDefinition::new("test.action", "Test", "desc").with_shortcut("Space")
        ]);
        dispatcher.on(StaticActionId::new("test.action"), || {});

        let ctx = ActionContext::new();
        let handled = dispatcher.handle_key_event(&KeyCode::ArrowUp, &Modifiers::NONE, &ctx);
        assert!(!handled);
    }

    #[test]
    fn action_dispatcher_returns_false_for_missing_handler() {
        let mut dispatcher = ActionDispatcher::new();
        // Register keybinding but no handler
        dispatcher.register_actions(&[
            ActionDefinition::new("test.action", "Test", "desc").with_shortcut("Space")
        ]);

        let ctx = ActionContext::new();
        let handled =
            dispatcher.handle_key_event(&KeyCode::Character(" ".into()), &Modifiers::NONE, &ctx);
        assert!(!handled);
    }

    #[test]
    fn action_dispatcher_respects_when_clause() {
        use std::cell::Cell;
        use std::rc::Rc;

        let called = Rc::new(Cell::new(false));
        let called_clone = called.clone();

        let mut dispatcher = ActionDispatcher::new();
        dispatcher.register_actions(&[ActionDefinition::new("test.action", "Test", "desc")
            .with_shortcut("Space")
            .with_when("tab:performance")]);
        dispatcher.on(StaticActionId::new("test.action"), move || {
            called_clone.set(true);
        });

        // Wrong tab — should not fire
        let mut ctx = ActionContext::new();
        ctx.set_tab("settings");
        assert!(!dispatcher.handle_key_event(
            &KeyCode::Character(" ".into()),
            &Modifiers::NONE,
            &ctx,
        ));
        assert!(!called.get());

        // Right tab — should fire
        ctx.set_tab("performance");
        assert!(dispatcher.handle_key_event(
            &KeyCode::Character(" ".into()),
            &Modifiers::NONE,
            &ctx,
        ));
        assert!(called.get());
    }

    #[test]
    fn action_dispatcher_has_handler() {
        let mut dispatcher = ActionDispatcher::new();
        dispatcher.on(StaticActionId::new("test.a"), || {});

        assert!(dispatcher.has_handler("test.a"));
        assert!(!dispatcher.has_handler("test.b"));
    }
}
