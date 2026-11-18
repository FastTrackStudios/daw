//! The project file: blobs once each, each branch's history appended and
//! folded, alternatives forked — and all of it read back as written.

use session_format::store::{MAIN, MomentKind, MomentRef, Store, hash_of};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("a dir");
    let store = Store::open(&dir.path().join("Song.session")).expect("a store");
    (dir, store)
}

/// One step of edits on `doc`, appended to `branch`: an item at `at`.
fn step(store: &Store, branch: &str, doc: &loro::LoroDoc, guid: &str, at: f64) {
    let from = doc.oplog_vv();
    let item = doc
        .get_map("items")
        .insert_container(guid, loro::LoroMap::new())
        .expect("an item");
    item.insert("position", at).expect("a position");
    doc.commit();
    let update = doc
        .export(loro::ExportMode::updates(&from))
        .expect("an update");
    store
        .append_update(branch, &format!("Add {guid}"), &update)
        .expect("appended");
}

/// A blob put twice — by two branches, two joined projects — is one row,
/// and comes back as it went in.
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

/// The song's undo steps load back as the doc they made, compacted or
/// not, from the file reopened.
#[test]
fn the_song_loads_back_compacted_or_not() {
    let (dir, store) = store();
    let doc = loro::LoroDoc::new();
    for (i, guid) in ["k1", "k2", "k3"].iter().enumerate() {
        step(
            &store,
            MAIN,
            &doc,
            guid,
            f64::from(u8::try_from(i).unwrap_or(0)),
        );
    }
    assert_eq!(
        store.load(MAIN).expect("loaded").get_deep_value(),
        doc.get_deep_value()
    );
    let names = |store: &Store| -> Vec<String> {
        store
            .steps(MAIN)
            .expect("the steps")
            .into_iter()
            .map(|s| s.label)
            .collect()
    };
    assert_eq!(names(&store), ["Add k1", "Add k2", "Add k3"]);
    store.compact(MAIN, &doc).expect("compacted");
    assert!(names(&store).is_empty(), "folded into the snapshot");
    step(&store, MAIN, &doc, "k4", 9.0);
    assert_eq!(names(&store), ["Add k4"]);
    drop(store);
    let reopened = Store::open(&dir.path().join("Song.session")).expect("reopened");
    assert_eq!(
        reopened.load(MAIN).expect("loaded").get_deep_value(),
        doc.get_deep_value(),
        "a snapshot and an update after it"
    );
}

/// An alternative forked from the song keeps what the song had then, its
/// own edits after, and none of the song's later ones — and the song
/// none of its.
#[test]
fn an_alternative_is_the_song_forked_and_edited_on_its_own() {
    let (_dir, store) = store();
    let song = loro::LoroDoc::new();
    step(&store, MAIN, &song, "k1", 1.0);
    store.fork("Organized", MAIN, &song).expect("forked");
    step(&store, MAIN, &song, "k2", 2.0);

    let organized = store.load("Organized").expect("the alternative");
    assert_eq!(organized.get_map("items").len(), 1, "the song as it was");
    step(&store, "Organized", &organized, "o1", 5.0);

    let organized = store.load("Organized").expect("again");
    let items = organized.get_map("items");
    assert!(
        items.get("k1").is_some() && items.get("o1").is_some(),
        "its own edit"
    );
    assert!(items.get("k2").is_none(), "not the song's later one");
    let song_back = store.load(MAIN).expect("the song");
    assert!(
        song_back.get_map("items").get("o1").is_none(),
        "the song has none of its"
    );
    let names: Vec<String> = store
        .branches()
        .expect("branches")
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, vec![MAIN.to_owned(), "Organized".to_owned()]);
}

/// One step on `doc`, appended to the song: an item for each of `ids`,
/// at its number plus `nudge` — many hits in one undo step, as a quantize
/// writes them.
fn hits(store: &Store, doc: &loro::LoroDoc, (ids, nudge): (std::ops::Range<u32>, f64)) {
    let from = doc.oplog_vv();
    let items = doc.get_map("items");
    for i in ids {
        let item = items
            .insert_container(&format!("hit{i}"), loro::LoroMap::new())
            .expect("an item");
        item.insert("position", f64::from(i) + nudge)
            .expect("a position");
    }
    doc.commit();
    let update = doc
        .export(loro::ExportMode::updates(&from))
        .expect("an update");
    store
        .append_update(MAIN, "Quantize", &update)
        .expect("appended");
}

/// A checkpoint (a branch forked before a drum rewrite) costs a row, not a
/// copy of the song: forking a long song grows its file by next to
/// nothing. And it outlives what happens to the song after — more edits,
/// the song compacted (a Loro snapshot keeps every operation, so the fork
/// point is still there) — opening as the song was when it was forked.
#[test]
fn a_checkpoint_shares_the_song_and_outlives_its_compaction() {
    let (dir, store) = store();
    let file = dir.path().join("Song.session");
    let song = loro::LoroDoc::new();
    // A long song: thousands of hits, in a few steps.
    for take in 0..4_u32 {
        hits(&store, &song, (take * 1_000..(take + 1) * 1_000, 0.0));
    }
    store.compact(MAIN, &song).expect("compacted");
    store.checkpoint().expect("one file");
    let before = std::fs::metadata(&file).expect("the file").len();
    let as_it_was = song.get_deep_value();

    store
        .fork("Before DRUMS_REQUANTIZE 2026-10-31 14:05", MAIN, &song)
        .expect("forked");
    store.checkpoint().expect("one file");
    let grown = std::fs::metadata(&file)
        .expect("the file")
        .len()
        .saturating_sub(before);
    assert!(
        grown <= 16 * 1024,
        "a fork is a row, not a copy: the file grew {grown} bytes on {before}"
    );

    // The rewrite, and the song compacted after it.
    hits(&store, &song, (0..1_000, 0.01));
    store.compact(MAIN, &song).expect("compacted again");
    drop(store);
    let reopened = Store::open(&file).expect("reopened");
    assert_eq!(
        reopened
            .load("Before DRUMS_REQUANTIZE 2026-10-31 14:05")
            .expect("the checkpoint opens")
            .get_deep_value(),
        as_it_was
    );
}

/// Every step is noted in the file's history with the doc's frontier
/// after it — two named alike stay two — and that outlives a compaction
/// and a reopen: each still opens the version it was, the song's history
/// being whole in its snapshot.
#[test]
fn a_noted_step_outlives_compaction_and_opens_as_it_was() {
    let (dir, store) = store();
    let doc = loro::LoroDoc::new();
    let mut noted = Vec::new();
    for (guid, at) in [("k1", 1.0), ("k2", 2.0)] {
        step(&store, MAIN, &doc, guid, at);
        let seq = store
            .note(
                MAIN,
                (MomentKind::Step, "Move", Some("ARRANGE_MOVE")),
                &doc.state_frontiers(),
            )
            .expect("noted");
        noted.push((seq, doc.get_deep_value()));
    }
    store
        .note(
            MAIN,
            (
                MomentKind::Mark,
                "Before QUANTIZE 2026-10-31 14:05",
                Some("QUANTIZE"),
            ),
            &doc.state_frontiers(),
        )
        .expect("marked");
    store.compact(MAIN, &doc).expect("compacted");
    drop(store);

    let reopened = Store::open(&dir.path().join("Song.session")).expect("reopened");
    let (history, total) = reopened.history(MAIN, (0, 10)).expect("its history");
    assert_eq!(total, 3);
    let labels: Vec<(&str, MomentKind)> =
        history.iter().map(|m| (m.label.as_str(), m.kind)).collect();
    assert_eq!(
        labels,
        [
            ("Before QUANTIZE 2026-10-31 14:05", MomentKind::Mark),
            ("Move", MomentKind::Step),
            ("Move", MomentKind::Step),
        ]
    );
    let song = reopened.load(MAIN).expect("the song");
    for (seq, value) in noted {
        let moment = reopened
            .moment(MAIN, &MomentRef::Seq(seq))
            .expect("read")
            .expect("there");
        let at = loro::Frontiers::decode(&moment.frontiers).expect("a frontier");
        assert_eq!(song.fork_at(&at).expect("forked").get_deep_value(), value);
    }
    assert!(
        reopened
            .moment(
                MAIN,
                &MomentRef::Mark("Before QUANTIZE 2026-10-31 14:05".to_owned())
            )
            .expect("read")
            .is_some()
    );
}

/// A file from another schema is refused, not misread.
#[test]
fn another_schema_is_refused() {
    let dir = tempfile::tempdir().expect("a dir");
    let path = dir.path().join("Song.session");
    Store::open(&path)
        .expect("a store")
        .set_meta("schema", "99")
        .expect("set");
    assert!(Store::open(&path).is_err());
}

/// After a checkpoint the project is one file: nothing left in a log
/// beside it.
#[test]
fn a_checkpointed_project_is_one_file() {
    let (dir, store) = store();
    let doc = loro::LoroDoc::new();
    step(&store, MAIN, &doc, "k1", 1.0);
    store
        .put_blob("fx", &b"state".repeat(10_000))
        .expect("a blob");
    store.checkpoint().expect("checkpointed");
    let wal = dir.path().join("Song.session-wal");
    let left = std::fs::metadata(&wal).map_or(0, |m| m.len());
    assert_eq!(left, 0, "the log is empty");
    assert!(
        std::fs::metadata(dir.path().join("Song.session")).map_or(0, |m| m.len()) > 4096,
        "the file holds it"
    );
}

/// A fold is taken with the song held and packed and written after; two in
/// flight at once may land in either order, and an older one written last
/// never puts the song back to what it held — nor loses an edit appended
/// while either was being packed.
#[test]
fn a_fold_written_after_a_later_one_changes_nothing() {
    let (dir, store) = store();
    let doc = loro::LoroDoc::new();
    step(&store, MAIN, &doc, "k1", 0.0);
    let older = store.take_snapshot(MAIN, &doc).expect("taken");
    step(&store, MAIN, &doc, "k2", 1.0);
    let newer = store.take_snapshot(MAIN, &doc).expect("taken");
    // Appended while both are being packed.
    step(&store, MAIN, &doc, "k3", 2.0);
    store
        .write_snapshot(newer.pack().expect("packed"))
        .expect("written");
    store
        .write_snapshot(older.pack().expect("packed"))
        .expect("written");
    drop(store);
    let reopened = Store::open(&dir.path().join("Song.session")).expect("reopened");
    assert_eq!(
        reopened.load(MAIN).expect("loaded").get_deep_value(),
        doc.get_deep_value()
    );
}
