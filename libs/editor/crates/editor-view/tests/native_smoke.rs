//! Headless smoke tests for the native (Blitz) render path.
//!
//! These run through `dioxus-test` — the component renders into a real
//! blitz-dom document (same DOM + layout stack as `dioxus-native`), no
//! browser involved. Native-feature builds only:
//!
//! ```sh
//! cargo test -p editor-view --no-default-features --features native
//! ```
#![cfg(feature = "native")]

use dioxus::prelude::*;
use dioxus_test::{
    matchers::{contains_substring, inner_html},
    render,
};
use editor_state::EditorState;
use editor_view::Editor;

#[component]
fn HelloEditor() -> Element {
    let state = use_signal(|| EditorState::new("hello world"));
    rsx! {
        Editor { state }
    }
}

#[test]
fn native_render_shows_doc_text() {
    let tester = render(HelloEditor).build();
    tester
        .query(".editor-root")
        .expect(inner_html(contains_substring("hello world")))
        .immediately()
        .unwrap();
}

#[component]
fn EmptyEditor() -> Element {
    let state = use_signal(|| EditorState::new(""));
    rsx! {
        Editor { state }
    }
}

/// Typing goes through the real native input path: Blitz routes the
/// keydown to the focused editor root (autofocus), the editor's keydown
/// fallthrough (`native::handle_text_input`) turns it into a transaction,
/// and the re-rendered tile tree shows the inserted text.
#[tokio::test]
async fn typing_inserts_text() {
    let tester = render(EmptyEditor).build();
    // Blitz only routes key events to the focused node; autofocus handling
    // lives in the windowing shell, so focus explicitly here.
    tester.query(".editor-root").immediately().unwrap().focus();
    tester.type_text("hi!");
    tester
        .query(".editor-root")
        .expect(inner_html(contains_substring("hi!")))
        .await
        .unwrap();
}
