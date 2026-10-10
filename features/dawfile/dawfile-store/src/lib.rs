//! One file per project, on Turso.
//!
//! The project is a Loro document — what everyone editing it, on any
//! device, merges — and this is where it lives on disk: one SQLite-format
//! file, written through [Turso](https://github.com/tursodatabase/turso),
//! holding
//!
//! ```text
//! meta       key → value              the schema, the project's id
//! blobs      hash → bytes (zstd)      plugin states, MIDI sources, an
//!                                     imported project's verbatim text —
//!                                     once each, however many versions
//!                                     or joined projects refer to them
//! updates    seq → Loro update        each undo step's edits, appended
//!                                     (zstd); folded into a snapshot by
//!                                     [`Store::compact`]
//! snapshots  upto → Loro snapshot     the doc as of update `upto`
//! versions   name → frontiers         named points in the history: a
//!                                     song's alternatives and checkpoints
//! ```
//!
//! Media is not here: audio stays files beside the project, as it is now.
//!
//! What makes it small is what goes in, not the engine: a project as a
//! Loro doc rather than text, every blob stored once by content, and a
//! checkpoint a name for a point in one history rather than a copy of the
//! project. Turso itself does not compress; the blobs and the history are
//! zstd'd here.
//!
//! Turso's API is async; the store drives it to completion on the
//! calling thread, so callers (a save, an open) stay synchronous.

use sha2::{Digest, Sha256};

/// The schema this build writes, kept in `meta.schema`.
pub const SCHEMA: i64 = 1;

/// The zstd level blobs and history are compressed at: well past the
/// default's ratio, still fast to write for a save.
const LEVEL: i32 = 9;

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] turso::Error),
    #[error("compression: {0}")]
    Codec(#[from] std::io::Error),
    #[error("loro: {0}")]
    Loro(String),
    #[error("blob {0} is not in the store")]
    Missing(String),
    #[error("blob {0} does not hash to its name")]
    Corrupt(String),
    #[error("the store is schema {found}, this build reads {SCHEMA}")]
    Schema { found: i64 },
}

/// The store's results.
pub type StoreResult<T> = Result<T, StoreError>;

/// A blob's name: its SHA-256, lowercase hex — the same names the
/// `.session` folder's `objects/` use, so a project moves between them
/// without renaming anything.
#[must_use]
pub fn hash_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// A named point in the project's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub name: String,
    /// The Loro frontiers, encoded.
    pub frontiers: Vec<u8>,
    /// Seconds since the epoch it was named at.
    pub at: i64,
}

/// One project's file.
pub struct Store {
    conn: turso::Connection,
    /// Kept so the file stays open as long as the store.
    _db: turso::Database,
}

/// Run `future` to completion here.
fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    pollster::block_on(future)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Store {
    /// Open the project file at `path`, made if it is not there.
    ///
    /// # Errors
    ///
    /// The file would not open, or it is a schema this build does not
    /// read.
    pub fn open(path: &std::path::Path) -> StoreResult<Self> {
        run(async {
            let db = turso::Builder::new_local(&path.to_string_lossy())
                .build()
                .await?;
            let conn = db.connect()?;
            for sql in [
                "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
                "CREATE TABLE IF NOT EXISTS blobs (hash TEXT PRIMARY KEY, kind TEXT NOT NULL, \
                 size INTEGER NOT NULL, data BLOB NOT NULL)",
                "CREATE TABLE IF NOT EXISTS updates (seq INTEGER PRIMARY KEY AUTOINCREMENT, \
                 label TEXT NOT NULL, at INTEGER NOT NULL, data BLOB NOT NULL)",
                "CREATE TABLE IF NOT EXISTS snapshots (upto INTEGER PRIMARY KEY, \
                 at INTEGER NOT NULL, data BLOB NOT NULL)",
                "CREATE TABLE IF NOT EXISTS versions (name TEXT PRIMARY KEY, \
                 frontiers BLOB NOT NULL, at INTEGER NOT NULL)",
            ] {
                conn.execute(sql, ()).await?;
            }
            let store = Self { conn, _db: db };
            match store.meta_async("schema").await? {
                None => {
                    store.set_meta_async("schema", &SCHEMA.to_string()).await?;
                }
                Some(found) => {
                    let found = found.parse().unwrap_or(-1);
                    if found != SCHEMA {
                        return Err(StoreError::Schema { found });
                    }
                }
            }
            Ok(store)
        })
    }

    async fn meta_async(&self, key: &str) -> StoreResult<Option<String>> {
        let mut rows = self
            .conn
            .query("SELECT value FROM meta WHERE key = ?1", (key,))
            .await?;
        Ok(match rows.next().await? {
            Some(row) => row.get::<String>(0).ok(),
            None => None,
        })
    }

    async fn set_meta_async(&self, key: &str, value: &str) -> StoreResult<()> {
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (key, value),
            )
            .await?;
        Ok(())
    }

    /// A `meta` value.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn meta(&self, key: &str) -> StoreResult<Option<String>> {
        run(self.meta_async(key))
    }

    /// Set a `meta` value.
    ///
    /// # Errors
    ///
    /// The database refused the write.
    pub fn set_meta(&self, key: &str, value: &str) -> StoreResult<()> {
        run(self.set_meta_async(key, value))
    }

    /// Keep `bytes` (of `kind`: `fx`, `midi`, `rpp`, …) and say its name.
    /// Bytes already here are not written again: a blob is stored once
    /// however many versions, alternatives or joined projects use it.
    ///
    /// # Errors
    ///
    /// The database refused the write.
    pub fn put_blob(&self, kind: &str, bytes: &[u8]) -> StoreResult<String> {
        let hash = hash_of(bytes);
        let packed = zstd::encode_all(bytes, LEVEL)?;
        let size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        run(self.conn.execute(
            "INSERT INTO blobs (hash, kind, size, data) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(hash) DO NOTHING",
            (hash.as_str(), kind, size, packed),
        ))?;
        Ok(hash)
    }

    /// The blob named `hash`, checked against its name.
    ///
    /// # Errors
    ///
    /// It is not here, or what is here is not what the name says.
    pub fn blob(&self, hash: &str) -> StoreResult<Vec<u8>> {
        let packed: Vec<u8> = run(async {
            let mut rows = self
                .conn
                .query("SELECT data FROM blobs WHERE hash = ?1", (hash,))
                .await?;
            match rows.next().await? {
                Some(row) => Ok(row.get::<Vec<u8>>(0)?),
                None => Err(StoreError::Missing(hash.to_owned())),
            }
        })?;
        let bytes = zstd::decode_all(packed.as_slice())?;
        if hash_of(&bytes) != hash {
            return Err(StoreError::Corrupt(hash.to_owned()));
        }
        Ok(bytes)
    }

    /// How many blobs, and their bytes before and after compression.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn blob_totals(&self) -> StoreResult<(i64, i64, i64)> {
        run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT COUNT(*), COALESCE(SUM(size), 0), COALESCE(SUM(LENGTH(data)), 0) \
                     FROM blobs",
                    (),
                )
                .await?;
            Ok(match rows.next().await? {
                Some(row) => (row.get(0)?, row.get(1)?, row.get(2)?),
                None => (0, 0, 0),
            })
        })
    }

    /// Append one step of edits — a Loro update, `label` the undo step's
    /// name — to the history.
    ///
    /// # Errors
    ///
    /// The database refused the write.
    pub fn append_update(&self, label: &str, update: &[u8]) -> StoreResult<()> {
        let packed = zstd::encode_all(update, LEVEL)?;
        run(self.conn.execute(
            "INSERT INTO updates (label, at, data) VALUES (?1, ?2, ?3)",
            (label, now(), packed),
        ))?;
        Ok(())
    }

    /// The project's doc: the latest snapshot, every update after it
    /// imported. An empty doc for a new project.
    ///
    /// # Errors
    ///
    /// The database refused the read, or the history would not import.
    pub fn load(&self) -> StoreResult<loro::LoroDoc> {
        let doc = loro::LoroDoc::new();
        let (upto, snapshot): (i64, Option<Vec<u8>>) = run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT upto, data FROM snapshots ORDER BY upto DESC LIMIT 1",
                    (),
                )
                .await?;
            StoreResult::Ok(match rows.next().await? {
                Some(row) => (row.get(0)?, Some(row.get::<Vec<u8>>(1)?)),
                None => (0, None),
            })
        })?;
        if let Some(packed) = snapshot {
            let bytes = zstd::decode_all(packed.as_slice())?;
            doc.import(&bytes)
                .map_err(|e| StoreError::Loro(e.to_string()))?;
        }
        let updates: Vec<Vec<u8>> = run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT data FROM updates WHERE seq > ?1 ORDER BY seq",
                    (upto,),
                )
                .await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(row.get::<Vec<u8>>(0)?);
            }
            StoreResult::Ok(out)
        })?;
        for packed in updates {
            let bytes = zstd::decode_all(packed.as_slice())?;
            doc.import(&bytes)
                .map_err(|e| StoreError::Loro(e.to_string()))?;
        }
        Ok(doc)
    }

    /// Fold the history into one snapshot of `doc` (which must hold every
    /// update appended so far) and drop the updates it covers. History is
    /// kept: a Loro snapshot carries every operation, so versions named
    /// before still check out.
    ///
    /// # Errors
    ///
    /// The doc would not export, or the database refused the write.
    pub fn compact(&self, doc: &loro::LoroDoc) -> StoreResult<()> {
        let snapshot = doc
            .export(loro::ExportMode::Snapshot)
            .map_err(|e| StoreError::Loro(e.to_string()))?;
        let packed = zstd::encode_all(snapshot.as_slice(), LEVEL)?;
        run(async {
            let mut rows = self
                .conn
                .query("SELECT COALESCE(MAX(seq), 0) FROM updates", ())
                .await?;
            let upto: i64 = match rows.next().await? {
                Some(row) => row.get(0)?,
                None => 0,
            };
            drop(rows);
            self.conn.execute("BEGIN", ()).await?;
            self.conn
                .execute(
                    "INSERT INTO snapshots (upto, at, data) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(upto) DO UPDATE SET at = excluded.at, data = excluded.data",
                    (upto, now(), packed),
                )
                .await?;
            self.conn
                .execute("DELETE FROM snapshots WHERE upto < ?1", (upto,))
                .await?;
            self.conn
                .execute("DELETE FROM updates WHERE seq <= ?1", (upto,))
                .await?;
            self.conn.execute("COMMIT", ()).await?;
            StoreResult::Ok(())
        })
    }

    /// Name the doc's current point in history `name` — an alternative,
    /// a checkpoint. A name already taken moves to here.
    ///
    /// # Errors
    ///
    /// The database refused the write.
    pub fn name_version(&self, name: &str, doc: &loro::LoroDoc) -> StoreResult<()> {
        let frontiers = doc.state_frontiers().encode();
        run(self.conn.execute(
            "INSERT INTO versions (name, frontiers, at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(name) DO UPDATE SET frontiers = excluded.frontiers, at = excluded.at",
            (name, frontiers, now()),
        ))?;
        Ok(())
    }

    /// Every named version, oldest first.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn versions(&self) -> StoreResult<Vec<Version>> {
        run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT name, frontiers, at FROM versions ORDER BY at, name",
                    (),
                )
                .await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(Version {
                    name: row.get(0)?,
                    frontiers: row.get(1)?,
                    at: row.get(2)?,
                });
            }
            Ok(out)
        })
    }
}
