//! A song is one file: the engine's project and the song's other
//! documents (the session's timing, the chart, the cues) as parts of one
//! store, its id in `meta`, whole once closed — opened from wherever it
//! is copied to, by the store and by the byte reader alike.

use std::sync::{Arc, Mutex};

use crdt::Persistence as _;
use loro::{ExportMode, LoroDoc, LoroValue};
use session_format::read::ProjectFile;
use session_format::store::{MAIN, PartReplica, SESSION, Store, StoreError};
use uuid::Uuid;

/// One commit putting `key` = `value` in `doc`'s `tempo` map; its update.
fn put(doc: &LoroDoc, key: &str, value: f64) -> Vec<u8> {
    let before = doc.oplog_vv();
    doc.get_map("tempo").insert(key, value).expect("an insert");
    doc.commit();
    doc.export(ExportMode::updates(&before)).expect("an export")
}

fn tempo(doc: &LoroDoc) -> LoroValue {
    doc.get_map("tempo").get_deep_value()
}

#[test]
fn the_song_keeps_one_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("Song.session")).unwrap();
    assert_eq!(store.song_id().unwrap(), None);
    let id = Uuid::new_v4();
    store.set_song_id(id).unwrap();
    store.set_song_id(id).unwrap();
    assert_eq!(store.ensure_song_id().unwrap(), id);
    assert!(matches!(
        store.set_song_id(Uuid::new_v4()),
        Err(StoreError::IdTaken { .. })
    ));
    // The session's own doc on `main` syncs under the song's id.
    assert_eq!(store.part_id(SESSION, MAIN).unwrap(), id);
    assert_eq!(store.part_of(id).unwrap().unwrap().part, SESSION);
}

#[test]
fn a_part_round_trips_through_updates_and_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Song.session");
    let doc = LoroDoc::new();
    {
        let store = Store::open(&path).unwrap();
        assert!(!store.has_part(SESSION, MAIN).unwrap());
        store
            .append_part_update(SESSION, MAIN, &put(&doc, "bpm", 120.0))
            .unwrap();
        store
            .append_part_update(SESSION, MAIN, &put(&doc, "bpm", 96.0))
            .unwrap();
        assert_eq!(tempo(&store.load_part(SESSION, MAIN).unwrap()), tempo(&doc));
        let loaded = store.load_part(SESSION, MAIN).unwrap();
        store.compact_part(SESSION, MAIN, &loaded).unwrap();
        store
            .append_part_update(SESSION, MAIN, &put(&doc, "beats", 3.0))
            .unwrap();
        store.close().unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(tempo(&store.load_part(SESSION, MAIN).unwrap()), tempo(&doc));
    assert_eq!(store.parts().unwrap().len(), 1);
    // The engine's project is untouched by its parts.
    assert_eq!(
        store.load(MAIN).unwrap().get_deep_value(),
        LoroDoc::new().get_deep_value()
    );
}

#[test]
fn a_peer_is_answered_with_what_it_lacks() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("Song.session")).unwrap();
    let doc = LoroDoc::new();
    store
        .append_part_update(SESSION, MAIN, &put(&doc, "bpm", 120.0))
        .unwrap();
    // A peer with nothing gets everything.
    let peer = LoroDoc::new();
    peer.import(&store.part_updates_since(SESSION, MAIN, &[]).unwrap())
        .unwrap();
    assert_eq!(tempo(&peer), tempo(&doc));
    store
        .append_part_update(SESSION, MAIN, &put(&doc, "bpm", 100.0))
        .unwrap();
    // A peer that says what it has gets only the rest.
    let rest = store
        .part_updates_since(SESSION, MAIN, &peer.oplog_vv().encode())
        .unwrap();
    peer.import(&rest).unwrap();
    assert_eq!(tempo(&peer), tempo(&doc));
}

#[test]
fn an_arrangement_is_a_branch_with_its_own_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("Song.session")).unwrap();
    let doc = LoroDoc::new();
    store
        .append_part_update(SESSION, MAIN, &put(&doc, "bpm", 120.0))
        .unwrap();
    let project = store.load(MAIN).unwrap();
    store.fork("Acoustic", MAIN, &project).unwrap();
    // The branch starts as the song was, and goes its own way.
    let acoustic = store.load_part(SESSION, "Acoustic").unwrap();
    assert_eq!(tempo(&acoustic), tempo(&doc));
    store
        .append_part_update(SESSION, "Acoustic", &put(&acoustic, "bpm", 80.0))
        .unwrap();
    assert_eq!(
        tempo(&store.load_part(SESSION, MAIN).unwrap()),
        tempo(&doc),
        "the song is not the arrangement"
    );
    // Its own doc id, so it syncs on its own.
    let id = store.part_id(SESSION, "Acoustic").unwrap();
    assert_ne!(id, store.part_id(SESSION, MAIN).unwrap());
    assert_eq!(store.part_of(id).unwrap().unwrap().branch, "Acoustic");
    store.delete_branch("Acoustic").unwrap();
    assert!(!store.has_part(SESSION, "Acoustic").unwrap());
}

#[tokio::test]
async fn the_parts_are_a_sync_replica() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        Store::open(&dir.path().join("Song.session")).unwrap(),
    ));
    let id = store.lock().unwrap().ensure_song_id().unwrap();
    let replica = PartReplica::new(store.clone());
    {
        assert_eq!(replica.load_snapshot(id).await.unwrap(), None);
        let doc = LoroDoc::new();
        replica
            .append_update(id, &put(&doc, "bpm", 120.0))
            .await
            .unwrap();
        let snapshot = replica.load_snapshot(id).await.unwrap().unwrap();
        let back = LoroDoc::new();
        back.import(&snapshot).unwrap();
        assert_eq!(tempo(&back), tempo(&doc));
        put(&doc, "bpm", 90.0);
        replica
            .compact(id, &doc.export(ExportMode::Snapshot).unwrap())
            .await
            .unwrap();
        let snapshot = replica.load_snapshot(id).await.unwrap().unwrap();
        let back = LoroDoc::new();
        back.import(&snapshot).unwrap();
        assert_eq!(tempo(&back), tempo(&doc));
        // A doc that is not this song's is refused, not made.
        assert!(replica.append_update(Uuid::new_v4(), b"x").await.is_err());
    }
}

#[test]
fn the_closed_file_alone_opens_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Song.session");
    let doc = LoroDoc::new();
    let id = Uuid::new_v4();
    {
        let store = Store::open(&path).unwrap();
        store.set_song_id(id).unwrap();
        store
            .append_part_update(SESSION, MAIN, &put(&doc, "bpm", 120.0))
            .unwrap();
        store.put_blob("fx", b"a plugin's state").unwrap();
        store.close().unwrap();
    }
    // Only the one file goes: no log, no shared memory beside it.
    let other = tempfile::tempdir().unwrap();
    let moved = other.path().join("Moved.session");
    std::fs::copy(&path, &moved).unwrap();
    let bytes = std::fs::read(&moved).unwrap();
    assert_eq!(
        session_format::sniff(&bytes),
        session_format::FileFormat::Store
    );

    let read = ProjectFile::read(&bytes).unwrap();
    assert_eq!(read.song_id(), Some(id.to_string().as_str()));
    assert!(read.has_part(SESSION, MAIN));
    assert_eq!(tempo(&read.part(SESSION, MAIN).unwrap()), tempo(&doc));

    let store = Store::open(&moved).unwrap();
    assert_eq!(store.song_id().unwrap(), Some(id));
    assert_eq!(tempo(&store.load_part(SESSION, MAIN).unwrap()), tempo(&doc));
    assert_eq!(
        store
            .blob(&session_format::store::hash_of(b"a plugin's state"))
            .unwrap(),
        b"a plugin's state"
    );
}

#[test]
fn a_file_from_before_parts_reads_with_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Old.session");
    {
        let store = Store::open(&path).unwrap();
        store.close().unwrap();
    }
    let read = ProjectFile::read(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(read.song_id(), None);
    assert!(!read.has_part(SESSION, MAIN));
    assert_eq!(
        tempo(&read.part(SESSION, MAIN).unwrap()),
        LoroValue::Map(Default::default())
    );
}
