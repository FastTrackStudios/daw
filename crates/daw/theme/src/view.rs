//! What a view declares it draws with.
//!
//! Each view crate (the arrangement, the mixer, the MIDI editor) says which
//! tokens it reads, which pages it asks a pack for and which fields it
//! hands those pages, as one [`Uses`]. [`Uses::check`] holds the
//! declaration to the spec: a name the spec lacks is a view drawing with
//! something no pack can implement.

use crate::spec;

/// One view's use of the spec.
#[derive(Clone, Copy, Debug)]
pub struct Uses {
    /// The view's name: `arrangement`, `mixer`, `editor`.
    pub view: &'static str,
    pub tokens: &'static [&'static str],
    pub parts: &'static [&'static str],
    pub fields: &'static [&'static str],
}

/// A name a view declares that the spec does not have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unknown {
    Token(&'static str),
    Part(&'static str),
    Field(&'static str),
}

impl std::fmt::Display for Unknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Token(n) => write!(f, "token `{n}` is not in the spec"),
            Self::Part(n) => write!(f, "part `{n}` is not in the spec"),
            Self::Field(n) => write!(f, "field `{n}` is not in the spec"),
        }
    }
}

impl Uses {
    /// Every name this view declares that the spec lacks.
    #[must_use]
    pub fn check(&self) -> Vec<Unknown> {
        let tokens = self
            .tokens
            .iter()
            .filter(|n| spec::token(n).is_none())
            .map(|n| Unknown::Token(n));
        let parts = self
            .parts
            .iter()
            .filter(|n| spec::part(n).is_none())
            .map(|n| Unknown::Part(n));
        let fields = self
            .fields
            .iter()
            .filter(|n| spec::field_named(n).is_none())
            .map(|n| Unknown::Field(n));
        tokens.chain(parts).chain(fields).collect()
    }

    /// Whether this view declares `token`.
    #[must_use]
    pub fn reads(&self, token: &str) -> bool {
        self.tokens.contains(&token)
    }

    /// Whether this view declares `part` (or the family it belongs to).
    #[must_use]
    pub fn asks_for(&self, part: &str) -> bool {
        self.parts.contains(&part)
            || spec::part(part).is_some_and(|p| p.family && self.parts.contains(&p.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_the_spec_lacks_is_reported() {
        let uses = Uses {
            view: "test",
            tokens: &["mute", "mutee"],
            parts: &["mcp.strip", "mcp.strips"],
            fields: &["track.name", "track.nam"],
        };
        assert_eq!(
            uses.check(),
            vec![
                Unknown::Token("mutee"),
                Unknown::Part("mcp.strips"),
                Unknown::Field("track.nam"),
            ]
        );
    }

    #[test]
    fn a_family_covers_its_particular_pages() {
        let uses = Uses {
            view: "test",
            tokens: &[],
            parts: &["fx.macro"],
            fields: &[],
        };
        assert!(uses.asks_for("fx.macro.tone"));
        assert!(!uses.asks_for("mcp.strip"));
    }
}
