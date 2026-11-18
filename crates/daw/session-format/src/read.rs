//! A project file (`Song.session`) read from its bytes, without Turso —
//! where there is no disk and no C to build zstd from: a browser opening
//! a song it was handed. The same answers as [`crate::store::Store`]'s
//! reads (`load`, `blob`) over the same file, through a pure-Rust reader
//! of the file's pages ([`sqlite`]) and zstd decoder.

mod sqlite;

use std::collections::HashMap;

pub use sqlite::SqliteError;

/// The branch every project has: the song itself.
pub const MAIN: &str = "main";

/// The schema this reader understands (`meta.schema`).
const SCHEMA: i64 = 1;

/// Why a project file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("{0}")]
    File(#[from] SqliteError),
    #[error("schema {found} is not one this build reads")]
    Schema { found: i64 },
    #[error("no branch {0}")]
    NoBranch(String),
    #[error("a {0} row is missing a column")]
    Column(&'static str),
    #[error("decompressing: {0}")]
    Compression(String),
    #[error("loro: {0}")]
    Loro(String),
    #[error("no blob {0}")]
    Missing(String),
    #[error("blob {0} is not what its name says")]
    Corrupt(String),
}

type Result<T> = std::result::Result<T, ReadError>;

/// A branch as the file records it: what it forked from, and where.
struct Branch {
    parent: Option<String>,
    /// The parent's frontiers at the fork, encoded.
    base: Option<Vec<u8>>,
}

/// A project file, read whole into memory: its branches, snapshots and
/// history as packed, its blobs packed until asked for.
pub struct ProjectFile {
    branches: HashMap<String, Branch>,
    /// Branch → (the last update the snapshot covers, the snapshot packed).
    snapshots: HashMap<String, (i64, Vec<u8>)>,
    /// (seq, branch, update packed), in seq order.
    updates: Vec<(i64, String, Vec<u8>)>,
    blobs: HashMap<String, Vec<u8>>,
}

impl ProjectFile {
    /// Read the file in `bytes`.
    ///
    /// # Errors
    /// Not a project file, one of a schema this build does not read, or
    /// malformed.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let db = sqlite::Db::new(bytes)?;
        let schema = db
            .rows("meta")?
            .into_iter()
            .find(|row| row.get(0).text() == Some("schema"))
            .and_then(|row| row.get(1).text().and_then(|v| v.parse().ok()));
        if let Some(found) = schema
            && found != SCHEMA
        {
            return Err(ReadError::Schema { found });
        }
        let branches = db
            .rows("branches")?
            .into_iter()
            .map(|mut row| {
                let name = text(row.take(0)).ok_or(ReadError::Column("branches"))?;
                let parent = text(row.take(1));
                let base = row.take(2).into_bytes();
                Ok((name, Branch { parent, base }))
            })
            .collect::<Result<_>>()?;
        let snapshots = db
            .rows("snapshots")?
            .into_iter()
            .map(|mut row| {
                let branch = text(row.take(0)).ok_or(ReadError::Column("snapshots"))?;
                let upto = row.get(1).int().ok_or(ReadError::Column("snapshots"))?;
                let data = row
                    .take(3)
                    .into_bytes()
                    .ok_or(ReadError::Column("snapshots"))?;
                Ok((branch, (upto, data)))
            })
            .collect::<Result<_>>()?;
        // `seq` is the rowid: stored as NULL in the record.
        let updates = db
            .rows("updates")?
            .into_iter()
            .map(|mut row| {
                let branch = text(row.take(1)).ok_or(ReadError::Column("updates"))?;
                let data = row
                    .take(4)
                    .into_bytes()
                    .ok_or(ReadError::Column("updates"))?;
                Ok((row.rowid, branch, data))
            })
            .collect::<Result<_>>()?;
        let blobs = db
            .rows("blobs")?
            .into_iter()
            .map(|mut row| {
                let hash = text(row.take(0)).ok_or(ReadError::Column("blobs"))?;
                let data = row.take(3).into_bytes().ok_or(ReadError::Column("blobs"))?;
                Ok((hash, data))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            branches,
            snapshots,
            updates,
            blobs,
        })
    }

    /// `branch`'s doc: its snapshot if it has one, else its parent forked
    /// where it forked; then every update of its own after that — as
    /// [`crate::store::Store::load`] has it.
    ///
    /// # Errors
    /// No such branch, or its history would not decompress or import.
    pub fn doc(&self, branch: &str) -> Result<loro::LoroDoc> {
        self.doc_at_depth(branch, 0)
    }

    fn doc_at_depth(&self, branch: &str, depth: usize) -> Result<loro::LoroDoc> {
        let found = self
            .branches
            .get(branch)
            .ok_or_else(|| ReadError::NoBranch(branch.to_owned()))?;
        let (doc, upto) = match (self.snapshots.get(branch), &found.parent, &found.base) {
            (Some((upto, packed)), _, _) => {
                let doc = loro::LoroDoc::new();
                doc.import(&unpack(packed)?)
                    .map_err(|e| ReadError::Loro(e.to_string()))?;
                (doc, *upto)
            }
            // A branch is never its own ancestor; a file that says so is
            // broken, not a reason to recurse forever.
            (None, Some(parent), Some(base)) if depth < self.branches.len() => {
                let from = self.doc_at_depth(parent, depth + 1)?;
                let at =
                    loro::Frontiers::decode(base).map_err(|e| ReadError::Loro(e.to_string()))?;
                let forked = from
                    .fork_at(&at)
                    .map_err(|e| ReadError::Loro(e.to_string()))?;
                (forked, 0)
            }
            _ => (loro::LoroDoc::new(), 0),
        };
        let updates = self
            .updates
            .iter()
            .filter(|(seq, name, _)| name == branch && *seq > upto)
            .map(|(_, _, packed)| unpack(packed))
            .collect::<Result<Vec<_>>>()?;
        // All at once, as the store does: one import works the state out
        // once.
        if !updates.is_empty() {
            doc.import_batch(&updates)
                .map_err(|e| ReadError::Loro(e.to_string()))?;
        }
        Ok(doc)
    }

    /// The blob named `hash`, checked against its name.
    ///
    /// # Errors
    /// It is not here, would not decompress, or is not what its name says.
    pub fn blob(&self, hash: &str) -> Result<Vec<u8>> {
        let packed = self
            .blobs
            .get(hash)
            .ok_or_else(|| ReadError::Missing(hash.to_owned()))?;
        let bytes = unpack(packed)?;
        if hash_of(&bytes) != hash {
            return Err(ReadError::Corrupt(hash.to_owned()));
        }
        Ok(bytes)
    }

    /// Every blob's name.
    pub fn blob_names(&self) -> impl Iterator<Item = &str> {
        self.blobs.keys().map(String::as_str)
    }
}

fn text(value: sqlite::Value) -> Option<String> {
    match value {
        sqlite::Value::Text(text) => Some(text),
        _ => None,
    }
}

/// A blob's name: its SHA-256, lowercase hex — the store's.
fn hash_of(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Bytes the store packed with zstd, unpacked.
fn unpack(packed: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(packed)
        .map_err(|e| ReadError::Compression(e.to_string()))?;
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| ReadError::Compression(e.to_string()))?;
    Ok(out)
}

#[cfg(all(test, feature = "store"))]
mod tests {
    use super::*;
    use crate::store::Store;

    /// A doc holding `n` in its map under `key`.
    fn set(doc: &loro::LoroDoc, key: &str, n: i64) -> Vec<u8> {
        let before = doc.oplog_vv();
        doc.get_map("song").insert(key, n).unwrap();
        doc.commit();
        doc.export(loro::ExportMode::updates(&before)).unwrap()
    }

    /// The file a store wrote, as its bytes, once its log is folded in.
    fn written(path: &std::path::Path, store: Store) -> Vec<u8> {
        store.checkpoint().unwrap();
        drop(store);
        std::fs::read(path).unwrap()
    }

    #[test]
    fn reads_what_the_store_loads_snapshot_history_fork_and_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Song.session");
        let store = Store::open(&path).unwrap();
        let doc = loro::LoroDoc::new();
        // History, then a snapshot of it, then more history after it.
        for i in 0..40 {
            store
                .append_update(MAIN, "step", &set(&doc, &format!("k{i}"), i))
                .unwrap();
        }
        store.compact(MAIN, &doc).unwrap();
        store.fork("Organized", MAIN, &doc).unwrap();
        // Enough rows that the table needs interior pages.
        for i in 0..400 {
            store
                .append_update(MAIN, "later", &set(&doc, "late", i))
                .unwrap();
        }
        let fork = store.load("Organized").unwrap();
        store
            .append_update("Organized", "own", &set(&fork, "own", 7))
            .unwrap();
        // A blob far bigger than a page: an overflow chain.
        let big: Vec<u8> = (0..300_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let small = b"a plugin's state".to_vec();
        let big_hash = store.put_blob("state", &big).unwrap();
        let small_hash = store.put_blob("state", &small).unwrap();
        let expect_main = store.load(MAIN).unwrap().get_deep_value();
        let expect_fork = store.load("Organized").unwrap().get_deep_value();

        let file = ProjectFile::read(&written(&path, store)).unwrap();
        assert_eq!(file.doc(MAIN).unwrap().get_deep_value(), expect_main);
        assert_eq!(file.doc("Organized").unwrap().get_deep_value(), expect_fork);
        assert_eq!(file.blob(&big_hash).unwrap(), big);
        assert_eq!(file.blob(&small_hash).unwrap(), small);
        assert_eq!(file.blob_names().count(), 2);
        assert!(matches!(file.doc("Nope"), Err(ReadError::NoBranch(_))));
        assert!(matches!(
            file.blob("sha256-none"),
            Err(ReadError::Missing(_))
        ));
    }

    #[test]
    fn a_new_file_reads_as_an_empty_song() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Empty.session");
        let store = Store::open(&path).unwrap();
        let file = ProjectFile::read(&written(&path, store)).unwrap();
        assert!(file.doc(MAIN).unwrap().get_map("song").is_empty());
    }
}
