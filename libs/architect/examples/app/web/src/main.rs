//! Dioxus web (wasm) entry point. All UI + the vox client lifecycle live
//! in `app_ui::App`, shared verbatim with the desktop target — the only
//! difference is the `Transport`: web talks to `app-server` over a vox
//! WebSocket (the browser can't run the in-process transport).

use app_ui::{App, Transport};
use dioxus::prelude::*;

fn main() {
    // Surface Rust panics as readable console.error messages (message +
    // location) instead of a bare `RuntimeError: unreachable` trap.
    console_error_panic_hook::set_once();
    dioxus::launch(Root);
}

#[component]
fn Root() -> Element {
    use_context_provider(|| Transport::Remote(app_ui::DEFAULT_SERVER_URL.to_string()));
    rsx! { App {} }
}
