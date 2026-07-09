//! Utility functions shared across the monarchy crate.
//!
//! This module contains helper functions used internally by monarchy,
//! primarily for pattern matching operations.
//!
//! # Word Boundary Matching
//!
//! The [`contains_word`] function is the core pattern matching utility.
//! It matches patterns as whole words, respecting:
//!
//! - Traditional word boundaries (spaces, underscores, hyphens)
//! - CamelCase boundaries (lowercase followed by uppercase)
//! - Number boundaries (letter followed by digit or vice versa)
//!
//! ```ignore
//! use monarchy::utils::contains_word;
//!
//! // Traditional word boundaries
//! assert!(contains_word("Kick In", "kick"));
//! assert!(contains_word("kick_in", "kick"));
//! assert!(!contains_word("kickstart", "kick"));
//!
//! // CamelCase boundaries
//! assert!(contains_word("EdCrunch", "Ed"));
//! assert!(contains_word("EdCrunch", "Crunch"));
//!
//! // Number boundaries
//! assert!(contains_word("Crunch1", "Crunch"));
//! assert!(contains_word("DX7", "DX"));
//! ```

/// Check if text contains pattern as a whole word (not just as a substring).
///
/// A word is defined as a sequence of alphanumeric characters.
/// The pattern must be surrounded by non-alphanumeric characters, be at the start/end of the string,
/// or be at a camelCase boundary (lowercase followed by uppercase).
///
/// # Examples
///
/// ```
/// use monarchy::utils::contains_word;
///
/// assert!(contains_word("kick_in", "kick"));
/// assert!(contains_word("Kick In", "kick"));
/// assert!(contains_word("kick", "kick"));
/// assert!(!contains_word("kickstart", "kick"));
/// assert!(!contains_word("sidekick", "kick"));
/// // CamelCase support
/// assert!(contains_word("EdCrunch", "Ed"));
/// assert!(contains_word("EdCrunch", "Crunch"));
/// assert!(contains_word("JohnyLead", "Johny"));
/// assert!(contains_word("JohnyLead", "Lead"));
/// ```
pub fn contains_word(text: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();

    // Find all occurrences of the pattern
    let mut start = 0;
    while let Some(pos) = text_lower[start..].find(&pattern_lower) {
        let actual_pos = start + pos;
        let after_pos = actual_pos + pattern_lower.len();

        // Check if character before pattern is a word boundary
        let before_is_boundary = if actual_pos > 0 {
            let before_char = text.chars().nth(actual_pos - 1);
            let current_char = text.chars().nth(actual_pos);
            match (before_char, current_char) {
                (Some(b), Some(c)) => {
                    // Word boundary if:
                    // 1. Before char is not alphanumeric, OR
                    // 2. CamelCase: before is lowercase and current is uppercase
                    !b.is_alphanumeric() || (b.is_lowercase() && c.is_uppercase())
                }
                _ => true,
            }
        } else {
            true // At start, so it's a boundary
        };

        // Check if character after pattern is a word boundary
        let after_is_boundary = if after_pos < text.len() {
            let last_pattern_char = text.chars().nth(after_pos - 1);
            let after_char = text.chars().nth(after_pos);
            match (last_pattern_char, after_char) {
                (Some(l), Some(a)) => {
                    // Word boundary if:
                    // 1. After char is not alphanumeric, OR
                    // 2. CamelCase: last is lowercase/digit and after is uppercase, OR
                    // 3. Number boundary: last is alphabetic and after is digit, OR
                    // 4. Number boundary: last is digit and after is alphabetic
                    !a.is_alphanumeric()
                        || (l.is_lowercase() && a.is_uppercase())
                        || (l.is_alphabetic() && a.is_ascii_digit())
                        || (l.is_ascii_digit() && a.is_alphabetic())
                }
                _ => true,
            }
        } else {
            true // At end, so it's a boundary
        };

        // Pattern is a whole word if it's at word boundaries
        if before_is_boundary && after_is_boundary {
            return true;
        }

        // Continue searching from after this occurrence
        start = actual_pos + 1;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_word_basic() {
        assert!(contains_word("kick", "kick"));
        assert!(contains_word("Kick", "kick"));
        assert!(contains_word("KICK", "kick"));
    }

    #[test]
    fn test_contains_word_with_separators() {
        assert!(contains_word("kick_in", "kick"));
        assert!(contains_word("kick-in", "kick"));
        assert!(contains_word("kick in", "kick"));
        assert!(contains_word("Kick In", "in"));
    }

    #[test]
    fn test_contains_word_at_boundaries() {
        assert!(contains_word("kick", "kick"));
        assert!(contains_word("the kick drum", "kick"));
        assert!(contains_word("kick drum", "kick"));
        assert!(contains_word("the kick", "kick"));
    }

    #[test]
    fn test_contains_word_not_substring() {
        assert!(!contains_word("kickstart", "kick"));
        assert!(!contains_word("sidekick", "kick"));
        assert!(!contains_word("kicking", "kick"));
    }

    #[test]
    fn test_contains_word_empty_pattern() {
        assert!(!contains_word("kick", ""));
    }

    #[test]
    fn test_contains_word_pattern_longer_than_text() {
        assert!(!contains_word("ki", "kick"));
    }

    #[test]
    fn test_contains_word_camelcase() {
        // CamelCase: word boundaries at lowercase-to-uppercase transitions
        assert!(contains_word("EdCrunch", "Ed"));
        assert!(contains_word("EdCrunch", "Crunch"));
        assert!(contains_word("JohnyLead", "Johny"));
        assert!(contains_word("JohnyLead", "Lead"));
        assert!(contains_word("JohnyCrunch1", "Johny"));
        assert!(contains_word("JohnyCrunch1", "Crunch"));
        // Should not match partial words
        assert!(!contains_word("EdCrunch", "dCr"));
        assert!(!contains_word("EdCrunch", "run"));
    }
}
