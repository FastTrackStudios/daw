//! Markdown live-preview decorations on the native (Blitz) path —
//! marker hide/reveal around the caret, heading/bold classes, and the
//! caret-proximity behavior that makes the editor feel like Obsidian.
#![cfg(feature = "native")]

mod common;
use common::*;
use dioxus_test::matchers::not;

#[tokio::test]
async fn bold_markers_hidden_when_caret_away() {
    // Caret at doc start, well outside the bold span at the end.
    let t = mount(Setup::text("x **bold**").caret(0).markdown());
    t.query(".editor-root")
        .expect(inner_html(contains_substring("bold")))
        .await
        .unwrap();
    t.query(".editor-root")
        .expect(inner_html(not(contains_substring("**"))))
        .await
        .unwrap();
}

#[tokio::test]
async fn bold_markers_revealed_when_caret_inside() {
    // Caret inside the bold word — source markers must be visible.
    let t = mount(Setup::text("x **bold**").caret(6).markdown());
    t.query(".editor-root")
        .expect(inner_html(contains_substring("**")))
        .await
        .unwrap();
}

#[tokio::test]
async fn moving_caret_into_bold_reveals_markers() {
    let t = mount(Setup::text("**b** xxxx").caret(9).markdown());
    t.query(".editor-root")
        .expect(inner_html(not(contains_substring("**"))))
        .await
        .unwrap();
    // Walk the caret left into the bold span.
    press(&t, &["Home"]);
    press(&t, &["ArrowRight", "ArrowRight"]);
    t.query(".editor-root")
        .expect(inner_html(contains_substring("**")))
        .await
        .unwrap();
}

#[tokio::test]
async fn heading_marker_hidden_when_caret_on_other_line() {
    let t = mount(Setup::text("# Title\nbody").caret(10).markdown());
    t.query(".editor-root")
        .expect(inner_html(contains_substring("Title")))
        .await
        .unwrap();
    t.query(".editor-root")
        .expect(inner_html(not(contains_substring("md-heading-marker"))))
        .await
        .unwrap();
}

#[tokio::test]
async fn heading_marker_revealed_on_caret_line() {
    let t = mount(Setup::text("# Title\nbody").caret(3).markdown());
    t.query(".editor-root")
        .expect(inner_html(contains_substring("md-heading-marker")))
        .await
        .unwrap();
}

#[tokio::test]
async fn typing_next_to_bold_keeps_it_decorated() {
    let t = mount(Setup::text("**b** x").caret(7).markdown());
    t.type_text("y");
    expect_probe(&t, "doc", "**b** xy").await;
    // Still decorated (markers hidden) — the decoration source re-runs
    // on every transaction.
    t.query(".editor-root")
        .expect(inner_html(not(contains_substring("**"))))
        .await
        .unwrap();
}
