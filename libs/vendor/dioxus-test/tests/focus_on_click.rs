//! Does clicking focus the nearest focusable ancestor? And does a
//! `stop_propagation()` mousedown handler on a child break that?
use dioxus::prelude::*;
use dioxus_test::render;

#[component]
fn Plain() -> Element {
    rsx! {
        div { class: "outer", tabindex: "0",
            span { class: "inner", "hello" }
        }
    }
}

#[component]
fn StopProp() -> Element {
    rsx! {
        div { class: "outer", tabindex: "0",
            span { class: "inner",
                onmousedown: move |e: Event<MouseData>| { e.stop_propagation(); },
                "hello"
            }
        }
    }
}

#[test]
fn click_focuses_plain() {
    let t = render(Plain).build();
    let c = t.query(".inner").immediately().unwrap().center();
    let before = t.blitz_focus();
    t.click_at(c.page().x as f32, c.page().y as f32);
    let after = t.blitz_focus();
    println!("plain: before={before:?} after={after:?}");
    assert_ne!(before, after, "click should move focus (plain)");
}

#[test]
fn click_focuses_with_stop_propagation() {
    let t = render(StopProp).build();
    let c = t.query(".inner").immediately().unwrap().center();
    let before = t.blitz_focus();
    t.click_at(c.page().x as f32, c.page().y as f32);
    let after = t.blitz_focus();
    println!("stopprop: before={before:?} after={after:?}");
    assert_ne!(before, after, "click should move focus (stop_propagation)");
}
