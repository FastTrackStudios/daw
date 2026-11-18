//! A project file is a sync replica: two devices' copies of one branch
//! edit apart, exchange what they did, converge — and what each received
//! is still in its file when it opens again (docs/design/sync.md).

use std::time::{Duration, Instant};

use crdt::CrdtDoc;
use loro::{ExportMode, LoroValue, VersionVector};
use session_format::store::{MAIN, Replica, Store};
use uuid::Uuid;

/// `doc`'s `items` map, as plain values.
fn items(doc: &loro::LoroDoc) -> LoroValue {
    doc.get_map("items").get_deep_value()
}

/// Put `key` = `value` in `doc`'s `items`, as one commit.
fn put(doc: &CrdtDoc, key: &str, value: f64) {
    doc.loro()
        .get_map("items")
        .insert(key, value)
        .expect("an insert");
    doc.loro().commit();
}

/// What `from` has that `to` has not.
fn delta(from: &CrdtDoc, to: &CrdtDoc) -> Vec<u8> {
    let since: VersionVector = to.loro().oplog_vv();
    from.loro()
        .export(ExportMode::updates(&since))
        .expect("an export")
}

/// The branch as its file holds it, once the file holds `want` — the
/// doc's writer appends in the background (and holds the file while it
/// does), so this waits for it, briefly.
fn settled(path: &std::path::Path, want: &LoroValue) -> LoroValue {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let read = Store::open(path).and_then(|store| store.load(MAIN));
        match read {
            Ok(doc) if &items(&doc) == want => return items(&doc),
            Ok(doc) if Instant::now() > deadline => return items(&doc),
            Err(e) if Instant::now() > deadline => panic!("reopen: {e}"),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_devices_edit_apart_converge_and_keep_it() {
    let dir = tempfile::tempdir().expect("a dir");
    let (a_path, b_path) = (dir.path().join("A.session"), dir.path().join("B.session"));

    // Device A makes the project; device B gets it with the same doc id.
    let a_store = Store::open(&a_path).expect("A");
    let id: Uuid = a_store.doc_id(MAIN).expect("an id");
    assert_eq!(a_store.doc_id(MAIN).expect("again"), id, "the id is kept");
    let b_store = Store::open(&b_path).expect("B");
    b_store.adopt_doc_id(MAIN, id).expect("adopted");
    assert_eq!(b_store.branch_of(id).expect("read"), Some(MAIN.to_owned()));

    let a = CrdtDoc::open(id, Replica::new(a_store))
        .await
        .expect("open A");
    let b = CrdtDoc::open(id, Replica::new(b_store))
        .await
        .expect("open B");

    // Apart: each edits with no session between them.
    put(&a, "kick", 1.0);
    put(&b, "snare", 2.0);
    put(&a, "shared", 10.0);
    put(&b, "shared", 20.0);

    // Back together: each takes the other's, durably.
    let (to_b, to_a) = (delta(&a, &b), delta(&b, &a));
    b.apply_remote_durable(id, &to_b)
        .await
        .expect("B takes A's");
    a.apply_remote_durable(id, &to_a)
        .await
        .expect("A takes B's");

    let merged = items(a.loro());
    assert_eq!(merged, items(b.loro()), "the replicas converge");
    let LoroValue::Map(map) = &merged else {
        panic!("items is a map: {merged:?}");
    };
    assert!(
        map.contains_key("kick") && map.contains_key("snare"),
        "both edits land"
    );

    // Each file holds the converged branch: its own edits and what it took.
    drop((a, b));
    assert_eq!(settled(&a_path, &merged), merged, "A's file");
    assert_eq!(settled(&b_path, &merged), merged, "B's file");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fold_keeps_the_branch_and_its_doc() {
    let dir = tempfile::tempdir().expect("a dir");
    let path = dir.path().join("Song.session");
    let store = Store::open(&path).expect("a store");
    let id = store.doc_id(MAIN).expect("an id");

    let doc = CrdtDoc::open(id, Replica::new(store)).await.expect("open");
    put(&doc, "kick", 1.0);
    put(&doc, "snare", 2.0);
    doc.compact(id).await.expect("folded");
    put(&doc, "hat", 3.0);
    let want = items(doc.loro());
    drop(doc);

    assert_eq!(
        settled(&path, &want),
        want,
        "the fold and the step after it"
    );
    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.doc_id(MAIN).expect("id"),
        id,
        "the branch keeps its doc"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_named_before_its_commit_keeps_its_name_in_the_history() {
    let dir = tempfile::tempdir().expect("a dir");
    let path = dir.path().join("Song.session");
    let store = Store::open(&path).expect("a store");
    let id = store.doc_id(MAIN).expect("an id");
    let replica = std::sync::Arc::new(Replica::new(store));
    let doc = CrdtDoc::open_shared(id, replica.clone())
        .await
        .expect("open");

    replica.name_next("Move item");
    put(&doc, "kick", 1.0);
    put(&doc, "snare", 2.0);
    let want = items(doc.loro());
    drop(doc);

    settled(&path, &want);
    let labels: Vec<String> = Store::open(&path)
        .expect("reopen")
        .steps(MAIN)
        .expect("steps")
        .into_iter()
        .map(|step| step.label)
        .collect();
    assert_eq!(
        labels,
        ["Move item", session_format::store::SYNC_LABEL],
        "the named step keeps its name; an unnamed one is a sync"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_doc_of_another_project_is_refused() {
    let dir = tempfile::tempdir().expect("a dir");
    let store = Store::open(&dir.path().join("Song.session")).expect("a store");
    let stranger = Uuid::new_v4();
    let replica = Replica::new(store);
    use crdt::Persistence as _;
    assert!(
        replica
            .load_snapshot(stranger)
            .await
            .expect("read")
            .is_none()
    );
    assert!(replica.append_update(stranger, b"x").await.is_err());
}
