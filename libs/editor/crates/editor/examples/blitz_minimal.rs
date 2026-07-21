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
    dioxus_native::launch(App);
}

#[component]
fn App() -> Element {
    let state = use_signal(|| EditorState::new(SEED));
    let keymap = use_hook(standard_markdown_keymap);
    let vim = use_signal(editor::editor_vim::VimState::new);

    rsx! {
        style { dangerous_inner_html: include_str!("../assets/editor.css") }
        div {
            style: "max-width: 46rem; margin: 2rem auto; padding: 0 1rem; \
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
