//! App-level end-to-end tests.
//!
//! Mounts the *real* server router (`app_server::vox_router`, in-memory
//! backend) on an ephemeral TCP port, then drives it with real vox
//! clients over a WebSocket — the same `ExampleRepoClient` /
//! `ExampleServiceClient` a browser or the CLI establishes. Every byte
//! goes through facet encoding on the wire, so this is the test that
//! catches transport + schema regressions (it's what surfaced the vox
//! Uuid-encoding bug).
//!
//! Run with: `cargo test -p app-tests-e2e`

#![cfg(test)]

use app_server::vox_router;
use example::architect::{Page, Sort, SortOrder};
use example::backend_memory::ExampleRepoMemory;
use example::{ExampleCreate, ExampleRepoClient, ExampleServiceClient, ExampleUpdate};
use tokio::sync::oneshot;
use vox_core::initiator_on;
use vox_websocket::WsLink;

/// Spawn the real router on an OS-assigned port. Returns the `/vox` URL
/// and a shutdown sender — send `()` (or drop it) to stop the server.
async fn spawn() -> (String, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let app = vox_router(ExampleRepoMemory::new(), app_server::Collab::ephemeral());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });
    (format!("ws://{addr}/vox"), shutdown_tx)
}

async fn repo_client(ws_url: &str) -> ExampleRepoClient {
    let link = WsLink::connect(ws_url).await.expect("WsLink::connect");
    initiator_on(link)
        .establish::<ExampleRepoClient>()
        .await
        .expect("ExampleRepo handshake")
}

async fn service_client(ws_url: &str) -> ExampleServiceClient {
    let link = WsLink::connect(ws_url).await.expect("WsLink::connect");
    initiator_on(link)
        .establish::<ExampleServiceClient>()
        .await
        .expect("ExampleService handshake")
}

fn page(size: u32) -> Page {
    Page { index: 0, size }
}

#[tokio::test]
async fn health_endpoint_serves_ok() {
    let (ws_url, shutdown) = spawn().await;
    let http = ws_url
        .replace("ws://", "http://")
        .replace("/vox", "/api/health");
    assert_eq!(reqwest_health(&http).await, "ok");
    let _ = shutdown.send(());
}

#[tokio::test]
async fn repo_full_crud_round_trip() {
    let (ws_url, shutdown) = spawn().await;
    let client = repo_client(&ws_url).await;

    // create
    let created = client
        .create(ExampleCreate {
            name: "alpha".into(),
            description: "first".into(),
        })
        .await
        .expect("create");
    assert_eq!(created.name, "alpha");

    // get
    let fetched = client.get(created.id).await.expect("get");
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.description, "first");

    // update (partial: name only)
    let updated = client
        .update(
            created.id,
            ExampleUpdate {
                name: Some("alpha-renamed".into()),
                description: None,
            },
        )
        .await
        .expect("update");
    assert_eq!(updated.name, "alpha-renamed");
    assert_eq!(updated.description, "first", "untouched field preserved");

    // list shows it
    let list = client
        .list(
            page(100),
            Some(Sort {
                field: "name".into(),
                order: SortOrder::Asc,
            }),
            None,
        )
        .await
        .expect("list");
    assert!(list.items.iter().any(|e| e.id == created.id));

    // delete, then get -> NotFound
    client.delete(created.id).await.expect("delete");
    let err = client.get(created.id).await.unwrap_err();
    assert!(
        format!("{err:?}").contains("NotFound"),
        "expected NotFound after delete, got {err:?}"
    );

    let _ = shutdown.send(());
}

#[tokio::test]
async fn service_search_matches_name_and_description() {
    let (ws_url, shutdown) = spawn().await;
    let repo = repo_client(&ws_url).await;
    let service = service_client(&ws_url).await;

    for (name, desc) in [
        ("apple pie", "a dessert"),
        ("banana bread", "also a dessert"),
        ("car engine", "not food"),
    ] {
        repo.create(ExampleCreate {
            name: name.into(),
            description: desc.into(),
        })
        .await
        .expect("seed create");
    }

    // matches a name
    let by_name = service.search("banana".into(), 10).await.expect("search");
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].name, "banana bread");

    // matches a description across rows (case-insensitive)
    let by_desc = service.search("DESSERT".into(), 10).await.expect("search");
    assert_eq!(by_desc.len(), 2, "both desserts match on description");

    // empty query is rejected
    let err = service.search("   ".into(), 10).await.unwrap_err();
    assert!(format!("{err:?}").contains("InvalidInput"));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn service_duplicate_copies_row() {
    let (ws_url, shutdown) = spawn().await;
    let repo = repo_client(&ws_url).await;
    let service = service_client(&ws_url).await;

    let original = repo
        .create(ExampleCreate {
            name: "template".into(),
            description: "reusable".into(),
        })
        .await
        .expect("create");

    // default name = "<name> (copy)"
    let copy = service
        .duplicate(original.id, None)
        .await
        .expect("duplicate");
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.name, "template (copy)");
    assert_eq!(copy.description, "reusable");

    // explicit new name
    let named = service
        .duplicate(original.id, Some("renamed copy".into()))
        .await
        .expect("duplicate named");
    assert_eq!(named.name, "renamed copy");

    // both copies + original are present
    let list = repo.list(page(100), None, None).await.expect("list");
    assert_eq!(list.total, 3);

    // duplicating a missing row -> NotFound
    let err = service
        .duplicate(uuid::Uuid::new_v4(), None)
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("NotFound"));

    let _ = shutdown.send(());
}

// Same contract, no server: serve the backend in-process over a vox
// in-memory link and drive the *same* generated clients. This is the
// "inject the transport" payoff — the desktop app runs exactly this way.
#[tokio::test]
async fn local_transport_round_trip() {
    use app_server::service_router;
    use architect::{LocalServer, Scope};

    let scope = Scope::new();
    // Serve the full surface (repo + ExampleService) in-process — the same
    // `service_router` the axum server mounts, just over an in-memory link.
    let local = LocalServer::serve(
        service_router(ExampleRepoMemory::new(), &app_server::Collab::ephemeral()),
        scope.clone(),
    );
    let repo: ExampleRepoClient = local.establish().await.expect("local repo establish");
    let service: ExampleServiceClient = local.establish().await.expect("local service establish");

    // create → get (no socket, no server)
    let created = repo
        .create(ExampleCreate {
            name: "local".into(),
            description: "in-process".into(),
        })
        .await
        .expect("create");
    let got = repo.get(created.id).await.expect("get");
    assert_eq!(got.name, "local");

    // the ExampleService surface works identically in-process
    let hits = service.search("local".into(), 10).await.expect("search");
    assert_eq!(hits.len(), 1);
    let copy = service
        .duplicate(created.id, None)
        .await
        .expect("duplicate");
    assert_eq!(copy.name, "local (copy)");

    scope.close().await;
}

// String primary keys over the wire: `Tag` is keyed by a caller-supplied
// `slug: String` (no Uuid anywhere). Serve `TagEvented` in-process and
// drive the generated clients — every id round-trips facet encoding, the
// repo methods take `id: String`, and `TagEvent::Deleted` broadcasts the
// String id (the publish happens *after* the inner delete consumed one
// clone of the key).
#[tokio::test]
async fn string_pk_local_transport_and_events() {
    use architect::{LocalServer, Scope, Services as _};
    use example::architect::vox;
    use example::backend_memory::TagRepoMemory;
    use example::{TagCreate, TagEvent, TagEventsClient, TagRepoClient, TagUpdate};
    use tokio::time::{Duration, timeout};

    let scope = Scope::new();
    // `TagEvented`'s Services bundle mounts CRUD + the event feed.
    let evented = example::TagEvented::new(TagRepoMemory::new());
    let local = LocalServer::serve(evented.into_router(), scope.clone());

    let repo: TagRepoClient = local.establish().await.expect("local tag repo establish");
    let events: TagEventsClient = local.establish().await.expect("local tag events establish");

    let (tx, mut rx) = vox::channel::<TagEvent>();
    events.subscribe(tx).await.expect("subscribe");

    async fn recv_owned(rx: &mut vox::Rx<TagEvent>) -> TagEvent {
        let item = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event within 5s")
            .expect("stream healthy")
            .expect("stream open");
        let mut owned: Option<TagEvent> = None;
        let _ = item.map(|e| owned = Some(e.clone()));
        owned.expect("owned event")
    }

    // Snapshot first (empty repo), then the broadcasts.
    match recv_owned(&mut rx).await {
        TagEvent::Snapshot(rows) => assert!(rows.is_empty(), "expected empty snapshot: {rows:?}"),
        other => panic!("expected Snapshot first, got {other:?}"),
    }

    let created = repo
        .create(TagCreate {
            slug: "rust-lang".into(),
            label: "Rust".into(),
        })
        .await
        .expect("create");
    assert_eq!(created.slug, "rust-lang");
    match recv_owned(&mut rx).await {
        TagEvent::Upserted(row) => assert_eq!(row.slug, "rust-lang"),
        other => panic!("expected Upserted after create, got {other:?}"),
    }

    let updated = repo
        .update(
            "rust-lang".into(),
            TagUpdate {
                label: Some("Rust (the language)".into()),
            },
        )
        .await
        .expect("update");
    assert_eq!(updated.label, "Rust (the language)");
    match recv_owned(&mut rx).await {
        TagEvent::Upserted(row) => assert_eq!(row.label, "Rust (the language)"),
        other => panic!("expected Upserted after update, got {other:?}"),
    }

    repo.delete("rust-lang".into()).await.expect("delete");
    match recv_owned(&mut rx).await {
        TagEvent::Deleted(id) => assert_eq!(id, "rust-lang"),
        other => panic!("expected Deleted, got {other:?}"),
    }

    scope.close().await;
}

/// Live events over the real WebSocket: subscribe with a vox channel,
/// mutate through a *different* client connection, and assert the
/// broadcast arrives — the current row set as `Snapshot` **first**
/// (effect-`SubscriptionRef` semantics), then create/update as
/// `Upserted` and delete as `Deleted`. This is the wire proof for
/// `architect::PubSub` + the streaming sibling-trait pattern (and what
/// the Dioxus `use_store_stream` hook rides in the browser).
#[tokio::test]
async fn events_stream_broadcasts_writes() {
    use example::architect::vox;
    use example::{ExampleEvent, ExampleEventsClient};
    use tokio::time::{Duration, timeout};

    let (ws_url, shutdown) = spawn().await;

    // A row that exists *before* anyone subscribes — it must arrive in
    // the snapshot, not as a change event.
    let repo = repo_client(&ws_url).await;
    let pre_existing = repo
        .create(ExampleCreate {
            name: "pre-existing".into(),
            description: "before subscribe".into(),
        })
        .await
        .expect("create pre-existing");

    // The subscriber is a *view over the writer's own socket* — every
    // typed client is `Client::new(caller)` over one shared connection
    // (the single-socket model the app shell uses). A second socket works
    // identically; this proves multiplexing.
    let events = ExampleEventsClient::new(repo.caller.clone());
    let (tx, mut rx) = vox::channel::<ExampleEvent>();
    events.subscribe(tx).await.expect("subscribe");

    // Writes on the same connection still broadcast back to it.
    let created = repo
        .create(ExampleCreate {
            name: "streamed".into(),
            description: "live".into(),
        })
        .await
        .expect("create");

    async fn recv_owned(rx: &mut vox::Rx<ExampleEvent>) -> ExampleEvent {
        let item = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event within 5s")
            .expect("stream healthy")
            .expect("stream open");
        let mut owned: Option<ExampleEvent> = None;
        let _ = item.map(|e| owned = Some(e.clone()));
        owned.expect("owned event")
    }

    // First event: the snapshot, containing the pre-existing row (and
    // possibly the just-created one — the create raced the snapshot read;
    // either way the *next* delivery of `created` is the Upserted).
    match recv_owned(&mut rx).await {
        ExampleEvent::Snapshot(rows) => {
            assert!(
                rows.iter().any(|r| r.id == pre_existing.id),
                "snapshot must contain the pre-existing row: {rows:?}"
            );
        }
        other => panic!("expected Snapshot first, got {other:?}"),
    }

    match recv_owned(&mut rx).await {
        ExampleEvent::Upserted(row) => {
            assert_eq!(row.id, created.id);
            assert_eq!(row.name, "streamed");
        }
        other => panic!("expected Upserted after create, got {other:?}"),
    }

    repo.update(
        created.id,
        ExampleUpdate {
            name: Some("streamed-2".into()),
            description: None,
        },
    )
    .await
    .expect("update");
    match recv_owned(&mut rx).await {
        ExampleEvent::Upserted(row) => assert_eq!(row.name, "streamed-2"),
        other => panic!("expected Upserted after update, got {other:?}"),
    }

    repo.delete(created.id).await.expect("delete");
    match recv_owned(&mut rx).await {
        ExampleEvent::Deleted(id) => assert_eq!(id, created.id),
        other => panic!("expected Deleted after delete, got {other:?}"),
    }

    let _ = shutdown.send(());
}

// Tiny stdlib HTTP client to avoid pulling reqwest in for one GET.
async fn reqwest_health(url: &str) -> String {
    let (host_port, path) = url.trim_start_matches("http://").split_once('/').unwrap();
    let mut stream = tokio::net::TcpStream::connect(host_port).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!("GET /{path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let s = String::from_utf8_lossy(&buf);
    s.rsplit("\r\n\r\n").next().unwrap_or("").trim().to_string()
}

// ── Real-time collaboration over the real socket ──────────────────────
//
// The same DocSync/DocPresence services the Collab page uses, driven by
// two real replicas over two real WebSocket connections. Proves the
// whole sync stack — channels through axum_ws, version-vector catch-up,
// fan-out across connections — not just the in-process transport.

async fn sync_client(ws_url: &str) -> crdt::sync::DocSyncClient {
    let link = WsLink::connect(ws_url).await.expect("WsLink::connect");
    initiator_on(link)
        .establish::<crdt::sync::DocSyncClient>()
        .await
        .expect("DocSync handshake")
}

async fn presence_client(ws_url: &str) -> crdt::sync::DocPresenceClient {
    let link = WsLink::connect(ws_url).await.expect("WsLink::connect");
    initiator_on(link)
        .establish::<crdt::sync::DocPresenceClient>()
        .await
        .expect("DocPresence handshake")
}

/// Poll until `pred` (or panic after 5s).
async fn eventually(what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !pred() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn notes_replicas_converge_over_websocket() {
    use example::{COLLAB_DOC_ID, Note, NoteCreate, NoteRepoLoro, NoteUpdate};

    let (ws_url, shutdown) = spawn().await;

    // Two replicas on two sockets — the CRDT-flag derive gives each a
    // typed repo over its local doc.
    let doc_a = crdt::CrdtDoc::ephemeral();
    let mut synced_a = crdt::sync::SyncedDoc::new(COLLAB_DOC_ID, doc_a.clone());
    let client_a = sync_client(&ws_url).await;
    tokio::spawn(async move { synced_a.run(&client_a).await });
    let repo_a = NoteRepoLoro::new(&doc_a);

    let doc_b = crdt::CrdtDoc::ephemeral();
    let mut synced_b = crdt::sync::SyncedDoc::new(COLLAB_DOC_ID, doc_b.clone());
    let client_b = sync_client(&ws_url).await;
    tokio::spawn(async move { synced_b.run(&client_b).await });
    let repo_b = NoteRepoLoro::new(&doc_b);

    // A writes through its local repo — instant locally, synced out.
    let note = repo_a
        .inner()
        .create(NoteCreate {
            text: "hello from a".into(),
            author: "a".into(),
        })
        .await
        .expect("create on a");

    let items_of = |repo: &NoteRepoLoro| -> Vec<Note> { repo.inner().items_now().unwrap() };

    let rb = repo_b.clone();
    eventually("b sees a's note", move || {
        items_of(&rb).iter().any(|n| n.text == "hello from a")
    })
    .await;

    // B edits the same row — the update merges back to A.
    repo_b
        .inner()
        .update(
            note.id,
            NoteUpdate {
                text: Some("edited by b".into()),
                author: None,
            },
        )
        .await
        .expect("update on b");
    let ra = repo_a.clone();
    eventually("a sees b's edit", move || {
        items_of(&ra).iter().any(|n| n.text == "edited by b")
    })
    .await;

    // A late joiner catches the whole history by version vector.
    let doc_c = crdt::CrdtDoc::ephemeral();
    let mut synced_c = crdt::sync::SyncedDoc::new(COLLAB_DOC_ID, doc_c.clone());
    let client_c = sync_client(&ws_url).await;
    tokio::spawn(async move { synced_c.run(&client_c).await });
    let repo_c = NoteRepoLoro::new(&doc_c);
    eventually("late joiner has the converged note", move || {
        items_of(&repo_c).iter().any(|n| n.text == "edited by b")
    })
    .await;

    let _ = shutdown.send(());
}

#[tokio::test]
async fn presence_propagates_between_peers() {
    use example::COLLAB_DOC_ID;

    let (ws_url, shutdown) = spawn().await;

    let (peer_a, mut driver_a) = crdt::sync::PresencePeer::new(COLLAB_DOC_ID, 30_000);
    let client_a = presence_client(&ws_url).await;
    tokio::spawn(async move { driver_a.run(&client_a).await });

    let (peer_b, mut driver_b) = crdt::sync::PresencePeer::new(COLLAB_DOC_ID, 30_000);
    let client_b = presence_client(&ws_url).await;
    tokio::spawn(async move { driver_b.run(&client_b).await });

    peer_a.set("client-a", "alice");
    let pb = peer_b.clone();
    eventually("b sees a's presence", move || {
        matches!(pb.states().get("client-a"), Some(v) if format!("{v:?}").contains("alice"))
    })
    .await;

    // A peer that joins later gets the current picture from the
    // server's mirror store on attach.
    let (peer_c, mut driver_c) = crdt::sync::PresencePeer::new(COLLAB_DOC_ID, 30_000);
    let client_c = presence_client(&ws_url).await;
    tokio::spawn(async move { driver_c.run(&client_c).await });
    let pc = peer_c.clone();
    eventually("late joiner sees a's presence", move || {
        matches!(pc.states().get("client-a"), Some(v) if format!("{v:?}").contains("alice"))
    })
    .await;

    let _ = shutdown.send(());
}

/// Two *different* docs sync through the registry the server mounts as
/// one `DocSync` dispatcher: the well-known notes doc and a second doc
/// opened on demand, each converging among its own replicas with no
/// bleed between them.
#[tokio::test]
async fn two_docs_sync_through_one_registry() {
    use example::COLLAB_DOC_ID;

    let (ws_url, shutdown) = spawn().await;
    let second_doc = uuid::Uuid::new_v4();

    // One replica pair per doc, all four over the same mounted services.
    let mut docs = Vec::new();
    for doc_id in [COLLAB_DOC_ID, COLLAB_DOC_ID, second_doc, second_doc] {
        let doc = crdt::CrdtDoc::ephemeral();
        let mut synced = crdt::sync::SyncedDoc::new(doc_id, doc.clone());
        let client = sync_client(&ws_url).await;
        tokio::spawn(async move { synced.run(&client).await });
        docs.push(doc);
    }
    let (notes_a, notes_b, second_a, second_b) = (&docs[0], &docs[1], &docs[2], &docs[3]);

    notes_a
        .loro()
        .get_map("root")
        .insert("notes-key", "notes-value")
        .expect("insert on notes doc");
    notes_a.loro().commit();
    second_a
        .loro()
        .get_map("root")
        .insert("second-key", "second-value")
        .expect("insert on second doc");
    second_a.loro().commit();

    let value_of = |doc: &crdt::CrdtDoc, key: &str| -> Option<String> {
        doc.loro()
            .get_map("root")
            .get(key)
            .and_then(|v| v.into_value().ok())
            .and_then(|v| v.into_string().ok())
            .map(|s| s.to_string())
    };

    let nb = notes_b.clone();
    eventually("notes replicas converge", move || {
        value_of(&nb, "notes-key").as_deref() == Some("notes-value")
    })
    .await;
    let sb = second_b.clone();
    eventually("second doc replicas converge", move || {
        value_of(&sb, "second-key").as_deref() == Some("second-value")
    })
    .await;

    // Isolation: distinct collaboration boundaries behind one dispatcher.
    assert_eq!(
        value_of(notes_b, "second-key"),
        None,
        "notes doc saw the second doc's write"
    );
    assert_eq!(
        value_of(second_b, "notes-key"),
        None,
        "second doc saw the notes doc's write"
    );

    let _ = shutdown.send(());
}
