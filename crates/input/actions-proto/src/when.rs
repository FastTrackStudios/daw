//! Expression-based when-clauses for action activation contexts.
//!
//! When-clauses determine whether an action is active given the current
//! application state. They start simple and scale to complex modal editing.
//!
//! ## Expression Language
//!
//! ```text
//! "tab:performance"                       → tag presence check
//! "mode == normal"                        → variable equality
//! "!popup_open"                           → negated tag
//! "tab:performance && mode == normal"     → conjunction
//! "mode == normal || mode == visual"      → disjunction
//! "tab:performance && !popup_open"        → combined
//! ```
//!
//! ## Precedence
//!
//! `!` (highest) > `&&` > `||` (lowest)

use std::collections::{HashMap, HashSet};
use std::fmt;

// ============================================================================
// WhenExpr AST
// ============================================================================

/// A parsed when-clause expression.
#[derive(Debug, Clone, PartialEq)]
pub enum WhenExpr {
    /// Tag presence check: `"tab:performance"`
    Tag(String),
    /// Variable equality: `"mode == normal"`
    Eq(String, String),
    /// Variable inequality: `"mode != insert"`
    Neq(String, String),
    /// Negation: `"!popup_open"`
    Not(Box<WhenExpr>),
    /// Conjunction: `"a && b"`
    And(Box<WhenExpr>, Box<WhenExpr>),
    /// Disjunction: `"a || b"`
    Or(Box<WhenExpr>, Box<WhenExpr>),
    /// Always true (empty when clause).
    Always,
}

impl WhenExpr {
    /// Parse a when-clause expression string.
    ///
    /// Returns `Always` for empty/whitespace-only input.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(WhenExpr::Always);
        }
        let tokens = tokenize(input)?;
        let mut pos = 0;
        let expr = parse_or(&tokens, &mut pos)?;
        if pos < tokens.len() {
            return Err(ParseError::UnexpectedToken(format!(
                "unexpected token at position {}: {:?}",
                pos, tokens[pos]
            )));
        }
        Ok(expr)
    }

    /// Evaluate this expression against an `ActionContext`.
    pub fn evaluate(&self, ctx: &ActionContext) -> bool {
        match self {
            WhenExpr::Always => true,
            WhenExpr::Tag(tag) => ctx.has_tag(tag),
            WhenExpr::Eq(var, val) => ctx.get_var(var).map(|v| v == val).unwrap_or(false),
            WhenExpr::Neq(var, val) => ctx.get_var(var).map(|v| v != val).unwrap_or(true),
            WhenExpr::Not(inner) => !inner.evaluate(ctx),
            WhenExpr::And(a, b) => a.evaluate(ctx) && b.evaluate(ctx),
            WhenExpr::Or(a, b) => a.evaluate(ctx) || b.evaluate(ctx),
        }
    }
}

impl fmt::Display for WhenExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhenExpr::Always => write!(f, "true"),
            WhenExpr::Tag(t) => write!(f, "{}", t),
            WhenExpr::Eq(var, val) => write!(f, "{} == {}", var, val),
            WhenExpr::Neq(var, val) => write!(f, "{} != {}", var, val),
            WhenExpr::Not(inner) => write!(f, "!{}", inner),
            WhenExpr::And(a, b) => write!(f, "{} && {}", a, b),
            WhenExpr::Or(a, b) => write!(f, "{} || {}", a, b),
        }
    }
}

// ============================================================================
// ActionContext
// ============================================================================

/// Runtime state used to evaluate when-clause expressions.
///
/// Combines simple boolean tags with key-value variables.
#[derive(Debug, Clone, Default)]
pub struct ActionContext {
    /// Simple boolean tags: "tab:performance", "popup_open", "focus:editor"
    tags: HashSet<String>,
    /// Key-value variables: "mode" -> "normal", "tab" -> "performance"
    vars: HashMap<String, String>,
}

impl ActionContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a boolean tag (e.g., "tab:performance").
    pub fn set_tag(&mut self, tag: impl Into<String>) {
        self.tags.insert(tag.into());
    }

    /// Remove a boolean tag.
    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.remove(tag);
    }

    /// Check if a tag is present.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }

    /// Set a key-value variable (e.g., "mode" = "normal").
    pub fn set_var(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    /// Remove a variable.
    pub fn remove_var(&mut self, key: &str) {
        self.vars.remove(key);
    }

    /// Get a variable's value.
    pub fn get_var(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(|s| s.as_str())
    }

    /// Convenience: set mode variable (for modal editing).
    pub fn set_mode(&mut self, mode: &str) {
        self.set_var("mode", mode);
    }

    /// Convenience: set the active tab (sets both var and tag).
    pub fn set_tab(&mut self, tab: &str) {
        // Remove old tab tags
        self.tags.retain(|t| !t.starts_with("tab:"));
        self.set_var("tab", tab);
        self.set_tag(format!("tab:{}", tab));
    }
}

// ============================================================================
// Parse Error
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    UnexpectedEnd,
    UnexpectedToken(String),
    InvalidExpression(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedEnd => write!(f, "unexpected end of expression"),
            ParseError::UnexpectedToken(msg) => write!(f, "{}", msg),
            ParseError::InvalidExpression(msg) => write!(f, "invalid expression: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

// ============================================================================
// Tokenizer
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String), // "tab:performance", "mode", "normal", "popup_open"
    And,           // "&&"
    Or,            // "||"
    Not,           // "!"
    Eq,            // "=="
    Neq,           // "!="
    LParen,        // "("
    RParen,        // ")"
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '&' if i + 1 < chars.len() && chars[i + 1] == '&' => {
                tokens.push(Token::And);
                i += 2;
            }
            '|' if i + 1 < chars.len() && chars[i + 1] == '|' => {
                tokens.push(Token::Or);
                i += 2;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Neq);
                i += 2;
            }
            '!' => {
                tokens.push(Token::Not);
                i += 1;
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Eq);
                i += 2;
            }
            c if is_ident_start(c) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                tokens.push(Token::Ident(word));
            }
            c => {
                return Err(ParseError::UnexpectedToken(format!(
                    "unexpected character: '{}'",
                    c
                )));
            }
        }
    }

    Ok(tokens)
}

fn is_ident_start(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':' || c == '.' || c == '-'
}

// ============================================================================
// Recursive descent parser: || > && > unary (!) > atom
// ============================================================================

fn parse_or(tokens: &[Token], pos: &mut usize) -> Result<WhenExpr, ParseError> {
    let mut left = parse_and(tokens, pos)?;
    while *pos < tokens.len() && tokens[*pos] == Token::Or {
        *pos += 1;
        let right = parse_and(tokens, pos)?;
        left = WhenExpr::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(tokens: &[Token], pos: &mut usize) -> Result<WhenExpr, ParseError> {
    let mut left = parse_unary(tokens, pos)?;
    while *pos < tokens.len() && tokens[*pos] == Token::And {
        *pos += 1;
        let right = parse_unary(tokens, pos)?;
        left = WhenExpr::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_unary(tokens: &[Token], pos: &mut usize) -> Result<WhenExpr, ParseError> {
    if *pos < tokens.len() && tokens[*pos] == Token::Not {
        *pos += 1;
        let inner = parse_unary(tokens, pos)?;
        return Ok(WhenExpr::Not(Box::new(inner)));
    }
    parse_atom(tokens, pos)
}

fn parse_atom(tokens: &[Token], pos: &mut usize) -> Result<WhenExpr, ParseError> {
    if *pos >= tokens.len() {
        return Err(ParseError::UnexpectedEnd);
    }

    // Parenthesized expression
    if tokens[*pos] == Token::LParen {
        *pos += 1;
        let expr = parse_or(tokens, pos)?;
        if *pos >= tokens.len() || tokens[*pos] != Token::RParen {
            return Err(ParseError::UnexpectedToken("expected ')'".into()));
        }
        *pos += 1;
        return Ok(expr);
    }

    // Identifier — could be:
    //   1. `ident == value` (equality)
    //   2. `ident != value` (inequality)
    //   3. `ident` alone (tag presence)
    if let Token::Ident(name) = &tokens[*pos] {
        let name = name.clone();
        *pos += 1;

        // Check for == or !=
        if *pos < tokens.len() {
            if tokens[*pos] == Token::Eq {
                *pos += 1;
                if *pos >= tokens.len() {
                    return Err(ParseError::UnexpectedEnd);
                }
                if let Token::Ident(val) = &tokens[*pos] {
                    let val = val.clone();
                    *pos += 1;
                    return Ok(WhenExpr::Eq(name, val));
                }
                return Err(ParseError::UnexpectedToken(
                    "expected value after ==".into(),
                ));
            }
            if tokens[*pos] == Token::Neq {
                *pos += 1;
                if *pos >= tokens.len() {
                    return Err(ParseError::UnexpectedEnd);
                }
                if let Token::Ident(val) = &tokens[*pos] {
                    let val = val.clone();
                    *pos += 1;
                    return Ok(WhenExpr::Neq(name, val));
                }
                return Err(ParseError::UnexpectedToken(
                    "expected value after !=".into(),
                ));
            }
        }

        // Just a tag
        return Ok(WhenExpr::Tag(name));
    }

    Err(ParseError::UnexpectedToken(format!(
        "unexpected token: {:?}",
        tokens[*pos]
    )))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        assert_eq!(WhenExpr::parse("").unwrap(), WhenExpr::Always);
        assert_eq!(WhenExpr::parse("  ").unwrap(), WhenExpr::Always);
    }

    #[test]
    fn parse_simple_tag() {
        assert_eq!(
            WhenExpr::parse("tab:performance").unwrap(),
            WhenExpr::Tag("tab:performance".into())
        );
    }

    #[test]
    fn parse_equality() {
        assert_eq!(
            WhenExpr::parse("mode == normal").unwrap(),
            WhenExpr::Eq("mode".into(), "normal".into())
        );
    }

    #[test]
    fn parse_inequality() {
        assert_eq!(
            WhenExpr::parse("mode != insert").unwrap(),
            WhenExpr::Neq("mode".into(), "insert".into())
        );
    }

    #[test]
    fn parse_negation() {
        assert_eq!(
            WhenExpr::parse("!popup_open").unwrap(),
            WhenExpr::Not(Box::new(WhenExpr::Tag("popup_open".into())))
        );
    }

    #[test]
    fn parse_and() {
        let expr = WhenExpr::parse("tab:performance && mode == normal").unwrap();
        assert_eq!(
            expr,
            WhenExpr::And(
                Box::new(WhenExpr::Tag("tab:performance".into())),
                Box::new(WhenExpr::Eq("mode".into(), "normal".into()))
            )
        );
    }

    #[test]
    fn parse_or() {
        let expr = WhenExpr::parse("mode == normal || mode == visual").unwrap();
        assert_eq!(
            expr,
            WhenExpr::Or(
                Box::new(WhenExpr::Eq("mode".into(), "normal".into())),
                Box::new(WhenExpr::Eq("mode".into(), "visual".into()))
            )
        );
    }

    #[test]
    fn parse_complex() {
        let expr = WhenExpr::parse("tab:performance && !popup_open && mode != insert").unwrap();
        // && is left-associative: (tab:performance && !popup_open) && mode != insert
        assert_eq!(
            expr,
            WhenExpr::And(
                Box::new(WhenExpr::And(
                    Box::new(WhenExpr::Tag("tab:performance".into())),
                    Box::new(WhenExpr::Not(Box::new(WhenExpr::Tag("popup_open".into()))))
                )),
                Box::new(WhenExpr::Neq("mode".into(), "insert".into()))
            )
        );
    }

    #[test]
    fn parse_precedence_or_lower_than_and() {
        // a || b && c should parse as a || (b && c)
        let expr = WhenExpr::parse("a || b && c").unwrap();
        assert_eq!(
            expr,
            WhenExpr::Or(
                Box::new(WhenExpr::Tag("a".into())),
                Box::new(WhenExpr::And(
                    Box::new(WhenExpr::Tag("b".into())),
                    Box::new(WhenExpr::Tag("c".into()))
                ))
            )
        );
    }

    #[test]
    fn parse_parens() {
        let expr = WhenExpr::parse("(a || b) && c").unwrap();
        assert_eq!(
            expr,
            WhenExpr::And(
                Box::new(WhenExpr::Or(
                    Box::new(WhenExpr::Tag("a".into())),
                    Box::new(WhenExpr::Tag("b".into()))
                )),
                Box::new(WhenExpr::Tag("c".into()))
            )
        );
    }

    #[test]
    fn eval_always() {
        let ctx = ActionContext::new();
        assert!(WhenExpr::Always.evaluate(&ctx));
    }

    #[test]
    fn eval_tag() {
        let mut ctx = ActionContext::new();
        assert!(!WhenExpr::Tag("tab:performance".into()).evaluate(&ctx));
        ctx.set_tag("tab:performance");
        assert!(WhenExpr::Tag("tab:performance".into()).evaluate(&ctx));
    }

    #[test]
    fn eval_eq() {
        let mut ctx = ActionContext::new();
        ctx.set_var("mode", "normal");
        assert!(WhenExpr::Eq("mode".into(), "normal".into()).evaluate(&ctx));
        assert!(!WhenExpr::Eq("mode".into(), "insert".into()).evaluate(&ctx));
    }

    #[test]
    fn eval_neq() {
        let mut ctx = ActionContext::new();
        ctx.set_var("mode", "normal");
        assert!(WhenExpr::Neq("mode".into(), "insert".into()).evaluate(&ctx));
        assert!(!WhenExpr::Neq("mode".into(), "normal".into()).evaluate(&ctx));
    }

    #[test]
    fn eval_neq_missing_var() {
        let ctx = ActionContext::new();
        // Missing var: != returns true (var is not equal to the value)
        assert!(WhenExpr::Neq("mode".into(), "insert".into()).evaluate(&ctx));
    }

    #[test]
    fn eval_not() {
        let mut ctx = ActionContext::new();
        let expr = WhenExpr::Not(Box::new(WhenExpr::Tag("popup_open".into())));
        assert!(expr.evaluate(&ctx));
        ctx.set_tag("popup_open");
        assert!(!expr.evaluate(&ctx));
    }

    #[test]
    fn eval_and() {
        let mut ctx = ActionContext::new();
        ctx.set_tag("tab:performance");
        ctx.set_var("mode", "normal");

        let expr = WhenExpr::And(
            Box::new(WhenExpr::Tag("tab:performance".into())),
            Box::new(WhenExpr::Eq("mode".into(), "normal".into())),
        );
        assert!(expr.evaluate(&ctx));

        ctx.set_var("mode", "insert");
        assert!(!expr.evaluate(&ctx));
    }

    #[test]
    fn eval_or() {
        let mut ctx = ActionContext::new();
        ctx.set_var("mode", "visual");

        let expr = WhenExpr::Or(
            Box::new(WhenExpr::Eq("mode".into(), "normal".into())),
            Box::new(WhenExpr::Eq("mode".into(), "visual".into())),
        );
        assert!(expr.evaluate(&ctx));

        ctx.set_var("mode", "insert");
        assert!(!expr.evaluate(&ctx));
    }

    #[test]
    fn action_context_set_tab() {
        let mut ctx = ActionContext::new();
        ctx.set_tab("performance");
        assert!(ctx.has_tag("tab:performance"));
        assert_eq!(ctx.get_var("tab"), Some("performance"));

        ctx.set_tab("chart");
        assert!(!ctx.has_tag("tab:performance"));
        assert!(ctx.has_tag("tab:chart"));
        assert_eq!(ctx.get_var("tab"), Some("chart"));
    }

    #[test]
    fn roundtrip_display() {
        let cases = [
            "tab:performance",
            "mode == normal",
            "mode != insert",
            "!popup_open",
        ];
        for input in cases {
            let expr = WhenExpr::parse(input).unwrap();
            // Display should produce a parseable string
            let displayed = expr.to_string();
            let reparsed = WhenExpr::parse(&displayed).unwrap();
            assert_eq!(expr, reparsed, "roundtrip failed for: {}", input);
        }
    }
}
