//! Minimal Blitz-native editor — the whole embedding story in one file.
//!
//! Run from the repo root:
//!
//! ```sh
//! cargo run -p editor --example blitz_minimal --no-default-features --features native
//! ```
//!
//! This opens a real `dioxus-native` (Blitz / Vello / wgpu) window — no
//! webview, no JS — with a working markdown editor: live-preview
//! decorations (bold/heading markers hide and reveal around the caret,
//! `((uuid))` block-ref chips, hidden `id::` lines), vim modal editing
//! (hjkl, operators + text objects like `ciw`/`ci"`, visual mode, undo),
//! list-continuing Enter, bracket auto-pairing, and word-wise
//! Ctrl-motions — all driven by the same shared `editor_state` core the
//! web build uses, and all covered by the headless `dioxus-test` suites
//! in `editor-view/tests/native_*.rs`.
//!
//! Styling is inlined (`dangerous_inner_html`) because Blitz does not
//! load external stylesheets — same rule as the signal-domain UIs.

use dioxus::prelude::*;
use editor::{EditorState, combined_decorations, editor_view, standard_markdown_keymap};
use editor_view::DecorationSource;

const SEED: &str = "\
# Editor on Blitz

This window is **pure Rust** — Blitz DOM, Vello, wgpu. No webview.

- vim is on: try `hjkl`, `dd`, `ciw`, `vi(` …
- Enter continues lists like this one
- brackets auto-pair: (try typing one)
- **bold** and `# heading` markers reveal near the caret

1. ordered lists renumber on Enter
2. `Tab` indents a list item, `Shift-Tab` outdents
";

fn main() {
    // `RUST_LOG=editor_view=debug` shows the keydown dispatch trace.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        // stderr: unbuffered, so traces reach a piped log immediately.
        .with_writer(std::io::stderr)
        .init();
    dioxus_native::launch(App);
}

/// Readable light theme. These are the EXACT token names the editor CSS
/// reads (`--background`, `--foreground`, `--primary`, `--muted`,
/// `--muted-foreground`) — not the fts-ui aliases. A saturated
/// `--primary` matters: the vim block caret paints the glyph under it in
/// `--background` (reverse video), so a light accent makes the character
/// vanish (white-on-light). Deep blue keeps the glyph legible.
const THEME: &str = "
:root {
    --background: #ffffff;
    --muted: #f2f4f7;
    --foreground: #1a1c20;
    --muted-foreground: #6b7280;
    --primary: #1d4ed8;
}
body { background: #ffffff; color: #1a1c20; }
";

#[component]
fn App() -> Element {
    let state = use_signal(|| EditorState::new(SEED));
    let keymap = use_hook(standard_markdown_keymap);
    let vim = use_signal(editor::editor_vim::VimState::new);

    rsx! {
        style { dangerous_inner_html: include_str!("../assets/editor.css") }
        // The editor's CSS reads design-system tokens (--background,
        // --text, --accent, …) that a host app normally injects. This
        // standalone example has no design system, so define a readable
        // light theme here — otherwise text is dark-on-dark and invisible.
        style { dangerous_inner_html: THEME }
        div {
            style: "max-width: 46rem; margin: 2rem auto; padding: 0 1rem; \
                    color: #1a1c20; background: #ffffff; \
                    font-family: system-ui, sans-serif;",
            editor_view::Editor {
                state,
                vim: Some(vim),
                keymap: keymap.clone(),
                decorations: DecorationSource::ptr(combined_decorations),
            }
        }
    }
}
