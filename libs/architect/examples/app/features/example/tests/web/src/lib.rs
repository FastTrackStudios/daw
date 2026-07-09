//! Browser integration tests for the architect-generated vox client.
//!
//! Drives `ExampleRepoClient` end-to-end against a real `example-server`
//! over a WebSocket: create a row, fetch it back, list, delete, verify
//! it's gone. Every byte goes through facet encoding on the wire.
//!
//! Prerequisite: `cargo run -p app-server` in another terminal.
//! Then run from the architect repo root:
//!
//! ```sh
//! wasm-pack test --headless --chrome crates/example-test-wasm
//! ```

#![cfg(target_arch = "wasm32")]

use example::{
    ExampleCreate, ExampleRepoClient, ExampleServiceClient, ExampleUpdate,
    architect::{Page, Sort, SortOrder},
};
use uuid::Uuid;
use vox_core::initiator_on;
use vox_websocket::WsLink;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const SERVER_URL: &str = "ws://127.0.0.1:4040/vox";

async fn connect() -> ExampleRepoClient {
    let link = WsLink::connect(SERVER_URL)
        .await
        .expect("WsLink::connect — is example-server running on :4040?");
    initiator_on(link)
        .establish::<ExampleRepoClient>()
        .await
        .expect("vox handshake failed")
}

async fn connect_service() -> ExampleServiceClient {
    let link = WsLink::connect(SERVER_URL)
        .await
        .expect("WsLink::connect — is example-server running on :4040?");
    initiator_on(link)
        .establish::<ExampleServiceClient>()
        .await
        .expect("vox handshake failed")
}

#[wasm_bindgen_test]
async fn create_then_get_round_trip() {
    let client = connect().await;

    let name = format!("wasm-test-{}", Uuid::new_v4());
    let created = client
        .create(ExampleCreate {
            name: name.clone(),
            description: "round-trip from a browser".into(),
        })
        .await
        .expect("create RPC failed");

    assert_eq!(created.name, name);

    let fetched = client.get(created.id).await.expect("get RPC failed");
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.name, name);
}

#[wasm_bindgen_test]
async fn list_includes_inserted_row() {
    let client = connect().await;

    let marker = format!("list-marker-{}", Uuid::new_v4());
    let created = client
        .create(ExampleCreate {
            name: marker.clone(),
            description: "list test row".into(),
        })
        .await
        .expect("create failed");

    let page = client
        .list(
            Page {
                index: 0,
                size: 100,
            },
            Some(Sort {
                field: "name".into(),
                order: SortOrder::Asc,
            }),
            None,
        )
        .await
        .expect("list failed");

    assert!(page.total >= 1);
    let found = page.items.iter().find(|e| e.id == created.id);
    assert!(found.is_some(), "freshly-created row not in list");
}

#[wasm_bindgen_test]
async fn delete_removes_row() {
    let client = connect().await;

    let created = client
        .create(ExampleCreate {
            name: format!("delete-{}", Uuid::new_v4()),
            description: "to be deleted".into(),
        })
        .await
        .expect("create failed");

    client.delete(created.id).await.expect("delete failed");

    let err = client.get(created.id).await.unwrap_err();
    // RepoError::NotFound is what the server returns when the row's gone.
    let msg = format!("{err:?}");
    assert!(msg.contains("NotFound"), "expected NotFound, got: {msg}");
}

#[wasm_bindgen_test]
async fn update_round_trip() {
    let client = connect().await;

    let created = client
        .create(ExampleCreate {
            name: format!("update-{}", Uuid::new_v4()),
            description: "before".into(),
        })
        .await
        .expect("create failed");

    let updated = client
        .update(
            created.id,
            ExampleUpdate {
                name: None,
                description: Some("after".into()),
            },
        )
        .await
        .expect("update failed");

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.description, "after");
    assert_eq!(updated.name, created.name, "untouched field preserved");
}

#[wasm_bindgen_test]
async fn service_search_finds_inserted_row() {
    let repo = connect().await;
    let service = connect_service().await;

    let marker = format!("searchable-{}", Uuid::new_v4());
    repo.create(ExampleCreate {
        name: marker.clone(),
        description: "find me by name".into(),
    })
    .await
    .expect("create failed");

    let hits = service
        .search(marker.clone(), 10)
        .await
        .expect("search failed");
    assert!(
        hits.iter().any(|e| e.name == marker),
        "search did not return the freshly-created row"
    );
}

#[wasm_bindgen_test]
async fn service_duplicate_creates_copy() {
    let repo = connect().await;
    let service = connect_service().await;

    let original = repo
        .create(ExampleCreate {
            name: format!("dup-{}", Uuid::new_v4()),
            description: "to be copied".into(),
        })
        .await
        .expect("create failed");

    let copy = service
        .duplicate(original.id, None)
        .await
        .expect("duplicate failed");

    assert_ne!(copy.id, original.id, "copy must be a new row");
    assert_eq!(copy.description, original.description);
    assert!(
        copy.name.ends_with("(copy)"),
        "default duplicate name should be suffixed, got {}",
        copy.name
    );
}
