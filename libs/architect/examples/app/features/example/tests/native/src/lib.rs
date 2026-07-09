//! Native integration tests for the example feature. See `tests/web`
//! for the matching browser/wasm coverage that exercises the same
//! contract over a real vox WebSocket against the server.

#![cfg(test)]

use architect::{Page, RepoError, Sort, SortOrder};
use example_memory::ExampleRepoMemory;
use example_proto::{ExampleCreate, ExampleRepo, ExampleUpdate};

fn repo() -> ExampleRepoMemory {
    ExampleRepoMemory::new()
}

// r[verify repo.create.id]
#[tokio::test]
async fn create_then_get_round_trip() {
    let r = repo();
    let created = r
        .create(ExampleCreate {
            name: "alpha".into(),
            description: "first".into(),
        })
        .await
        .unwrap();
    assert_eq!(created.name, "alpha");

    let got = r.get(created.id).await.unwrap();
    assert_eq!(got.id, created.id);
    assert_eq!(got.name, "alpha");
}

// r[verify repo.list.sort.name]
#[tokio::test]
async fn list_sorted_by_name_ascending() {
    let r = repo();
    for n in ["charlie", "alpha", "bravo"] {
        r.create(ExampleCreate {
            name: n.into(),
            description: String::new(),
        })
        .await
        .unwrap();
    }
    let page = r
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
        .unwrap();
    let names: Vec<_> = page.items.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
}

// r[verify repo.update.partial]
#[tokio::test]
async fn update_changes_fields() {
    let r = repo();
    let created = r
        .create(ExampleCreate {
            name: "before".into(),
            description: "old".into(),
        })
        .await
        .unwrap();
    let updated = r
        .update(
            created.id,
            ExampleUpdate {
                name: Some("after".into()),
                description: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "after");
    assert_eq!(updated.description, "old");
}

// r[verify repo.delete.missing]
// r[verify repo.get.missing]
#[tokio::test]
async fn delete_removes_row() {
    let r = repo();
    let created = r
        .create(ExampleCreate {
            name: "tmp".into(),
            description: String::new(),
        })
        .await
        .unwrap();
    r.delete(created.id).await.unwrap();
    assert!(matches!(r.get(created.id).await, Err(RepoError::NotFound)));
}

// ── Fake-rs seeding ───────────────────────────────────────────────────
//
// `example/fake` adds `#[derive(Dummy)]` to every wire + emitted
// struct, and (because `repo` is on too) emits a `seed_fake_example`
// helper that loops + creates in one call. Pass it a count or a range.

#[tokio::test]
async fn seed_fake_with_fixed_count() {
    let r = repo();
    let rows = example_proto::seed_fake_example(&r, 5usize).await.unwrap();
    assert_eq!(rows.len(), 5);
    let page = r
        .list(
            Page {
                index: 0,
                size: 100,
            },
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.total, 5);
}

#[tokio::test]
async fn seed_fake_with_range_picks_within_bounds() {
    let r = repo();
    let rows = example_proto::seed_fake_example(&r, 3..8usize)
        .await
        .unwrap();
    assert!(rows.len() >= 3 && rows.len() < 8);
}

#[tokio::test]
async fn seed_with_seeded_rng_is_deterministic() {
    use fake::Fake;
    use fake::rand::SeedableRng;
    let mut rng_a = fake::rand::rngs::StdRng::seed_from_u64(42);
    let mut rng_b = fake::rand::rngs::StdRng::seed_from_u64(42);
    let a: ExampleCreate = fake::Faker.fake_with_rng(&mut rng_a);
    let b: ExampleCreate = fake::Faker.fake_with_rng(&mut rng_b);
    assert_eq!(a.name, b.name);
    assert_eq!(a.description, b.description);
}

// r[verify repo.list.sort.unknown]
#[tokio::test]
async fn unsortable_field_errors() {
    let r = repo();
    let err = r
        .list(
            Page { index: 0, size: 10 },
            Some(Sort {
                field: "description".into(),
                order: SortOrder::Asc,
            }),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::InvalidInput(_)));
}

// ── String primary keys (`Tag`) ───────────────────────────────────────
//
// `Tag` is keyed by a caller-supplied `slug: String` — no Uuid anywhere.
// The derive threads the pk type through every emission, so the same
// repo contract works with a non-`Copy` key: `TagCreate` carries the
// slug, the repo methods take `id: String`, and `TagEvent::Deleted`
// broadcasts the String id (see app-tests-e2e for the wire round-trip).

use example_proto::{TagCreate, TagRepo, TagUpdate};

fn tag_repo() -> example_memory::TagRepoMemory {
    example_memory::TagRepoMemory::new()
}

#[tokio::test]
async fn string_pk_create_then_get_round_trip() {
    let r = tag_repo();
    let created = r
        .create(TagCreate {
            slug: "rust-lang".into(),
            label: "Rust".into(),
        })
        .await
        .unwrap();
    assert_eq!(created.slug, "rust-lang");

    let got = r.get("rust-lang".into()).await.unwrap();
    assert_eq!(got.slug, created.slug);
    assert_eq!(got.label, "Rust");
}

#[tokio::test]
async fn string_pk_duplicate_slug_conflicts() {
    let r = tag_repo();
    r.create(TagCreate {
        slug: "dup".into(),
        label: "first".into(),
    })
    .await
    .unwrap();
    let err = r
        .create(TagCreate {
            slug: "dup".into(),
            label: "second".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::Conflict(_)));
}

#[tokio::test]
async fn string_pk_update_changes_fields() {
    let r = tag_repo();
    r.create(TagCreate {
        slug: "wip".into(),
        label: "before".into(),
    })
    .await
    .unwrap();
    let updated = r
        .update(
            "wip".into(),
            TagUpdate {
                label: Some("after".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.slug, "wip");
    assert_eq!(updated.label, "after");
}

#[tokio::test]
async fn string_pk_delete_removes_row() {
    let r = tag_repo();
    r.create(TagCreate {
        slug: "gone".into(),
        label: "soon".into(),
    })
    .await
    .unwrap();
    r.delete("gone".into()).await.unwrap();
    assert!(matches!(
        r.get("gone".into()).await,
        Err(RepoError::NotFound)
    ));
}

// ── External backend conformance ──────────────────────────────────────
//
// Same contract, different impl. Proves the third-party extension
// pattern: a backend that depends only on `example-proto` (no
// `architect/server`, no SeaORM, no other in-tree backend) satisfies
// the same `ExampleRepo` trait the in-tree memory backend does.
//
// Both tests use the trait surface only — there's no way for the test
// body to tell it's running against `StubBackend` vs `ExampleRepoMemory`.

// r[verify repo.create.id]
#[tokio::test]
async fn external_backend_round_trip() {
    let r = example_stub_backend::StubBackend::new();
    let created = r
        .create(ExampleCreate {
            name: "external".into(),
            description: "from a third-party-shaped crate".into(),
        })
        .await
        .unwrap();
    let got = r.get(created.id).await.unwrap();
    assert_eq!(got.id, created.id);
    assert_eq!(got.name, "external");
}

// ── Loro CRDT backend conformance ─────────────────────────────────────
//
// Same `ExampleRepo` contract, Loro-backed implementation. The doc
// is the source of truth; the trait surface stays identical. If
// these tests pass, the contract is genuinely backend-agnostic and
// generalizing into `features/crdt/crdt` is safe.

fn crdt_repo() -> example_crdt::ExampleRepoLoro {
    example_crdt::ExampleRepoLoro::new(&example_crdt::CrdtDoc::ephemeral())
}

// r[verify repo.create.id]
#[tokio::test]
async fn crdt_create_then_get_round_trip() {
    let r = crdt_repo();
    let created = r
        .create(ExampleCreate {
            name: "alpha".into(),
            description: "first".into(),
        })
        .await
        .unwrap();
    let got = r.get(created.id).await.unwrap();
    assert_eq!(got.id, created.id);
    assert_eq!(got.name, "alpha");
    assert_eq!(got.description, "first");
}

// r[verify repo.list.sort.name]
#[tokio::test]
async fn crdt_list_sorted_by_name() {
    let r = crdt_repo();
    for n in ["charlie", "alpha", "bravo"] {
        r.create(ExampleCreate {
            name: n.into(),
            description: String::new(),
        })
        .await
        .unwrap();
    }
    let page = r
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
        .unwrap();
    let names: Vec<_> = page.items.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
}

// r[verify repo.update.partial]
#[tokio::test]
async fn crdt_update_partial() {
    let r = crdt_repo();
    let created = r
        .create(ExampleCreate {
            name: "before".into(),
            description: "old".into(),
        })
        .await
        .unwrap();
    let updated = r
        .update(
            created.id,
            ExampleUpdate {
                name: Some("after".into()),
                description: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "after");
    assert_eq!(updated.description, "old");
}

// r[verify repo.delete.missing]
#[tokio::test]
async fn crdt_delete_removes_row() {
    let r = crdt_repo();
    let created = r
        .create(ExampleCreate {
            name: "tmp".into(),
            description: String::new(),
        })
        .await
        .unwrap();
    r.delete(created.id).await.unwrap();
    assert!(matches!(r.get(created.id).await, Err(RepoError::NotFound)));
}

#[tokio::test]
async fn crdt_seed_fake_with_fixed_count() {
    let r = crdt_repo();
    let rows = example_proto::seed_fake_example(&r, 5usize).await.unwrap();
    assert_eq!(rows.len(), 5);
    let page = r
        .list(
            Page {
                index: 0,
                size: 100,
            },
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.total, 5);
}

// Concurrent-replica merge: two repos backed by independent docs,
// each writes locally, exchange their update bytes, both converge.
// This is the test that says "yes, this is actually a CRDT, not
// just an in-memory store with extra steps."
#[tokio::test]
async fn crdt_two_replicas_converge_after_sync() {
    use loro::ExportMode;

    let a = crdt_repo();
    let b = crdt_repo();

    let alpha = a
        .create(ExampleCreate {
            name: "from-a".into(),
            description: "A".into(),
        })
        .await
        .unwrap();
    let beta = b
        .create(ExampleCreate {
            name: "from-b".into(),
            description: "B".into(),
        })
        .await
        .unwrap();

    let a_bytes = a.doc().export(ExportMode::all_updates()).unwrap();
    let b_bytes = b.doc().export(ExportMode::all_updates()).unwrap();
    b.doc().import(&a_bytes).unwrap();
    a.doc().import(&b_bytes).unwrap();

    let a_list = a
        .list(
            Page {
                index: 0,
                size: 100,
            },
            None,
            None,
        )
        .await
        .unwrap();
    let b_list = b
        .list(
            Page {
                index: 0,
                size: 100,
            },
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(a_list.total, 2);
    assert_eq!(b_list.total, 2);

    let mut a_ids: Vec<_> = a_list.items.iter().map(|e| e.id).collect();
    let mut b_ids: Vec<_> = b_list.items.iter().map(|e| e.id).collect();
    a_ids.sort();
    b_ids.sort();
    assert_eq!(a_ids, b_ids);
    assert!(a_ids.contains(&alpha.id));
    assert!(a_ids.contains(&beta.id));
}

// Snapshot persistence round-trip: write data, export a full
// snapshot (the bytes you'd store in `loro_doc.snapshot`), drop the
// repo, build a fresh LoroDoc, import the bytes, and read everything
// back. This is the contract `features/crdt/crdt-seaorm`'s `Persistence`
// trait has to honor: bytes in, bytes out, identical state.
#[tokio::test]
async fn crdt_snapshot_round_trip() {
    use loro::{ExportMode, LoroDoc};

    let original = crdt_repo();
    let a = original
        .create(ExampleCreate {
            name: "first".into(),
            description: "alpha".into(),
        })
        .await
        .unwrap();
    let b = original
        .create(ExampleCreate {
            name: "second".into(),
            description: "beta".into(),
        })
        .await
        .unwrap();
    original
        .update(
            a.id,
            ExampleUpdate {
                name: Some("first-renamed".into()),
                description: None,
            },
        )
        .await
        .unwrap();

    // What persistence would write.
    let snapshot_bytes = original.doc().export(ExportMode::Snapshot).unwrap();
    drop(original);

    // What persistence would read on cold start.
    let restored_doc = LoroDoc::new();
    restored_doc.import(&snapshot_bytes).unwrap();
    let restored =
        example_crdt::ExampleRepoLoro::new(&example_crdt::CrdtDoc::from_loro(restored_doc));

    let got_a = restored.get(a.id).await.unwrap();
    assert_eq!(got_a.name, "first-renamed");
    assert_eq!(got_a.description, "alpha");

    let got_b = restored.get(b.id).await.unwrap();
    assert_eq!(got_b.name, "second");
    assert_eq!(got_b.description, "beta");

    let page = restored
        .list(
            Page {
                index: 0,
                size: 100,
            },
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.total, 2);
}

// Update-log persistence round-trip: instead of storing one snapshot,
// store the incremental update bytes emitted on every commit and
// replay them on cold start. This is the second persistence shape
// `features/crdt/crdt-seaorm` will support — append-mostly with periodic
// compaction. Same outcome: bytes in, bytes out, identical state.
#[tokio::test]
async fn crdt_update_log_round_trip() {
    use loro::{ExportMode, LoroDoc};
    use std::sync::{Arc, Mutex};

    let original = crdt_repo();

    // Capture every local update the doc emits. In production this
    // closure pushes into a SeaORM `loro_update` table.
    let log: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let log_for_sub = log.clone();
    let _sub = original
        .doc()
        .subscribe_local_update(Box::new(move |bytes| {
            log_for_sub.lock().unwrap().push(bytes.to_vec());
            true
        }));

    let row = original
        .create(ExampleCreate {
            name: "logged".into(),
            description: "via update stream".into(),
        })
        .await
        .unwrap();
    original
        .update(
            row.id,
            ExampleUpdate {
                name: Some("logged-v2".into()),
                description: None,
            },
        )
        .await
        .unwrap();

    let updates = log.lock().unwrap().clone();
    assert!(!updates.is_empty(), "expected at least one update");
    drop(original);

    // Replay onto a fresh doc — equivalent to `importBatch`.
    let restored_doc = LoroDoc::new();
    for bytes in &updates {
        restored_doc.import(bytes).unwrap();
    }
    let restored =
        example_crdt::ExampleRepoLoro::new(&example_crdt::CrdtDoc::from_loro(restored_doc));

    let got = restored.get(row.id).await.unwrap();
    assert_eq!(got.name, "logged-v2");
    assert_eq!(got.description, "via update stream");

    // And a compaction-style round-trip: snapshot the replayed doc,
    // import on a third doc, same result. This is what periodic
    // compaction does — turn N updates into one snapshot.
    let compacted_bytes = restored.doc().export(ExportMode::Snapshot).unwrap();
    let recompact_doc = LoroDoc::new();
    recompact_doc.import(&compacted_bytes).unwrap();
    let recompact =
        example_crdt::ExampleRepoLoro::new(&example_crdt::CrdtDoc::from_loro(recompact_doc));
    let recompacted = recompact.get(row.id).await.unwrap();
    assert_eq!(recompacted.name, "logged-v2");
}

// Full `CrdtDoc::open` lifecycle against a real Persistence impl
// (the in-memory one shipped with features/crdt/crdt). Proves the path the
// SeaORM persistence will run in production: open, write, compact
// to flush a snapshot, drop, re-open against the same persistence,
// observe identical state.
#[tokio::test]
async fn crdt_open_compact_reopen() {
    use crdt::in_memory::InMemoryPersistence;
    use example_crdt::{CrdtDoc, ExampleRepoLoro};
    use uuid::Uuid;

    let persistence = InMemoryPersistence::new();
    let doc_id = Uuid::new_v4();

    // First session — write, compact (forces snapshot through the
    // persistence trait, synchronous path).
    let row_id = {
        let cdoc = CrdtDoc::open(doc_id, persistence.clone()).await.unwrap();
        let repo = ExampleRepoLoro::new(&cdoc);
        let row = repo
            .create(ExampleCreate {
                name: "persistent".into(),
                description: "across sessions".into(),
            })
            .await
            .unwrap();
        cdoc.compact(doc_id).await.unwrap();
        row.id
    };

    // Second session — open against the same persistence; state
    // should be loaded from the snapshot.
    let cdoc = CrdtDoc::open(doc_id, persistence).await.unwrap();
    let repo = ExampleRepoLoro::new(&cdoc);
    let got = repo.get(row_id).await.unwrap();
    assert_eq!(got.name, "persistent");
    assert_eq!(got.description, "across sessions");
}

// r[verify repo.list.sort.name]
#[tokio::test]
async fn external_backend_seed_data_sorts() {
    let r = example_stub_backend::StubBackend::with_seed_data();
    let page = r
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
        .unwrap();
    let names: Vec<_> = page.items.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["stub.alpha", "stub.bravo", "stub.charlie"]);
}

// The `#[derive(Entity)]` repo participates in the Layer system: the
// emitted `ExampleRepoLayer` token makes every backend composable +
// swappable through one generic call site, no consumer change.
#[test]
fn repos_compose_as_layers_for_any_backend() {
    use architect::{Layer, Services};
    use example_stub_backend::StubBackend;

    // Identical for every backend: build the router and read the
    // bundle's advertised surface.
    fn surface<B>(backend: B) -> Vec<&'static str>
    where
        B: ExampleRepo + Services + Clone + Send + Sync + 'static,
    {
        let _router = backend.into_router();
        B::layers()
            .descriptors()
            .iter()
            .map(|d| d.service_name)
            .collect()
    }

    assert_eq!(surface(ExampleRepoMemory::new()), vec!["ExampleRepo"]);
    assert_eq!(
        surface(StubBackend::with_seed_data()),
        vec!["ExampleRepo"],
        "a third-party backend mounts identically to the in-tree one"
    );
}
