//! Native integration test of the **whole live loop**: mount the real
//! `App` over the in-process transport, write through a *separate* client
//! (as another user would), and assert the mounted page updates — fetch,
//! store, event subscription, and render, end to end, no browser.
//!
//! This is the layer between the SSR component tests (markup only) and
//! the browser e2e (real WebSocket): it proves the derived client layer
//! (`provide_example` → store + events) actually drives the UI.

use app_server::service_router;
use app_ui::{App, Transport};
use dioxus::prelude::*;
use example::ExampleCreate;
use example::architect::{LocalServer, Scope};
use example::backend_memory::ExampleRepoMemory;
use example::{ExampleRepoClient, ExampleUpdate};
use std::time::Duration;

/// Pump the VirtualDom until the rendered HTML satisfies `pred` (or
/// panic after `secs`). Applies work as it arrives; the 25ms tick keeps
/// us re-checking while async tasks (connects, subscriptions) settle.
async fn pump_until(dom: &mut VirtualDom, secs: u64, pred: impl Fn(&str) -> bool) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let html = dioxus_ssr::render(dom);
        if pred(&html) {
            return html;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for condition; last html:\n{html}"
        );
        tokio::select! {
            _ = dom.wait_for_work() => dom.render_immediate(&mut dioxus_core::NoOpMutations),
            _ = tokio::time::sleep(Duration::from_millis(25)) => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn external_writes_appear_live_in_the_mounted_app() {
    let scope = Scope::new();
    // The real service graph (CRUD + search + live events), in-process.
    let local = LocalServer::serve(
        service_router(ExampleRepoMemory::new(), &app_server::Collab::ephemeral()),
        scope.clone(),
    );

    // The "other user": a plain client on its own in-process session.
    let other: ExampleRepoClient = local.establish().await.expect("other client");

    // Mount the real App over the Local transport.
    let mut dom = VirtualDom::new(App).with_root_context(Transport::Local(local));
    dom.rebuild_in_place();

    // The app connects, subscribes, and receives the (empty) snapshot.
    pump_until(&mut dom, 10, |html| html.contains("No examples yet")).await;

    // Someone else creates a row — the mounted app must show it without
    // any local interaction (event → store → render).
    let row = other
        .create(ExampleCreate {
            name: "from-elsewhere".into(),
            description: "pushed live".into(),
        })
        .await
        .expect("external create");
    pump_until(&mut dom, 10, |html| html.contains("from-elsewhere")).await;

    // …updates propagate…
    other
        .update(
            row.id,
            ExampleUpdate {
                name: Some("renamed-live".into()),
                description: None,
            },
        )
        .await
        .expect("external update");
    pump_until(&mut dom, 10, |html| html.contains("renamed-live")).await;

    // …and deletions bring back the empty state.
    other.delete(row.id).await.expect("external delete");
    pump_until(&mut dom, 10, |html| html.contains("No examples yet")).await;

    scope.close().await;
}
