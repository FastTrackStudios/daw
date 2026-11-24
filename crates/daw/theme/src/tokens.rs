//! A token's value, falling back as the spec says when a pack leaves it out.

use frame::theme::runtime::{Rgba, Theme, Value};

use crate::spec::{self, Fallback};

/// Where a token's value came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    /// The pack set it, or a token it falls back to.
    Pack(Value),
    /// The spec's own value: nothing the pack set reaches it.
    Default(Value),
    /// Neither: the view draws its built-in look.
    Host,
}

impl Resolved {
    /// The value, if the pack or the spec has one.
    #[must_use]
    pub const fn value(&self) -> Option<&Value> {
        match self {
            Self::Pack(v) | Self::Default(v) => Some(v),
            Self::Host => None,
        }
    }
}

/// The tokens of a loaded theme (or of none), looked up by name.
#[derive(Clone, Copy)]
pub struct Tokens<'a> {
    theme: Option<&'a Theme>,
}

impl<'a> Tokens<'a> {
    #[must_use]
    pub const fn new(theme: Option<&'a Theme>) -> Self {
        Self { theme }
    }

    /// `name`'s value: the pack's, else what the spec says it falls back
    /// to. A name the spec lacks is the pack's or the host's.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Resolved {
        let mut at = name;
        // A chain is no longer than the spec (its test holds it acyclic).
        for _ in 0..=spec::TOKENS.len() {
            if let Some(v) = self.theme.and_then(|t| t.token(at)) {
                return Resolved::Pack(v.clone());
            }
            match spec::token(at).map(|t| t.fallback) {
                Some(Fallback::Token(next)) => at = next,
                Some(Fallback::Value(literal)) => return Resolved::Default(parse(literal)),
                Some(Fallback::Host) | None => return Resolved::Host,
            }
        }
        Resolved::Host
    }

    /// `name` as a colour; `None` for the host's own.
    #[must_use]
    pub fn color(&self, name: &str) -> Option<Rgba> {
        self.resolve(name).value().and_then(Value::color)
    }

    /// `name` as a number; `None` for the host's own.
    #[must_use]
    pub fn number(&self, name: &str) -> Option<f64> {
        self.resolve(name).value().and_then(Value::num)
    }

    /// `name` as a switch; `None` for the host's own.
    #[must_use]
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.resolve(name).value().and_then(Value::truthy)
    }

    /// `name` as text (a font's family); `None` for the host's own.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<String> {
        self.resolve(name)
            .value()
            .and_then(Value::as_text)
            .map(|t| t.to_string())
    }
}

/// A literal in Frame's syntax: a colour, a number, a switch or text.
fn parse(literal: &str) -> Value {
    if let Some(c) = literal
        .starts_with('#')
        .then(|| Rgba::from_hex(literal))
        .flatten()
    {
        return Value::Color(c);
    }
    match literal {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => literal
            .parse::<f64>()
            .map_or_else(|_| Value::text(literal), Value::Num),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(vars: &str) -> Theme {
        let src = format!("<Document><Variables modes=\"dark\">{vars}</Variables></Document>");
        Theme::from_markup(&src, spec::schema(), None).expect("compiles")
    }

    #[test]
    fn a_token_the_pack_sets_is_the_packs() {
        let t = theme(r##"<Var name="accent" value="#ff0000" />"##);
        let tokens = Tokens::new(Some(&t));
        assert_eq!(tokens.color("accent"), Rgba::from_hex("#ff0000"));
    }

    #[test]
    fn a_missing_token_follows_its_fallback_to_one_the_pack_sets() {
        // `route.out` falls back to `accent`.
        let t = theme(r##"<Var name="accent" value="#00ff00" />"##);
        let tokens = Tokens::new(Some(&t));
        assert_eq!(
            tokens.resolve("route.out"),
            Resolved::Pack(Value::Color(Rgba::from_hex("#00ff00").expect("hex")))
        );
    }

    #[test]
    fn a_missing_token_with_nothing_set_is_the_specs_or_the_hosts() {
        let t = theme(r##"<Var name="text" value="#ffffff" />"##);
        let tokens = Tokens::new(Some(&t));
        assert_eq!(tokens.number("radius.control"), Some(3.0));
        assert_eq!(tokens.flag("strip.live"), Some(false));
        assert_eq!(tokens.text("font.ui").as_deref(), Some("Inter"));
        assert_eq!(tokens.resolve("mute"), Resolved::Host);
        assert_eq!(Tokens::new(None).resolve("mute"), Resolved::Host);
    }
}
