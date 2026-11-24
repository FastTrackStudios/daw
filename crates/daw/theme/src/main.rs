//! `daw-theme`: check a theme pack against the spec, list its themes, or
//! install one from a link.
//!
//! ```sh
//! daw-theme check <pack folder or link> [theme]   # exit 1 when a theme fails
//! daw-theme list  <pack folder or link>
//! daw-theme spec                                  # every token, part and field
//! ```
//!
//! A link is fetched into `$XDG_CACHE_HOME/daw-theme` (or `~/.cache`).

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use daw_theme::{Source, check, install, spec};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = std::io::stdout().lock();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check", link, rest @ ..] => run_check(&mut out, link, rest.first().copied()),
        ["list", link] => run_list(&mut out, link),
        ["spec"] => run_spec(&mut out),
        _ => Err("usage: daw-theme check <pack> [theme] | list <pack> | spec".to_owned()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(why) => {
            let _ = writeln!(std::io::stderr(), "daw-theme: {why}");
            ExitCode::from(2)
        }
    }
}

fn cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("daw-theme")
}

fn open(link: &str) -> Result<daw_theme::Installed, String> {
    let source = Source::parse(link).map_err(|e| e.to_string())?;
    install(&source, &cache()).map_err(|e| e.to_string())
}

fn run_list(out: &mut impl std::io::Write, link: &str) -> Result<bool, String> {
    let got = open(link)?;
    let w = |e: std::io::Error| e.to_string();
    writeln!(out, "{} ({})", got.pack.title, got.pack.name).map_err(w)?;
    if let Some(c) = &got.commit {
        writeln!(out, "  at {c}").map_err(w)?;
    }
    for t in &got.pack.themes {
        writeln!(
            out,
            "  {:<20} {:<20} mode {}",
            t.name,
            t.title,
            t.mode.as_deref().unwrap_or("(first)")
        )
        .map_err(w)?;
    }
    Ok(true)
}

fn run_check(
    out: &mut impl std::io::Write,
    link: &str,
    only: Option<&str>,
) -> Result<bool, String> {
    let got = open(link)?;
    let w = |e: std::io::Error| e.to_string();
    let mut clean = true;
    for t in got
        .pack
        .themes
        .iter()
        .filter(|t| only.is_none_or(|o| o == t.name))
    {
        let report = check(t);
        clean &= report.is_clean();
        let verdict = if report.is_clean() { "passes" } else { "FAILS" };
        writeln!(
            out,
            "{}/{}: {verdict} spec {}",
            got.pack.name,
            t.name,
            spec::VERSION
        )
        .map_err(w)?;
        for e in &report.errors {
            writeln!(out, "  error: {e}").map_err(w)?;
        }
        for n in &report.unknown_tokens {
            writeln!(out, "  unknown token: {n}").map_err(w)?;
        }
        for n in &report.unknown_parts {
            writeln!(out, "  unknown page: {n}").map_err(w)?;
        }
        writeln!(out, "  pages: {}", report.pages.join(", ")).map_err(w)?;
        writeln!(
            out,
            "  missing tokens ({}): each falls back",
            report.missing_tokens.len()
        )
        .map_err(w)?;
        for (n, to) in &report.missing_tokens {
            writeln!(out, "    {n:<24} -> {to}").map_err(w)?;
        }
        if !report.missing_components.is_empty() {
            writeln!(
                out,
                "  shared components not provided: {}",
                report.missing_components.join(", ")
            )
            .map_err(w)?;
        }
    }
    Ok(clean)
}

fn run_spec(out: &mut impl std::io::Write) -> Result<bool, String> {
    let w = |e: std::io::Error| e.to_string();
    writeln!(out, "spec {}", spec::VERSION).map_err(w)?;
    writeln!(out, "tokens:").map_err(w)?;
    for t in spec::TOKENS {
        let falls = match t.fallback {
            spec::Fallback::Token(n) => format!("-> ${n}"),
            spec::Fallback::Value(v) => format!("= {v}"),
            spec::Fallback::Host => "the view's own".to_owned(),
        };
        writeln!(
            out,
            "  {:<24} {:<7} {:<22} {}",
            t.name,
            format!("{:?}", t.kind),
            falls,
            t.doc
        )
        .map_err(w)?;
    }
    writeln!(out, "parts:").map_err(w)?;
    for p in spec::PARTS {
        let family = if p.family { ".*" } else { "" };
        writeln!(
            out,
            "  {:<24} {:<10} {}",
            format!("{}{family}", p.name),
            format!("{:?}", p.kind),
            p.doc
        )
        .map_err(w)?;
    }
    writeln!(out, "fields:").map_err(w)?;
    for f in spec::FIELDS {
        writeln!(out, "  {:<24} {:?}", f.name, f.class).map_err(w)?;
    }
    Ok(true)
}
