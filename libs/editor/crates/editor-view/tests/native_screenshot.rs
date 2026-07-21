//! Visual-inspection harness: rasterize editor states to PNGs via
//! `DocumentTester::render_png`. Not asserted — a debugging aid for
//! caret/decoration styling that HTML assertions can't catch. Outputs
//! land in the target tmp dir; open them or Read them during dev.
#![cfg(feature = "native")]

mod common;
use common::*;

/// Light theme supplying the exact tokens editor.css reads, so shots
/// render with real colors instead of the dark fallbacks.
const THEME: &str = "
:root { --background:#ffffff; --muted:#f2f4f7; --foreground:#1a1c20;
        --muted-foreground:#6b7280; --primary:#1d4ed8; }
.cm-line { font-size: 44px; line-height: 1.6; }
";

fn out(name: &str) -> String {
    format!("{}/{name}", std::env::temp_dir().display())
}

#[tokio::test]
async fn shot_descender() {
    // Caret on the 'g' (descender) of "going gypsy".
    let t = mount(Setup::text("going gypsy").caret(0).vim().theme(THEME));
    t.render_png(out("editor_descender.png"));
}

#[tokio::test]
async fn shot_empty_line_and_space_caret() {
    // Line 1, an EMPTY line, then more text; caret on the space in
    // "foo bar" (offset in the space between words).
    let t = mount(
        Setup::text("first line

foo bar baz
last")
            .caret(15) // space between "foo" and "bar"
            .vim()
            .theme(THEME),
    );
    t.render_png(out("editor_issues.png"));
}

#[tokio::test]
async fn shot_caret_on_empty_line() {
    // Caret sits on the empty line (offset 11 = the empty line start).
    let t = mount(Setup::text("first line

after").caret(11).vim().theme(THEME));
    t.render_png(out("editor_empty_caret.png"));
}
