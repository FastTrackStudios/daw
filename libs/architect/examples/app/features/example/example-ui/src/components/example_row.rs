//! `ExampleRow` — the smallest unit: the content of one example as a row.
//!
//! Renders just the name + description, with no wrapping `<li>` and no
//! navigation, so it composes inside any list item — the dumb
//! [`ExampleList`](crate::components::ExampleList) wraps it in a plain row,
//! and the wired list view wraps it in a clickable one.

use dioxus::prelude::*;
use example::Example;

#[component]
pub fn ExampleRow(example: Example) -> Element {
    rsx! {
        span { class: "example-row__name", "{example.name}" }
        span { class: "example-row__description", "{example.description}" }
    }
}
