//! Runtime UI shell — the full example app, shared verbatim by the web
//! (wasm) and desktop targets.
//!
//! The shell owns three things and nothing else:
//! - **Transport** ([`transport`]) — *where* the clients connect (remote
//!   WebSocket vs in-process), chosen once at the root.
//! - **Context provision** — `App` provides the app-wide registries
//!   (notifications, reactivity), the entity's derived store, and one
//!   `architect::Connection<C>` per typed client; the feature hooks
//!   consume them.
//! - **Routing + composition** ([`Route`], [`pages`]) — the typed route
//!   table and pages that `match` the feature hooks' `AtomResult` phases
//!   and navigate with normal Dioxus `Link` / `nav`.
//!
//! Data never flows through Dioxus server functions — every read/write is a
//! typed vox call. See `docs/.../idioms.md`.

mod pages;
mod status;
mod transport;

use architect::use_app;
use dioxus::prelude::*;
use example_ui::provide_example;

pub use transport::{DEFAULT_SERVER_URL, Transport};

use pages::{Collab, EditExample, ExampleDetail, Home, InspectExample, NotFound};
use status::NotificationTray;

/// Bundled stylesheet. `dx` collects it for both web and desktop.
static MAIN_CSS: Asset = asset!("/assets/main.css");

/// Typed route table. `AppShell` wraps the content routes; anything else
/// falls through to `NotFound`.
#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(AppShell)]
        #[route("/")]
        Home {},
        #[route("/example/:id")]
        ExampleDetail { id: String },
        #[route("/example/:id/edit")]
        EditExample { id: String },
        #[route("/example/:id/inspect")]
        InspectExample { id: String },
        #[route("/collab")]
        Collab {},
    #[end_layout]
    #[route("/:..route")]
    NotFound { route: Vec<String> },
}

/// App root. Picks the transport, provides the app-wide registries + the
/// derived store + one connection per typed client, and mounts the router.
#[component]
pub fn App() -> Element {
    // The transport is chosen at the app root (web → Remote, desktop →
    // Local). Default to Remote so a bare `App` mount (tests) still works.
    let transport = use_hook(|| {
        try_consume_context::<Transport>()
            .unwrap_or_else(|| Transport::Remote(DEFAULT_SERVER_URL.to_string()))
    });

    // App-wide registries (notifications + reactivity) and the app's
    // **single** connection — every feature's typed clients are cheap
    // views over the shared `Caller`, so adding a feature adds zero
    // connections.
    let connect_transport = transport.clone();
    use_app(move || async move { transport::connect(&connect_transport).await });

    // The example feature's whole client layer: the derived store plus
    // the live event subscription that keeps it fed (all pages stay in
    // sync with other clients without polling). One line per feature.
    provide_example();

    // The collaborative layer: a local replica of the shared notes doc
    // (synced over the same connection — reconnects are version-vector
    // deltas) and the presence channel. The replica is usable
    // immediately, online or not.
    crdt::use_synced_doc(example::COLLAB_DOC_ID);
    crdt::use_presence_channel(example::COLLAB_DOC_ID, 30_000);

    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        Router::<Route> {}
    }
}

/// Layout shared by every content route: header/brand + the page outlet,
/// plus the app-wide notification tray (where rolled-back mutations that
/// navigated away report their failures).
#[component]
fn AppShell() -> Element {
    rsx! {
        header { class: "app-header",
            Link { class: "brand", to: Route::Home {}, "architect" }
            span { class: "tagline", "reference example" }
            nav { class: "app-nav",
                Link { class: "app-nav__link", to: Route::Home {}, "Examples" }
                Link { class: "app-nav__link", to: Route::Collab {}, "Collab" }
            }
        }
        NotificationTray {}
        main { class: "app-main",
            Outlet::<Route> {}
        }
    }
}
