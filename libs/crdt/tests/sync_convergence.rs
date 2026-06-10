//! Convergence test for the doc-sync transport: two live replicas and a
//! late joiner, all syncing one canonical doc through `DocSyncHost` over
//! the in-process vox transport. Local edits on any replica must appear
//! on every other; a replica that connects late (or reconnects) must
//! catch up by version vector.

#![cfg(not(target_arch = "wasm32"))]

use std::time::Duration;

use architect::{LayerRouter, LocalServer, Scope};
use crdt::CrdtDoc;
use crdt::sync::{DocSyncClient, DocSyncHost, SyncedDoc, doc_sync_service_descriptor};
use uuid::Uuid;

/// Poll until `pred` holds on the doc (or panic after 5s).
async fn converge(doc: &CrdtDoc, what: &str, pred: impl Fn(&loro::LoroDoc) -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if pred(doc.loro()) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn map_value(doc: &loro::LoroDoc, key: &str) -> Option<String> {
    doc.get_map("root")
        .get(key)
        .and_then(|v| v.into_value().ok())
        .and_then(|v| v.into_string().ok())
        .map(|s| s.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn replicas_converge_and_late_joiners_catch_up() {
    let doc_id = Uuid::new_v4();
    let scope = Scope::new();

    // Canonical doc + sync host, served in-process.
    let canonical = CrdtDoc::ephemeral();
    let host = DocSyncHost::new(doc_id, canonical.clone());
    let router = LayerRouter::new().with(
        doc_sync_service_descriptor(),
        crdt::sync::DocSyncDispatcher::new(host),
    );
    let local = LocalServer::serve(router, scope.clone());

    // Replica A and B: fresh docs, each on its own session.
    let client_a: DocSyncClient = local.establish().await.expect("client a");
    let mut a = SyncedDoc::new(doc_id, CrdtDoc::ephemeral());
    let doc_a = a.doc().clone();
    tokio::spawn(async move { a.run(&client_a).await });

    let client_b: DocSyncClient = local.establish().await.expect("client b");
    let mut b = SyncedDoc::new(doc_id, CrdtDoc::ephemeral());
    let doc_b = b.doc().clone();
    tokio::spawn(async move { b.run(&client_b).await });

    // A writes — B and the canonical doc must see it.
    doc_a
        .loro()
        .get_map("root")
        .insert("from-a", "hello")
        .expect("insert a");
    doc_a.loro().commit();
    converge(&doc_b, "b sees a's write", |d| {
        map_value(d, "from-a").as_deref() == Some("hello")
    })
    .await;
    converge(&canonical, "canonical sees a's write", |d| {
        map_value(d, "from-a").as_deref() == Some("hello")
    })
    .await;

    // B writes concurrently-ish — A must see it.
    doc_b
        .loro()
        .get_map("root")
        .insert("from-b", "world")
        .expect("insert b");
    doc_b.loro().commit();
    converge(&doc_a, "a sees b's write", |d| {
        map_value(d, "from-b").as_deref() == Some("world")
    })
    .await;

    // A late joiner with an empty doc catches the whole history up via
    // version-vector delta (the same path a reconnect takes).
    let client_c: DocSyncClient = local.establish().await.expect("client c");
    let mut c = SyncedDoc::new(doc_id, CrdtDoc::ephemeral());
    let doc_c = c.doc().clone();
    tokio::spawn(async move { c.run(&client_c).await });
    converge(&doc_c, "late joiner has full history", |d| {
        map_value(d, "from-a").as_deref() == Some("hello")
            && map_value(d, "from-b").as_deref() == Some("world")
    })
    .await;

    scope.close().await;
}
