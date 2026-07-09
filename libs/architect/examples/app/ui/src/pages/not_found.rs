//! Catch-all 404 page.

use dioxus::prelude::*;

use crate::Route;

#[component]
pub fn NotFound(route: Vec<String>) -> Element {
    rsx! {
        div { class: "status",
            h2 { "Not found" }
            p { "No route for /{route.join(\"/\")}" }
            Link { class: "back", to: Route::Home {}, "← home" }
        }
    }
}
