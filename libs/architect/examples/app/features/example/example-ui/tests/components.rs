//! Render the feature's presentational components to static markup and
//! assert on what they emit — no socket, no router, no browser. Catches
//! empty-state handling, data binding, and form structure regressions.

use chrono::Utc;
use dioxus::prelude::*;
use example::{Example, ExampleCreate, ExampleUpdate};
use example_ui::{ExampleCreateForm, ExampleEditForm, ExampleList, ExampleRow, SearchBar};
use uuid::Uuid;

/// Render a component (no props) to HTML.
fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

fn sample(name: &str, description: &str) -> Example {
    Example {
        id: Uuid::new_v4(),
        name: name.into(),
        description: description.into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn list_empty_shows_placeholder() {
    fn app() -> Element {
        rsx! { ExampleList { items: Vec::<Example>::new() } }
    }
    let html = render(app);
    assert!(html.contains("No examples yet"), "html: {html}");
}

#[test]
fn list_renders_each_row() {
    fn app() -> Element {
        rsx! {
            ExampleList {
                items: vec![sample("alpha", "first"), sample("beta", "second")],
            }
        }
    }
    let html = render(app);
    assert!(html.contains("alpha"));
    assert!(html.contains("beta"));
    assert!(html.contains("first"));
    assert!(html.contains("second"));
}

#[test]
fn row_renders_name_and_description() {
    fn app() -> Element {
        rsx! { ExampleRow { example: sample("solo", "details") } }
    }
    let html = render(app);
    assert!(html.contains("solo"));
    assert!(html.contains("details"));
}

#[test]
fn create_form_has_inputs_and_submit() {
    fn app() -> Element {
        rsx! { ExampleCreateForm { on_submit: move |_: ExampleCreate| {} } }
    }
    let html = render(app);
    assert!(html.matches("<input").count() >= 2, "two inputs: {html}");
    assert!(html.contains("<button"));
    // Labels come from the derive (Title Case of the payload fields).
    assert!(html.contains("Name"), "labeled name field: {html}");
    assert!(
        html.contains("Description"),
        "labeled description field: {html}"
    );
}

#[test]
fn edit_form_prefills_from_the_row() {
    fn app() -> Element {
        rsx! {
            ExampleEditForm {
                example: sample("preset-name", "preset-desc"),
                on_submit: move |_: ExampleUpdate| {},
            }
        }
    }
    let html = render(app);
    assert!(html.contains("preset-name"), "prefilled name: {html}");
    assert!(
        html.contains("preset-desc"),
        "prefilled description: {html}"
    );
}

#[test]
fn search_bar_renders_search_input() {
    fn app() -> Element {
        rsx! { SearchBar { on_search: move |_: String| {} } }
    }
    let html = render(app);
    assert!(html.contains("search"), "html: {html}");
    assert!(html.contains("<button"));
}
