//! Text editing on the native (Blitz) path — typing, Enter, Tab,
//! Backspace/Delete, and typing over a selection. The web path hands
//! these to contenteditable; on native `editor-view` IS the default
//! action, so each is asserted against the doc probe.
#![cfg(feature = "native")]

mod common;
use common::*;

#[tokio::test]
async fn typing_inserts_at_caret() {
    let t = mount(Setup::text("ac").caret(1));
    t.type_text("b");
    expect_probe(&t, "doc", "abc").await;
    expect_probe(&t, "head", "2").await;
}

#[tokio::test]
async fn enter_splits_line() {
    let t = mount(Setup::text("ab").caret(1));
    press(&t, &["Enter"]);
    expect_probe(&t, "doc", "a\nb").await;
    expect_probe(&t, "head", "2").await;
}

#[tokio::test]
async fn tab_inserts_tab() {
    let t = mount(Setup::text("").caret(0));
    press(&t, &["Tab"]);
    expect_probe(&t, "doc", "\t").await;
}

#[tokio::test]
async fn typing_replaces_selection() {
    // Select "ell" (1..4) then type "x" → "hxo".
    let t = mount(Setup::text("hello").caret(1));
    t.press_key(parse_key("ArrowRight"), Modifiers::SHIFT);
    t.press_key(parse_key("ArrowRight"), Modifiers::SHIFT);
    t.press_key(parse_key("ArrowRight"), Modifiers::SHIFT);
    expect_probe(&t, "head", "4").await;
    t.type_text("x");
    expect_probe(&t, "doc", "hxo").await;
    expect_probe(&t, "head", "2").await;
}

#[tokio::test]
async fn backspace_deletes_before_caret() {
    let t = mount(Setup::text("abc").caret(2));
    press(&t, &["Backspace"]);
    expect_probe(&t, "doc", "ac").await;
    expect_probe(&t, "head", "1").await;
}

#[tokio::test]
async fn delete_removes_after_caret() {
    let t = mount(Setup::text("abc").caret(1));
    press(&t, &["Delete"]);
    expect_probe(&t, "doc", "ac").await;
    expect_probe(&t, "head", "1").await;
}

#[tokio::test]
async fn backspace_joins_lines() {
    let t = mount(Setup::text("a\nb").caret(2));
    press(&t, &["Backspace"]);
    expect_probe(&t, "doc", "ab").await;
    expect_probe(&t, "head", "1").await;
}

#[tokio::test]
async fn multi_char_typing_renders() {
    let t = mount(Setup::text(""));
    t.type_text("hello world");
    expect_probe(&t, "doc", "hello world").await;
    // And the rendered tile tree shows it too.
    t.query(".editor-root")
        .expect(inner_html(contains_substring("hello world")))
        .await
        .unwrap();
}
