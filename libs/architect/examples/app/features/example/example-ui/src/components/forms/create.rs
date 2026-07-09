//! `ExampleCreateForm` — the create form, generated from the entity.
//!
//! The fields, their validation, and the typed payload all come from
//! `#[architect(form)]` on the entity (`use_example_create_fields`):
//! `name` is required, `description` optional (`form(optional)` on the
//! field), and `submit()` returns a validated [`ExampleCreate`] — the
//! exact payload the derived mutations take. This component is just the
//! layout: two [`TextField`]s and a button.

use architect::form::TextField;
use dioxus::prelude::*;
use example::{ExampleCreate, use_example_create_fields};

#[component]
pub fn ExampleCreateForm(on_submit: EventHandler<ExampleCreate>) -> Element {
    let fields = use_example_create_fields();

    rsx! {
        form {
            class: "example-form",
            onsubmit: move |evt| {
                evt.prevent_default();
                // `submit()` validates every field (revealing errors) and
                // yields the typed payload only when the form passes.
                if let Some(input) = fields.submit() {
                    on_submit.call(input);
                    fields.reset();
                }
            },
            TextField { field: fields.name, label: "Name", placeholder: "e.g. Quarterly report" }
            TextField { field: fields.description, label: "Description", placeholder: "optional" }
            button { class: "btn primary", r#type: "submit", "Add example" }
        }
    }
}
