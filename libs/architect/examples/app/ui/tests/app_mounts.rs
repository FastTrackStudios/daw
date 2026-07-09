//! Headless smoke test for the shared app shell.
//!
//! The web and desktop binaries are ~3 lines each (`dioxus::launch(App)`);
//! the real surface is `app_ui::App` — the router, the connection
//! lifecycle, the screens. A windowed end-to-end run isn't CI-friendly,
//! so here we mount the whole thing in a VirtualDom and render the
//! initial frame. That proves the route table, layout, and the
//! pre-connection state all build and render without a server present
//! (it should show the shell + the home composer + the loading phase).

use app_ui::App;
use dioxus::prelude::*;

#[test]
fn app_mounts_and_renders_initial_frame() {
    let mut dom = VirtualDom::new(App);
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);

    // Shell chrome from AppShell.
    assert!(html.contains("architect"), "brand missing: {html}");
    assert!(
        html.contains("reference example"),
        "tagline missing: {html}"
    );
    // Home is the index route: the create composer is always present, and
    // before the socket is up the list is in its loading phase (no server
    // in this test).
    assert!(
        html.contains("composer"),
        "expected the create composer on the index route: {html}"
    );
    assert!(
        html.to_lowercase().contains("loading"),
        "expected the loading phase on the index route: {html}"
    );
}
