//! `ExampleEditForm` — the edit form, generated from the entity and
//! seeded from the current row.
//!
//! `use_example_update_fields(&example)` pre-fills every field from the
//! row; `submit()` returns a validated [`ExampleUpdate`] ready for the
//! derived `mutations.update`. Mounted fresh per detail route, so the
//! one-time seeding is the intended behaviour.

use architect::form::TextField;
use dioxus::prelude::*;
use example::{Example, ExampleUpdate, use_example_update_fields};

#[component]
pub fn ExampleEditForm(example: Example, on_submit: EventHandler<ExampleUpdate>) -> Element {
    let fields = use_example_update_fields(&example);

    rsx! {
        form {
            class: "example-form",
            onsubmit: move |evt| {
                evt.prevent_default();
                if let Some(update) = fields.submit() {
                    on_submit.call(update);
                }
            },
            TextField { field: fields.name, label: "Name" }
            TextField { field: fields.description, label: "Description" }
            button { class: "btn primary", r#type: "submit", "Save changes" }
        }
    }
}
