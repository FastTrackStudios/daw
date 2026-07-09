//! `ExampleList` — a plain (non-interactive) list of examples, with an
//! empty state. Composes [`ExampleRow`](super::ExampleRow). The caller
//! hands the data in, which keeps it testable without an open socket
//! (`tests/components.rs`). The wired list view renders its own *clickable*
//! rows; this is the simple display version.

use dioxus::prelude::*;
use example::Example;

use super::ExampleRow;

#[component]
pub fn ExampleList(items: Vec<Example>) -> Element {
    if items.is_empty() {
        return rsx! {
            div { class: "example-list empty",
                p { "No examples yet." }
            }
        };
    }
    rsx! {
        ul { class: "example-list",
            for ex in items {
                li { key: "{ex.id}", class: "example-row",
                    ExampleRow { example: ex }
                }
            }
        }
    }
}
