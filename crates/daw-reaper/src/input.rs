//! REAPER Input Service Implementation
//!
//! Registers a TranslateAccel handler that intercepts keyboard events
//! and streams them to extension processes via a broadcast channel.
//! The key filter is evaluated synchronously — no SHM round-trip per keypress.
//!
//! VK codes from Windows/SWELL are converted to platform-agnostic `KeyCode`
//! before being sent to extensions.

use crate::main_thread;
use daw_proto::{
    InputContext, InputEvent, InputService, KeyCode, KeyEvent, KeyFilter, KeyModifiers, KeyMsgKind,
};
use reaper_high::Reaper;
use reaper_medium::{
    AccelMsgKind, AcceleratorBehavior, AcceleratorPosition, TranslateAccel, TranslateAccelArgs,
    TranslateAccelResult,
};
use roam::Tx;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tracing::{debug, info};

// =========================================================================
// Shared state between service and TranslateAccel handler
// =========================================================================

/// Shared state accessed by both the async service methods and the
/// synchronous TranslateAccel callback on REAPER's main thread.
struct InputState {
    /// Master enable switch. Checked atomically in TranslateAccel.
    enabled: AtomicBool,
    /// Key filter — determines which keys to eat. Read via `RwLock::read()`
    /// in TranslateAccel (non-blocking, no SHM round-trip).
    filter: RwLock<KeyFilter>,
    /// Broadcast channel for streaming eaten events to subscribers.
    event_tx: broadcast::Sender<InputEvent>,
}

impl InputState {
    fn new() -> Self {
        let (event_tx, _) = broadcast::channel::<InputEvent>(256);
        Self {
            enabled: AtomicBool::new(false),
            filter: RwLock::new(KeyFilter::PassAll),
            event_tx,
        }
    }
}

// =========================================================================
// TranslateAccel handler
// =========================================================================

/// The actual keyboard hook registered with REAPER.
struct InputAccelHandler {
    state: Arc<InputState>,
}

impl TranslateAccel for InputAccelHandler {
    fn call(&mut self, args: TranslateAccelArgs) -> TranslateAccelResult {
        // Fast path: if disabled, pass everything through
        if !self.state.enabled.load(Ordering::Relaxed) {
            return TranslateAccelResult::NotOurWindow;
        }

        let msg = args.msg;
        let msg_kind = match msg.message() {
            AccelMsgKind::KeyDown => KeyMsgKind::KeyDown,
            AccelMsgKind::KeyUp => KeyMsgKind::KeyUp,
            AccelMsgKind::SysKeyDown => KeyMsgKind::SysKeyDown,
            AccelMsgKind::SysKeyUp => KeyMsgKind::SysKeyUp,
            AccelMsgKind::Char => KeyMsgKind::Char,
            _ => return TranslateAccelResult::NotOurWindow,
        };

        let behavior = msg.behavior();
        let modifiers = KeyModifiers {
            ctrl: behavior.contains(AcceleratorBehavior::Control),
            alt: behavior.contains(AcceleratorBehavior::Alt),
            shift: behavior.contains(AcceleratorBehavior::Shift),
        };

        let vk_code = msg.key().get() as u32;

        // Convert VK code to platform-agnostic KeyCode
        let key = match vk_to_keycode(vk_code) {
            Some(k) => k,
            None => return TranslateAccelResult::NotOurWindow,
        };

        // Detect if a text field is focused (using SWELL/Win32 focus check)
        let is_text_focused = is_text_field_focused();

        // Always pass through when text is focused
        if is_text_focused {
            return TranslateAccelResult::NotOurWindow;
        }

        // Evaluate filter synchronously
        let should_eat = {
            let filter = self.state.filter.read().unwrap();
            match &*filter {
                KeyFilter::PassAll => false,
                KeyFilter::EatAll => true,
                KeyFilter::EatMatching { patterns } => {
                    patterns.iter().any(|p| matches_filter(p, &key, &modifiers))
                }
            }
        };

        if !should_eat {
            return TranslateAccelResult::NotOurWindow;
        }

        // Build event and broadcast
        let context = detect_input_context();
        let event = InputEvent::Key(KeyEvent {
            key,
            modifiers,
            msg_kind,
            context,
            is_text_focused,
        });

        // Non-blocking send — if no subscribers or channel full, that's fine
        let _ = self.state.event_tx.send(event);

        TranslateAccelResult::Eat
    }
}

/// Check if a text input field currently has focus.
fn is_text_field_focused() -> bool {
    // Use SWELL GetFocus + GetClassName to check if the focused
    // window is an edit control. This runs on the main thread (TranslateAccel).
    unsafe {
        let swell = reaper_low::Swell::get();
        let focused = swell.GetFocus();
        if focused.is_null() {
            return false;
        }
        let mut class_name = [0u8; 64];
        let len = swell.GetClassName(
            focused,
            class_name.as_mut_ptr() as *mut _,
            class_name.len() as i32,
        );
        if len <= 0 {
            return false;
        }
        let name = std::str::from_utf8_unchecked(&class_name[..len as usize]);
        // Edit controls: "Edit" on Windows, "Edit" or "RichEdit*" on SWELL
        name.eq_ignore_ascii_case("Edit")
            || name.starts_with("RichEdit")
            || name.eq_ignore_ascii_case("RICHEDIT50W")
    }
}

/// Detect which REAPER context has focus.
///
/// Simple heuristic: check if the MIDI editor is open and focused.
/// More sophisticated detection can be added later.
fn detect_input_context() -> InputContext {
    // For now, default to Main. The extension can refine context
    // based on window information if needed.
    InputContext::Main
}

// =========================================================================
// VK code → KeyCode conversion (REAPER-specific)
// =========================================================================

/// Convert a Windows/SWELL virtual key code to a platform-agnostic `KeyCode`.
///
/// Returns `None` for unmapped or irrelevant VK codes.
fn vk_to_keycode(vk: u32) -> Option<KeyCode> {
    match vk {
        // Letters A–Z (0x41–0x5A)
        0x41..=0x5A => {
            let ch = (vk as u8) as char;
            Some(KeyCode::Character(ch.to_lowercase().to_string()))
        }
        // Digits 0–9 (0x30–0x39)
        0x30..=0x39 => {
            let ch = (vk as u8) as char;
            Some(KeyCode::Character(ch.to_string()))
        }
        // Function keys F1–F24 (0x70–0x87)
        0x70..=0x87 => Some(KeyCode::F((vk - 0x70 + 1) as u8)),
        // Navigation
        0x25 => Some(KeyCode::ArrowLeft),
        0x26 => Some(KeyCode::ArrowUp),
        0x27 => Some(KeyCode::ArrowRight),
        0x28 => Some(KeyCode::ArrowDown),
        0x24 => Some(KeyCode::Home),
        0x23 => Some(KeyCode::End),
        0x21 => Some(KeyCode::PageUp),
        0x22 => Some(KeyCode::PageDown),
        0x2D => Some(KeyCode::Insert),
        // Editing keys
        0x0D => Some(KeyCode::Enter),
        0x1B => Some(KeyCode::Escape),
        0x09 => Some(KeyCode::Tab),
        0x08 => Some(KeyCode::Backspace),
        0x2E => Some(KeyCode::Delete),
        // Space
        0x20 => Some(KeyCode::Character(" ".to_string())),
        // Common punctuation via OEM keys
        0xBA => Some(KeyCode::Character(";".to_string())),
        0xBB => Some(KeyCode::Character("=".to_string())),
        0xBC => Some(KeyCode::Character(",".to_string())),
        0xBD => Some(KeyCode::Character("-".to_string())),
        0xBE => Some(KeyCode::Character(".".to_string())),
        0xBF => Some(KeyCode::Character("/".to_string())),
        0xC0 => Some(KeyCode::Character("`".to_string())),
        0xDB => Some(KeyCode::Character("[".to_string())),
        0xDC => Some(KeyCode::Character("\\".to_string())),
        0xDD => Some(KeyCode::Character("]".to_string())),
        0xDE => Some(KeyCode::Character("'".to_string())),
        _ => None,
    }
}

// =========================================================================
// EatMatching filter evaluation
// =========================================================================

fn matches_filter(
    pattern: &daw_proto::KeyPattern,
    key: &KeyCode,
    modifiers: &KeyModifiers,
) -> bool {
    if pattern.key != *key {
        return false;
    }
    if pattern.exact_modifiers {
        pattern.modifiers.ctrl == modifiers.ctrl
            && pattern.modifiers.alt == modifiers.alt
            && pattern.modifiers.shift == modifiers.shift
    } else {
        // Non-exact: pattern modifiers must be present, but extra modifiers are OK
        (!pattern.modifiers.ctrl || modifiers.ctrl)
            && (!pattern.modifiers.alt || modifiers.alt)
            && (!pattern.modifiers.shift || modifiers.shift)
    }
}

// =========================================================================
// Service implementation
// =========================================================================

/// REAPER input service.
///
/// Created once during daw-bridge initialization. Registers the
/// TranslateAccel handler and provides the async service interface.
#[derive(Clone)]
pub struct ReaperInput {
    state: Arc<InputState>,
}

impl ReaperInput {
    /// Create the input service and register the TranslateAccel handler.
    ///
    /// Must be called on the main thread (during extension init).
    pub fn new() -> Self {
        let state = Arc::new(InputState::new());

        // Register the TranslateAccel handler with REAPER
        let handler = Box::new(InputAccelHandler {
            state: state.clone(),
        });
        let reaper = Reaper::get();
        let mut session = reaper.medium_session();
        match session.plugin_register_add_accelerator_register(handler, AcceleratorPosition::Front)
        {
            Ok(_handle) => {
                // Leak the handle — the accelerator lives for the process lifetime.
                // We control enable/disable via the atomic bool.
                info!("Input TranslateAccel handler registered");
            }
            Err(e) => {
                tracing::error!("Failed to register TranslateAccel handler: {e}");
            }
        }

        Self { state }
    }
}

impl InputService for ReaperInput {
    async fn subscribe_input(&self, tx: Tx<InputEvent>) {
        let mut rx = self.state.event_tx.subscribe();
        info!(
            "Input event subscriber added (receivers: {})",
            self.state.event_tx.receiver_count()
        );

        moire::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            debug!("Input event subscriber disconnected");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!("Input event subscriber lagged by {count} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Input broadcast channel closed");
                        break;
                    }
                }
            }
        });
    }

    async fn set_key_filter(&self, filter: KeyFilter) {
        let label = match &filter {
            KeyFilter::EatAll => "EatAll".to_string(),
            KeyFilter::PassAll => "PassAll".to_string(),
            KeyFilter::EatMatching { patterns } => {
                format!("EatMatching({} patterns)", patterns.len())
            }
        };
        *self.state.filter.write().unwrap() = filter;
        info!("Key filter updated: {label}");
    }

    async fn get_key_filter(&self) -> KeyFilter {
        self.state.filter.read().unwrap().clone()
    }

    async fn set_enabled(&self, enabled: bool) {
        self.state.enabled.store(enabled, Ordering::Relaxed);
        info!(
            "Input interception {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    async fn is_enabled(&self) -> bool {
        self.state.enabled.load(Ordering::Relaxed)
    }

    async fn execute_action(&self, action_id: String) {
        main_thread::run(move || {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Try parsing as numeric command ID first
            let cmd_id = if let Ok(numeric_id) = action_id.parse::<u32>() {
                Some(numeric_id)
            } else if let Some(id) = medium.named_command_lookup(action_id.as_str()) {
                Some(id.get())
            } else {
                // Try with underscore prefix (REAPER convention)
                let prefixed = format!("_{action_id}");
                medium
                    .named_command_lookup(prefixed.as_str())
                    .map(|id| id.get())
            };

            match cmd_id {
                Some(id) if id > 0 => {
                    medium.low().Main_OnCommand(id as i32, 0);
                    debug!("Executed action '{}' (cmd_id={})", action_id, id);
                }
                _ => {
                    debug!("Action not found: '{}'", action_id);
                }
            }
        });
    }
}
