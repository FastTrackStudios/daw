//! Keyboard shortcut string parser.
//!
//! Parses strings like "Cmd+Shift+P", "Space", "Right", "Ctrl+L", "F5"
//! into structured `KeyBinding` values.

use crate::{KeyBinding, KeyCode, Modifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Empty,
    UnknownKey(String),
    NoKeySpecified,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty shortcut string"),
            ParseError::UnknownKey(k) => write!(f, "unknown key: {}", k),
            ParseError::NoKeySpecified => write!(f, "no key specified (only modifiers)"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a shortcut string into a `KeyBinding`.
///
/// Format: `[Modifier+]*Key`
///
/// Modifiers: `Cmd`/`Meta`/`Super`, `Ctrl`/`Control`, `Alt`/`Option`, `Shift`
/// Keys: Single characters, `Space`, `Enter`, `Escape`, `Tab`, `Backspace`,
///       `Delete`, `Up`/`ArrowUp`, `Down`/`ArrowDown`, `Left`/`ArrowLeft`,
///       `Right`/`ArrowRight`, `F1`-`F12`
pub fn parse_shortcut(input: &str) -> Result<KeyBinding, ParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::Empty);
    }

    let parts: Vec<&str> = input.split('+').collect();
    let mut modifiers = Modifiers::default();
    let mut key: Option<KeyCode> = None;

    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        let is_last = i == parts.len() - 1;

        // Try to parse as modifier first (unless it's the last part and could be a key)
        match part.to_lowercase().as_str() {
            "cmd" | "meta" | "super" | "command" if !is_last || parts.len() > 1 => {
                modifiers.meta = true;
            }
            "ctrl" | "control" if !is_last || parts.len() > 1 => {
                modifiers.ctrl = true;
            }
            "alt" | "option" | "opt" if !is_last || parts.len() > 1 => {
                modifiers.alt = true;
            }
            "shift" if !is_last || parts.len() > 1 => {
                modifiers.shift = true;
            }
            _ => {
                // Must be the key
                if key.is_some() {
                    return Err(ParseError::UnknownKey(part.to_string()));
                }
                key = Some(parse_key(part)?);
            }
        }
    }

    match key {
        Some(k) => Ok(KeyBinding { key: k, modifiers }),
        None => Err(ParseError::NoKeySpecified),
    }
}

fn parse_key(s: &str) -> Result<KeyCode, ParseError> {
    // Check named keys (case-insensitive)
    match s.to_lowercase().as_str() {
        "space" => return Ok(KeyCode::Character(" ".into())),
        "enter" | "return" => return Ok(KeyCode::Enter),
        "escape" | "esc" => return Ok(KeyCode::Escape),
        "tab" => return Ok(KeyCode::Tab),
        "backspace" => return Ok(KeyCode::Backspace),
        "delete" | "del" => return Ok(KeyCode::Delete),
        "up" | "arrowup" => return Ok(KeyCode::ArrowUp),
        "down" | "arrowdown" => return Ok(KeyCode::ArrowDown),
        "left" | "arrowleft" => return Ok(KeyCode::ArrowLeft),
        "right" | "arrowright" => return Ok(KeyCode::ArrowRight),
        _ => {}
    }

    // Check function keys: F1-F12
    if s.len() >= 2 && s.starts_with('F') || s.starts_with('f') {
        if let Ok(n) = s[1..].parse::<u8>() {
            if (1..=12).contains(&n) {
                return Ok(KeyCode::F(n));
            }
        }
    }

    // Single character key
    if s.len() == 1 {
        return Ok(KeyCode::Character(s.to_lowercase()));
    }

    Err(ParseError::UnknownKey(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_char() {
        let kb = parse_shortcut("P").unwrap();
        assert_eq!(kb.key, KeyCode::Character("p".into()));
        assert_eq!(kb.modifiers, Modifiers::NONE);
    }

    #[test]
    fn parse_space() {
        let kb = parse_shortcut("Space").unwrap();
        assert_eq!(kb.key, KeyCode::Character(" ".into()));
    }

    #[test]
    fn parse_arrow_keys() {
        assert_eq!(parse_shortcut("Up").unwrap().key, KeyCode::ArrowUp);
        assert_eq!(parse_shortcut("Down").unwrap().key, KeyCode::ArrowDown);
        assert_eq!(parse_shortcut("Left").unwrap().key, KeyCode::ArrowLeft);
        assert_eq!(parse_shortcut("Right").unwrap().key, KeyCode::ArrowRight);
    }

    #[test]
    fn parse_special_keys() {
        assert_eq!(parse_shortcut("Enter").unwrap().key, KeyCode::Enter);
        assert_eq!(parse_shortcut("Escape").unwrap().key, KeyCode::Escape);
        assert_eq!(parse_shortcut("Tab").unwrap().key, KeyCode::Tab);
        assert_eq!(parse_shortcut("Backspace").unwrap().key, KeyCode::Backspace);
        assert_eq!(parse_shortcut("Delete").unwrap().key, KeyCode::Delete);
    }

    #[test]
    fn parse_function_keys() {
        assert_eq!(parse_shortcut("F1").unwrap().key, KeyCode::F(1));
        assert_eq!(parse_shortcut("F12").unwrap().key, KeyCode::F(12));
    }

    #[test]
    fn parse_cmd_shift_p() {
        let kb = parse_shortcut("Cmd+Shift+P").unwrap();
        assert_eq!(kb.key, KeyCode::Character("p".into()));
        assert!(kb.modifiers.meta);
        assert!(kb.modifiers.shift);
        assert!(!kb.modifiers.ctrl);
        assert!(!kb.modifiers.alt);
    }

    #[test]
    fn parse_ctrl_l() {
        let kb = parse_shortcut("Ctrl+L").unwrap();
        assert_eq!(kb.key, KeyCode::Character("l".into()));
        assert!(kb.modifiers.ctrl);
    }

    #[test]
    fn parse_alt_f5() {
        let kb = parse_shortcut("Alt+F5").unwrap();
        assert_eq!(kb.key, KeyCode::F(5));
        assert!(kb.modifiers.alt);
    }

    #[test]
    fn parse_empty_fails() {
        assert!(parse_shortcut("").is_err());
    }

    #[test]
    fn parse_unknown_key() {
        assert!(parse_shortcut("FooBar").is_err());
    }

    #[test]
    fn parse_case_insensitive() {
        let kb = parse_shortcut("cmd+shift+p").unwrap();
        assert!(kb.modifiers.meta);
        assert!(kb.modifiers.shift);
        assert_eq!(kb.key, KeyCode::Character("p".into()));
    }

    #[test]
    fn parse_with_spaces() {
        let kb = parse_shortcut("  Cmd + Shift + P  ").unwrap();
        assert!(kb.modifiers.meta);
        assert!(kb.modifiers.shift);
        assert_eq!(kb.key, KeyCode::Character("p".into()));
    }

    #[test]
    fn parse_l_key() {
        // "L" as a standalone key (not modifier)
        let kb = parse_shortcut("L").unwrap();
        assert_eq!(kb.key, KeyCode::Character("l".into()));
        assert_eq!(kb.modifiers, Modifiers::NONE);
    }
}
