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
";

fn out(name: &str) -> String {
    format!("{}/{name}", std::env::temp_dir().display())
}

#[tokio::test]
async fn shot_block_caret() {
    let t = mount(Setup::text("hello world\nsecond line").caret(6).vim().theme(THEME));
    t.render_png(out("editor_block_caret.png"));
}

#[tokio::test]
async fn shot_insert_bar_caret() {
    let t = mount(Setup::text("hello world").caret(3).vim().theme(THEME));
    press(&t, &["i"]);
    t.render_png(out("editor_bar_caret.png"));
}
