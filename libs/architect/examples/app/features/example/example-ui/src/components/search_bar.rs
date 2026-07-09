//! `SearchBar` — a query input that emits on submit.
//!
//! Emits the query string via `on_search` on submit, and an empty string
//! when cleared so the caller can fall back to the full list. Backend-
//! agnostic: the caller routes the query to `ExampleServiceClient::search`.

use dioxus::prelude::*;

#[component]
pub fn SearchBar(on_search: EventHandler<String>) -> Element {
    let mut query = use_signal(String::new);
    rsx! {
        form {
            class: "search-bar",
            onsubmit: move |evt| {
                evt.prevent_default();
                on_search.call(query());
            },
            input {
                r#type: "search",
                placeholder: "search name or description…",
                value: "{query}",
                oninput: move |evt| {
                    let v = evt.value();
                    query.set(v.clone());
                    if v.is_empty() {
                        on_search.call(String::new());
                    }
                },
            }
            button { r#type: "submit", "Search" }
        }
    }
}
