//! Cursor movement + selection on the native (Blitz) path — arrows,
//! Home/End, shift-extension, and click-to-position, asserted against
//! the Rust-side selection through the harness probes.
#![cfg(feature = "native")]

mod common;
use common::*;

#[tokio::test]
async fn arrows_move_caret() {
    let t = mount(Setup::text("abc\ndef"));
    press(&t, &["ArrowRight", "ArrowRight"]);
    expect_probe(&t, "head", "2").await;
    press(&t, &["ArrowLeft"]);
    expect_probe(&t, "head", "1").await;
    press(&t, &["ArrowDown"]);
    expect_probe(&t, "head", "5").await; // col 1 on line 2
    press(&t, &["ArrowUp"]);
    expect_probe(&t, "head", "1").await;
}

#[tokio::test]
async fn arrow_left_at_start_stays() {
    let t = mount(Setup::text("abc"));
    press(&t, &["ArrowLeft", "ArrowLeft"]);
    expect_probe(&t, "head", "0").await;
}

#[tokio::test]
async fn arrow_right_at_end_stays() {
    let t = mount(Setup::text("ab").caret(2));
    press(&t, &["ArrowRight"]);
    expect_probe(&t, "head", "2").await;
}

#[tokio::test]
async fn home_end_jump_line_bounds() {
    let t = mount(Setup::text("hello\nworld").caret(8));
    press(&t, &["Home"]);
    expect_probe(&t, "head", "6").await;
    press(&t, &["End"]);
    expect_probe(&t, "head", "11").await;
}

#[tokio::test]
async fn shift_arrow_extends_selection() {
    let t = mount(Setup::text("abcdef").caret(1));
    t.press_key(parse_key("ArrowRight"), Modifiers::SHIFT);
    t.press_key(parse_key("ArrowRight"), Modifiers::SHIFT);
    expect_probe(&t, "anchor", "1").await;
    expect_probe(&t, "head", "3").await;
    // Unshifted arrow collapses to a caret again.
    press(&t, &["ArrowRight"]);
    expect_probe(&t, "anchor", "4").await;
    expect_probe(&t, "head", "4").await;
}

#[tokio::test]
async fn vertical_move_clamps_column() {
    // From col 4 on "abcde" down to "xy" (len 2): clamp to line end.
    let t = mount(Setup::text("abcde\nxy").caret(4));
    press(&t, &["ArrowDown"]);
    expect_probe(&t, "head", "8").await; // end of "xy"
}

#[tokio::test]
async fn click_positions_caret_on_line() {
    let t = mount(Setup::text("hello\nworld").caret(0));
    // Click the second rendered line — caret must land inside [6, 11].
    let line = t.query_all(".cm-line").immediately();
    assert!(line.len() >= 2, "expected 2 rendered lines");
    let c = line[1].center();
    t.click_at(c.page().x as f32, c.page().y as f32);
    t.pump().await.ok();
    let head: usize = t
        .query(dioxus_test::by_testid("head"))
        .immediately()
        .unwrap()
        .inner_html()
        .trim()
        .parse()
        .expect("head probe should be a number");
    assert!(
        (6..=11).contains(&head),
        "click on line 2 should put caret in 6..=11, got {head}"
    );
}
