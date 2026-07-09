//! Home page — the create composer, search, and the list.
//!
//! State comes from the feature's search-aware `use_examples` hook as one
//! `AtomResult`; the page `match`es the phase (universal arms via
//! `Spinner`/`StatusError`) and renders rows as normal Dioxus `Link`s.
//! Creating goes through the **derived** `ExampleMutations`.

use architect::AtomResult::{Defect, Error, Initial, Loading, Reloading, Success};
use architect::Id;
use dioxus::prelude::*;
use example::{Example, ExampleCreate};
use example_ui::{ExampleRow, SearchBar, use_example_mutations, use_examples};
use uuid::Uuid;

use crate::Route;
use crate::status::{DefectPanel, FailurePanel, Spinner};

#[component]
pub fn Home() -> Element {
    let mut query = use_signal(String::new);
    let mutations = use_example_mutations();
    let examples = use_examples(query);
    let count = examples.value().map(Vec::len).unwrap_or(0);

    rsx! {
        // Inline composer: the derive-generated form emits the typed
        // `ExampleCreate`; the derived mutation drops the optimistic row
        // into the list below. Typed end-to-end, no field plumbing.
        section { class: "composer",
            h2 { class: "composer__title", "Examples" }
            example_ui::ExampleCreateForm {
                on_submit: move |input: ExampleCreate| mutations.create(input),
            }
        }

        div { class: "toolbar",
            SearchBar { on_search: move |q: String| query.set(q) }
            if count > 0 {
                span { class: "count", "{count}" }
            }
        }
        if let Some(e) = mutations.create_error() {
            FailurePanel { error: e }
        }

        // The universal phase match. Stale-while-revalidate falls out of
        // the `Reloading` / failure-with-`last` arms.
        match examples {
            Initial | Loading => rsx! { Spinner {} },
            Reloading(rows) => rsx! {
                p { class: "status", "Refreshing…" }
                ExampleRows { rows }
            },
            Success(rows) => rsx! { ExampleRows { rows } },
            Error { error, last } => rsx! {
                FailurePanel { error }
                if let Some(rows) = last {
                    ExampleRows { rows }
                }
            },
            Defect { defect, .. } => rsx! { DefectPanel { defect } },
        }
    }
}

/// The list body — clickable real rows (a `Link` to the detail route) and
/// pending optimistic rows.
#[component]
fn ExampleRows(rows: Vec<(Id<Uuid>, Example)>) -> Element {
    if rows.is_empty() {
        return rsx! { p { class: "status empty", "No examples yet — create one." } };
    }
    rsx! {
        ul { class: "example-list",
            for (id, example) in rows {
                ExampleListRow { key: "{id}", id, example }
            }
        }
    }
}

#[component]
fn ExampleListRow(id: Id<Uuid>, example: Example) -> Element {
    match id {
        Id::Real(uuid) => rsx! {
            li { class: "example-link-row",
                Link { class: "row", to: Route::ExampleDetail { id: uuid.to_string() },
                    div { class: "row__content", ExampleRow { example } }
                    span { class: "chevron", "›" }
                }
            }
        },
        Id::Temp(_) => rsx! {
            li { class: "example-link-row is-pending",
                div { class: "row",
                    div { class: "row__content", ExampleRow { example } }
                    span { class: "row__status",
                        span { class: "spinner", aria_label: "saving" }
                        span { class: "row__status-label", "Saving…" }
                    }
                }
            }
        },
    }
}
