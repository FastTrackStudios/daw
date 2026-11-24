//! Holding a pack's theme to the spec.
//!
//! A theme **passes** when it compiles against the spec's fields (so every
//! binding names a field a view provides, and every `$token` it reads is
//! defined) and names no token or page the spec lacks. What it leaves out
//! is not a failure — each missing token falls back as the spec says, each
//! missing page is drawn by the view itself — and the report lists both.

use frame::document::proto::NodeKind;
use frame::theme::runtime::Theme;

use crate::pack::ThemeDef;
use crate::spec::{self, PartKind};
use crate::tokens::{Resolved, Tokens};
use crate::view::Uses;

/// What [`check`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The theme did not compile: every problem.
    pub errors: Vec<String>,
    /// Tokens the theme sets that the spec does not have (a `local.` one
    /// is the pack's own and never listed).
    pub unknown_tokens: Vec<String>,
    /// Pages the theme draws that no view asks for.
    pub unknown_parts: Vec<String>,
    /// Spec tokens the theme leaves out, and what each falls back to.
    pub missing_tokens: Vec<(String, String)>,
    /// Shared components the theme does not provide.
    pub missing_components: Vec<String>,
    /// The pages it draws.
    pub pages: Vec<String>,
}

impl Report {
    /// Whether the theme passes.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.unknown_tokens.is_empty() && self.unknown_parts.is_empty()
    }

    /// For each view, the pages it asks for that this theme draws, and
    /// those it draws itself.
    #[must_use]
    pub fn coverage(&self, views: &[Uses]) -> Vec<Coverage> {
        views
            .iter()
            .map(|v| {
                let (themed, native): (Vec<_>, Vec<_>) = v
                    .parts
                    .iter()
                    .filter(|p| spec::part(p).is_some_and(|p| p.kind == PartKind::Page))
                    .partition(|p| {
                        self.pages
                            .iter()
                            .any(|d| d == *p || d.starts_with(&format!("{p}.")))
                    });
                Coverage {
                    view: v.view,
                    themed: themed.into_iter().copied().collect(),
                    native: native.into_iter().copied().collect(),
                }
            })
            .collect()
    }
}

/// One view's pages, drawn by a theme or by the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub view: &'static str,
    pub themed: Vec<&'static str>,
    pub native: Vec<&'static str>,
}

/// Compile `theme` and hold it to the spec.
#[must_use]
pub fn check(theme: &ThemeDef) -> Report {
    match theme.compile() {
        Ok(t) => check_compiled(&t),
        Err(e) => Report {
            errors: vec![e.to_string()],
            ..Report::default()
        },
    }
}

/// Hold a compiled theme to the spec.
#[must_use]
pub fn check_compiled(theme: &Theme) -> Report {
    let unknown_tokens = theme
        .tokens()
        .map(|(n, _)| n)
        .filter(|n| !n.starts_with(spec::LOCAL) && spec::token(n).is_none())
        .map(str::to_owned)
        .collect();
    let mut pages: Vec<String> = theme.roles().map(str::to_owned).collect();
    pages.sort();
    let unknown_parts = pages
        .iter()
        .filter(|p| spec::part(p).is_none_or(|part| part.kind != PartKind::Page))
        .cloned()
        .collect();
    let tokens = Tokens::new(Some(theme));
    let missing_tokens = spec::TOKENS
        .iter()
        .filter(|t| theme.token(t.name).is_none())
        .map(|t| {
            let falls = match tokens.resolve(t.name) {
                Resolved::Pack(_) => format!("the pack's {}", alias_end(theme, t.name)),
                Resolved::Default(v) => format!("{v:?}"),
                Resolved::Host => "the view's own".to_owned(),
            };
            (t.name.to_owned(), falls)
        })
        .collect();
    let source = theme.source();
    let components: Vec<String> = source
        .pages()
        .iter()
        .flat_map(|&p| source.walk(p))
        .filter_map(|id| source.node(id))
        .filter(|n| n.kind == NodeKind::Component)
        .map(|n| n.name.clone())
        .collect();
    let missing_components = spec::PARTS
        .iter()
        .filter(|p| p.kind == PartKind::Component && !components.iter().any(|c| c == p.name))
        .map(|p| p.name.to_owned())
        .collect();
    Report {
        errors: Vec::new(),
        unknown_tokens,
        unknown_parts,
        missing_tokens,
        missing_components,
        pages,
    }
}

/// The token `name`'s fallbacks end at in `theme`: the first it sets.
fn alias_end(theme: &Theme, name: &str) -> String {
    let mut at = name;
    for _ in 0..=spec::TOKENS.len() {
        if theme.token(at).is_some() {
            return format!("`{at}`");
        }
        match spec::token(at).map(|t| t.fallback) {
            Some(spec::Fallback::Token(next)) => at = next,
            _ => break,
        }
    }
    format!("`{at}`")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Pack;
    use crate::pack::tests::Scratch;

    const DOC: &str = r##"<Document>
  <Variables modes="dark">
    <Var name="accent" value="#38bdf8" />
    <Var name="local.groove" value="#050506" />
    <Var name="mutee" value="#ff0000" />
  </Variables>
  <Page name="controls">
    <Component name="mute" w=20 h=20><Rectangle w=20 h=20 bind:fill="$accent" /></Component>
  </Page>
  <Page name="mcp.strip"><Frame name="Strip" w=86 h=400><Text name="Name" w=86 h=16 bind:text="track.name">T</Text></Frame></Page>
  <Page name="mcp.strips"><Frame name="Strip" w=86 h=400 /></Page>
</Document>"##;

    #[test]
    fn unknown_tokens_and_pages_fail_and_missing_ones_fall_back() {
        let s = Scratch::new("check");
        s.write("themes/t.frame", DOC);
        let pack = Pack::open(&s.0).expect("opens");
        let report = check(pack.theme(None).expect("theme"));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.unknown_tokens, ["mutee"]);
        assert_eq!(report.unknown_parts, ["mcp.strips"]);
        assert!(!report.is_clean());
        let route = report
            .missing_tokens
            .iter()
            .find(|(n, _)| n == "route.out")
            .expect("route.out");
        assert_eq!(route.1, "the pack's `accent`");
        assert!(!report.missing_components.contains(&"mute".to_owned()));
        assert!(report.missing_components.contains(&"solo".to_owned()));
    }

    #[test]
    fn a_field_no_view_provides_fails_the_compile() {
        let s = Scratch::new("field");
        s.write(
            "themes/t.frame",
            r#"<Document><Page name="tcp.row"><Frame name="Row" w=10 h=10><Text name="N" w=10 h=10 bind:text="track.nam">x</Text></Frame></Page></Document>"#,
        );
        let pack = Pack::open(&s.0).expect("opens");
        let report = check(pack.theme(None).expect("theme"));
        assert_ne!(report.errors, Vec::<String>::new());
        assert!(!report.is_clean());
    }

    #[test]
    fn coverage_splits_a_views_pages() {
        let report = Report {
            pages: vec!["mcp.strip".into(), "fx.macro.tone".into()],
            ..Report::default()
        };
        let mixer = Uses {
            view: "mixer",
            tokens: &[],
            parts: &["mcp.strip", "mcp.step", "fx.macro", "mute"],
            fields: &[],
        };
        let c = report.coverage(&[mixer]);
        assert_eq!(
            c.first().map(|c| c.themed.clone()),
            Some(vec!["mcp.strip", "fx.macro"])
        );
        assert_eq!(c.first().map(|c| c.native.clone()), Some(vec!["mcp.step"]));
    }
}
