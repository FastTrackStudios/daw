//! `ExampleCard` — the detail display of one example. Dumb: data in, no
//! navigation, no actions. The shell wraps it with `Link`s + action
//! buttons.

use dioxus::prelude::*;
use example::Example;

#[component]
pub fn ExampleCard(example: Example) -> Element {
    rsx! {
        article { class: "detail",
            h2 { "{example.name}" }
            p { class: "muted", "{example.description}" }
            dl { class: "meta",
                dt { "id" } dd { code { "{example.id}" } }
                dt { "created" } dd { "{example.created_at}" }
                dt { "updated" } dd { "{example.updated_at}" }
            }
        }
    }
}
