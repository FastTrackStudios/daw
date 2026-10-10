//! The project file: blobs once each, the history appended and folded,
//! versions named — and all of it read back as written.

use dawfile_store::{Store, hash_of};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("a dir");
    let store = Store::open(&dir.path().join("Song.fts")).expect("a store");
    (dir, store)
}

/// A blob put twice — by two versions, two joined projects — is one
/// row, and comes back as it went in.
#[test]
fn a_blob_is_kept_once() {
    let (_dir, store) = store();
    let state = b"kit=heavy thwacky".repeat(1_000);
    let a = store.put_blob("fx", &state).expect("put");
    let b = store.put_blob("fx", &state).expect("put again");
    assert_eq!(a, b);
    assert_eq!(a, hash_of(&state));
    let (count, size, packed) = store.blob_totals().expect("totals");
    assert_eq!(count, 1, "stored once");
    assert_eq!(usize::try_from(size).ok(), Some(state.len()));
    assert!(packed < size / 10, "compressed: {packed} of {size}");
    assert_eq!(store.blob(&a).expect("read"), state);
    assert!(store.blob(&hash_of(b"nothing")).is_err(), "not here");
}

/// Undo steps appended as Loro updates load back as the doc they made;
/// compacted, the same doc, and a version named before still checks out.
#[test]
fn the_history_loads_back_compacted_or_not() {
    let (dir, store) = store();
    let doc = loro::LoroDoc::new();
    let items = doc.get_map("items");
    let mut from = doc.oplog_vv();
    for (step, (guid, at)) in [("k1", 2.0), ("k2", 4.0), ("k3", 6.0)].iter().enumerate() {
        let item = items
            .insert_container(guid, loro::LoroMap::new())
            .expect("an item");
        item.insert("position", *at).expect("a position");
        doc.commit();
        let update = doc
            .export(loro::ExportMode::updates(&from))
            .expect("an update");
        store
            .append_update(&format!("Add item {step}"), &update)
            .expect("appended");
        from = doc.oplog_vv();
        if step == 1 {
            store.name_version("Two items", &doc).expect("named");
        }
    }
    let back = store.load().expect("loaded");
    assert_eq!(back.get_deep_value(), doc.get_deep_value());

    store.compact(&doc).expect("compacted");
    drop(store);
    let reopened = Store::open(&dir.path().join("Song.fts")).expect("reopened");
    let back = reopened.load().expect("loaded");
    assert_eq!(
        back.get_deep_value(),
        doc.get_deep_value(),
        "after compaction"
    );

    let versions = reopened.versions().expect("versions");
    assert_eq!(versions.len(), 1);
    let frontiers = loro::Frontiers::decode(&versions[0].frontiers).expect("frontiers");
    back.checkout(&frontiers).expect("checked out");
    let then = back.get_map("items");
    assert_eq!(then.len(), 2, "the version had two items");
}

/// A file from another schema is refused, not misread.
#[test]
fn another_schema_is_refused() {
    let dir = tempfile::tempdir().expect("a dir");
    let path = dir.path().join("Song.fts");
    Store::open(&path)
        .expect("a store")
        .set_meta("schema", "99")
        .expect("set");
    assert!(Store::open(&path).is_err());
}
