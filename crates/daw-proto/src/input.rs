//! Input Service — keyboard/mouse event interception and streaming.
//!
//! The host intercepts input events from the DAW and streams them to extension
//! processes over SHM. Extensions process keybindings, modal editing, and
//! command resolution — then call back to execute actions.
//!
//! # Latency Design
//!
//! The host's keyboard hook runs synchronously on the DAW's main thread.
//! To avoid per-keypress SHM round-trips, the extension uploads a [`KeyFilter`]
//! that the host evaluates locally. Eaten keys are streamed asynchronously.
//!
//! # Platform Agnosticism
//!
//! The wire format uses [`KeyCode`] — a platform-agnostic key representation.
//! The host is responsible for converting platform-specific key codes (e.g.
//! Windows VK codes) into `KeyCode` before streaming to extensions.

use facet::Facet;
use vox::{Tx, service};

// =========================================================================
// Platform-agnostic key code
// =========================================================================

/// Platform-agnostic keyboard key code.
///
/// The host converts platform-specific codes (Windows VK, macOS keycodes)
/// into this enum before streaming to extensions.
#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Facet)]
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
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    /// Function key (1–24).
    F(u8),
}

// =========================================================================
// Key event types
// =========================================================================

/// Keyboard event from the host's input hook.
#[derive(Debug, Clone, Facet)]
pub struct KeyEvent {
    /// Platform-agnostic key code.
    pub key: KeyCode,
    /// Active modifier keys.
    pub modifiers: KeyModifiers,
    /// Type of keyboard message.
    pub msg_kind: KeyMsgKind,
    /// Which DAW window context has focus.
    pub context: InputContext,
    /// Whether a text input field currently has focus.
    pub is_text_focused: bool,
}

/// Modifier key state at the time of a key event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Facet)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Type of keyboard message.
#[repr(u8)]
#[derive(Debug, Clone, Copy, Facet)]
pub enum KeyMsgKind {
    KeyDown,
    KeyUp,
    SysKeyDown,
    SysKeyUp,
    Char,
}

/// Which DAW window context has focus.
#[repr(u8)]
#[derive(Debug, Clone, Copy, Facet)]
pub enum InputContext {
    /// Main arrange view.
    Main,
    /// MIDI editor (floating or docked).
    Midi,
    /// Inline MIDI editor in arrange.
    MidiInline,
    /// Media explorer.
    MediaExplorer,
    /// Global context (applies everywhere).
    Global,
}

// =========================================================================
// Key filter (uploaded from extension to host)
// =========================================================================

/// Describes which keys the host should eat (intercept).
///
/// The extension uploads this filter to the host. The host evaluates it
/// synchronously — no SHM round-trip per keypress.
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum KeyFilter {
    /// Eat all keys (except when text fields are focused).
    EatAll,
    /// Pass all keys through to the DAW (extension is passive).
    PassAll,
    /// Eat only keys matching specific patterns.
    EatMatching { patterns: Vec<KeyPattern> },
}

/// A specific key + modifier combination to match against.
#[derive(Debug, Clone, Facet)]
pub struct KeyPattern {
    /// Key to match.
    pub key: KeyCode,
    /// Required modifier state.
    pub modifiers: KeyModifiers,
    /// If true, modifiers must match exactly (no extra modifiers allowed).
    pub exact_modifiers: bool,
}

// =========================================================================
// Input stream events
// =========================================================================

/// Events streamed from the host to extension processes.
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum InputEvent {
    /// A keyboard event that was eaten by the filter.
    Key(KeyEvent),
    /// A mouse wheel event.
    MouseWheel {
        delta: i16,
        horizontal: bool,
        context: InputContext,
    },
}

// =========================================================================
// Service trait
// =========================================================================

/// Raw input interception and streaming service.
///
/// The host captures keyboard/mouse events from REAPER's TranslateAccel hook
/// and streams them to subscribing extensions. Extensions upload a [`KeyFilter`]
/// to control which keys are eaten (intercepted) vs passed through to REAPER.
///
/// This service is domain-agnostic — the host has no knowledge of keybindings,
/// modal editing, or command resolution. Extensions handle all input processing.
#[service]
pub trait InputService {
    /// Subscribe to input events.
    ///
    /// The host streams all events that match the current key filter.
    /// Multiple subscribers are supported (broadcast).
    async fn subscribe_input(&self, tx: Tx<InputEvent>);

    /// Upload a key filter configuration.
    ///
    /// The host uses this to decide synchronously (in TranslateAccel) which
    /// keys to eat. Replaces the previous filter.
    async fn set_key_filter(&self, filter: KeyFilter);

    /// Get the current key filter.
    async fn get_key_filter(&self) -> KeyFilter;

    /// Enable or disable the input interception system entirely.
    ///
    /// When disabled, TranslateAccel passes all keys through to REAPER.
    async fn set_enabled(&self, enabled: bool);

    /// Check if input interception is currently enabled.
    async fn is_enabled(&self) -> bool;

    /// Execute a REAPER action by command name or numeric ID.
    ///
    /// Extensions call this after resolving a keybinding to an action.
    /// The host dispatches to REAPER's main thread.
    ///
    /// Supports:
    /// - Named commands: `"FTS_SIGNAL_NEXT_SONG"`
    /// - Named with prefix: `"_SWS_ABOUT"`
    /// - Numeric IDs: `"40044"` (undo)
    async fn execute_action(&self, action_id: String);
}
