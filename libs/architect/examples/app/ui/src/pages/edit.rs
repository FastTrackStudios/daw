//! Edit page — `match`es the derived `use_example` phase; the success arm
//! renders the dumb edit form and, on submit, runs the derived optimistic
//! `update` then navigates back with normal Dioxus `nav`.

use architect::AtomResult::{Defect, Error, Initial, Loading, Reloading, Success};
use dioxus::prelude::*;
use example::{Example, ExampleUpdate};
use example_ui::{ExampleEditForm, use_example, use_example_mutations};

use crate::Route;
use crate::status::{DefectPanel, FailurePanel, Spinner};

#[component]
pub fn EditExample(id: String) -> Element {
    rsx! {
        h2 { "Edit example" }
        match use_example(id) {
            Initial | Loading => rsx! { Spinner {} },
            Success(example) | Reloading(example) => rsx! { EditContent { example } },
            Error { error, .. } => rsx! { FailurePanel { error } },
            Defect { defect, .. } => rsx! { DefectPanel { defect } },
        }
    }
}

#[component]
fn EditContent(example: Example) -> Element {
    let nav = use_navigator();
    let mutations = use_example_mutations();
    let uuid = example.id;
    let detail = Route::ExampleDetail {
        id: uuid.to_string(),
    };

    rsx! {
        if let Some(e) = mutations.write_error() {
            FailurePanel { error: e }
        }
        // The derive-generated form is seeded from the row and emits the
        // typed `ExampleUpdate` — straight into the derived mutation.
        ExampleEditForm {
            example,
            on_submit: {
                let detail = detail.clone();
                move |update: ExampleUpdate| {
                    mutations.update(uuid, update);
                    nav.push(detail.clone());
                }
            },
        }
        Link { class: "back", to: detail, "← cancel" }
    }
}
