//! Windows virtual key code → `input::KeyCode` mapping.
//!
//! Converts raw keyboard events from daw-bridge (Windows VK codes)
//! into the `input` crate's platform-agnostic key representation.

use daw::service::{KeyModifiers, RawKeyEvent};
use input::{KeyCode, KeyEvent, Modifiers};

/// Convert a raw key event from daw-bridge into an `input::KeyEvent`.
///
/// Returns `None` for unmapped or irrelevant VK codes.
pub fn raw_to_key_event(raw: &RawKeyEvent) -> Option<KeyEvent> {
    let key = vk_to_keycode(raw.vk_code)?;
    let modifiers = convert_modifiers(&raw.modifiers);
    Some(KeyEvent { key, modifiers })
}

fn convert_modifiers(m: &KeyModifiers) -> Modifiers {
    Modifiers {
        ctrl: m.ctrl,
        alt: m.alt,
        shift: m.shift,
        meta: false,
    }
}

/// Map a Windows virtual key code to `input::KeyCode`.
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
        // Function keys F1–F12 (0x70–0x7B)
        0x70..=0x7B => Some(KeyCode::F((vk - 0x70 + 1) as u8)),
        // Navigation
        0x25 => Some(KeyCode::ArrowLeft),
        0x26 => Some(KeyCode::ArrowUp),
        0x27 => Some(KeyCode::ArrowRight),
        0x28 => Some(KeyCode::ArrowDown),
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
