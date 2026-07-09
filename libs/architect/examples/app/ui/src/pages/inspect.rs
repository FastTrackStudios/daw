//! Inspect page — the single-resource `use_async` demo. `use_example_live`
//! fetches one example straight from the server (no store, no optimism)
//! and is refreshable; the page `match`es its phase and renders the dumb
//! `ExampleCard`. Navigation is normal Dioxus `Link`s.

use architect::AtomResult::{Defect, Error, Initial, Loading, Reloading, Success};
use dioxus::prelude::*;
use example_ui::{ExampleCard, use_example_live};

use crate::Route;
use crate::status::{DefectPanel, FailurePanel, Spinner};

#[component]
pub fn InspectExample(id: String) -> Element {
    let detail = Route::ExampleDetail { id: id.clone() };
    let live = use_example_live(id);

    rsx! {
        div { class: "toolbar",
            button { class: "btn", onclick: move |_| live.refresh(), "↻ Refresh" }
            Link { class: "btn", to: detail, "← back" }
        }
        match live.state() {
            Initial | Loading => rsx! { Spinner {} },
            Reloading(example) => rsx! {
                p { class: "status", "Refreshing…" }
                ExampleCard { example }
            },
            Success(example) => rsx! { ExampleCard { example } },
            Error { error, .. } => rsx! { FailurePanel { error } },
            Defect { defect, .. } => rsx! { DefectPanel { defect } },
        }
    }
}
