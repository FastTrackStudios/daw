//! Dioxus desktop entry point.
//!
//! Same `app_ui::App` as the web target — but here the **full backend**
//! (the `ExampleRepo` CRUD surface + `ExampleService` search/duplicate)
//! runs in-process via a vox in-memory link, so the desktop app is a
//! self-contained stack with no server to launch. The only difference
//! from web is the `Transport` provided at the root: it serves the exact
//! same `app_server::service_router` the axum server mounts.

use app_server::service_router;
use app_ui::{App, Transport};
use architect::{LocalServer, Scope};
use dioxus::prelude::*;
use example_memory::ExampleRepoMemory;

fn main() {
    dioxus::launch(Root);
}

/// Serve the full service router over a vox in-memory link and hand the
/// resulting local transport to the shared app. The scope lives for the
/// process (the in-process acceptor tasks are torn down on exit).
#[component]
fn Root() -> Element {
    let transport = use_hook(|| {
        let scope = Scope::new();
        let router = service_router(ExampleRepoMemory::new(), &app_server::Collab::ephemeral());
        Transport::Local(LocalServer::serve(router, scope))
    });
    use_context_provider(|| transport.clone());
    rsx! { App {} }
}
