//! Detail page — view one example, with edit/inspect links, the derived
//! optimistic delete, and the service-backed duplicate.
//!
//! The error channel is **typed** end-to-end: `App(RepoError::NotFound)`
//! gets its own friendly arm, infrastructure failures get the reconnecting
//! treatment, and everything else falls through to the generic error.

use architect::AtomResult::{Defect, Error, Initial, Loading, Reloading, Success};
use architect::{ClientError, RepoError};
use dioxus::prelude::*;
use example::Example;
use example_ui::{ExampleCard, use_example, use_example_actions, use_example_mutations};

use crate::Route;
use crate::status::{DefectPanel, FailurePanel, Spinner};

#[component]
pub fn ExampleDetail(id: String) -> Element {
    rsx! {
        match use_example(id) {
            Initial | Loading => rsx! { Spinner {} },
            Success(example) | Reloading(example) => rsx! { DetailContent { example } },
            // The typed arm the string-flattened version couldn't write:
            Error { error: ClientError::App(RepoError::NotFound), .. } => rsx! {
                div { class: "status empty",
                    h2 { "Not here" }
                    p { "This example doesn't exist — it may have been deleted." }
                }
            },
            Error { error, .. } => rsx! { FailurePanel { error } },
            Defect { defect, .. } => rsx! { DefectPanel { defect } },
        }
        Link { class: "back", to: Route::Home {}, "← back" }
    }
}

#[component]
fn DetailContent(example: Example) -> Element {
    let nav = use_navigator();
    let mutations = use_example_mutations(); // derived: delete
    let actions = use_example_actions(); // hand-written: duplicate
    let uuid = example.id;
    let dup_name = format!("{} (copy)", example.name);
    let dup_desc = example.description.clone();

    rsx! {
        ExampleCard { example }
        div { class: "actions",
            Link { class: "btn", to: Route::EditExample { id: uuid.to_string() }, "Edit" }
            Link { class: "btn", to: Route::InspectExample { id: uuid.to_string() }, "Inspect" }
            button {
                class: "btn",
                disabled: actions.is_pending(),
                onclick: move |_| {
                    actions.duplicate(uuid, dup_name.clone(), dup_desc.clone());
                    nav.push(Route::Home {});
                },
                "Duplicate"
            }
            button {
                class: "btn danger",
                disabled: mutations.is_pending(),
                onclick: move |_| {
                    mutations.delete(uuid);
                    nav.push(Route::Home {});
                },
                "Delete"
            }
        }
    }
}
