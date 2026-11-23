//! A song's other documents, in its one file.
//!
//! The project (the engine's doc, [`Store::load`]) is not all a song is:
//! the session primitive keeps its timing, its sections and markers and
//! every extension's data (Keyflow's chart, Ignition's cues, Signal's
//! patch changes) in a Loro document of its own. That document lives here
//! too, as a **part**: a named Loro document beside the project, one per
//! branch, so `<Title>.session` is the whole song.
//!
//! ```text
//! part_updates    seq → part, branch, at, update   each commit, packed
//! part_snapshots  (part, branch) → upto, snapshot  folded by compact_part
//! meta            id → the song's UUID             what setlists name
//!                 part:<part>:<branch> → UUID      the part's doc id
//! ```
//!
//! A part is small (kilobytes: a tempo map, a chart's items), so a branch
//! forked from another takes a copy of the parent's part whole, history
//! included ([`Store::fork`]): the two stay of one lineage and merge, and
//! each syncs on its own doc id. The song's `main` part [`SESSION`] syncs
//! under the song's own id ([`Store::song_id`]): the id the session
//! document had when it was a file of its own, so peers that synced it
//! then still find it.
//!
//! Every update row is one Loro update: a peer that says what it has (a
//! version vector) is answered from the part's doc
//! ([`Store::part_updates_since`]), the exchange Drive's `/sync` makes.

use uuid::Uuid;

use super::{MAIN, Store, StoreError, StoreResult, import, now, run};

/// The session primitive's document: timing, sections, markers and every
/// extension's data.
pub const SESSION: &str = "session";

/// The `meta` key the song's id is kept under.
pub const ID_KEY: &str = "id";

/// The tables parts are kept in. Made by [`Store::open`].
pub(super) const TABLES: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS part_updates (seq INTEGER PRIMARY KEY AUTOINCREMENT, \
     part TEXT NOT NULL, branch TEXT NOT NULL, at INTEGER NOT NULL, data BLOB NOT NULL)",
    "CREATE TABLE IF NOT EXISTS part_snapshots (part TEXT NOT NULL, branch TEXT NOT NULL, \
     upto INTEGER NOT NULL, at INTEGER NOT NULL, data BLOB NOT NULL, \
     PRIMARY KEY (part, branch))",
];

/// The `meta` key part `part` of `branch` keeps its doc id under.
fn id_key(part: &str, branch: &str) -> String {
    format!("part:{part}:{branch}")
}

/// A part of a branch, as [`Store::parts`] lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartRef {
    pub part: String,
    pub branch: String,
}

impl Store {
    /// The song's id, when it has one: what a setlist names it by, kept in
    /// `meta.id`.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn song_id(&self) -> StoreResult<Option<Uuid>> {
        Ok(self
            .meta(ID_KEY)?
            .and_then(|v| Uuid::parse_str(v.trim()).ok()))
    }

    /// The song's id, made now if it has none.
    ///
    /// # Errors
    ///
    /// The database refused.
    pub fn ensure_song_id(&self) -> StoreResult<Uuid> {
        if let Some(id) = self.song_id()? {
            return Ok(id);
        }
        let id = Uuid::new_v4();
        self.set_meta(ID_KEY, &id.to_string())?;
        Ok(id)
    }

    /// Give the song the id `id` (a song brought in from a file that
    /// already named it). A song with another id keeps it: an id names one
    /// song for good.
    ///
    /// # Errors
    ///
    /// The song already has a different id, or the database refused.
    pub fn set_song_id(&self, id: Uuid) -> StoreResult<()> {
        match self.song_id()? {
            Some(had) if had == id => Ok(()),
            Some(had) => Err(StoreError::IdTaken {
                had: had.to_string(),
                asked: id.to_string(),
            }),
            None => self.set_meta(ID_KEY, &id.to_string()),
        }
    }

    /// Part `part` of `branch`'s doc id: for the song's [`SESSION`] on
    /// `main`, the song's id; else its own, made the first time it is asked
    /// for.
    ///
    /// # Errors
    ///
    /// The database refused.
    pub fn part_id(&self, part: &str, branch: &str) -> StoreResult<Uuid> {
        if part == SESSION && branch == MAIN {
            return self.ensure_song_id();
        }
        let key = id_key(part, branch);
        if let Some(id) = self.meta(&key)?.and_then(|v| Uuid::parse_str(&v).ok()) {
            return Ok(id);
        }
        let id = Uuid::new_v4();
        self.set_meta(&key, &id.to_string())?;
        Ok(id)
    }

    /// The part whose doc id is `id`, if it is one of this song's.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn part_of(&self, id: Uuid) -> StoreResult<Option<PartRef>> {
        if self.song_id()? == Some(id) {
            return Ok(Some(PartRef {
                part: SESSION.to_owned(),
                branch: MAIN.to_owned(),
            }));
        }
        let key: Option<String> = run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT key FROM meta WHERE key LIKE 'part:%' AND value = ?1",
                    (id.to_string(),),
                )
                .await?;
            StoreResult::Ok(match rows.next().await? {
                Some(row) => Some(row.get::<String>(0)?),
                None => None,
            })
        })?;
        Ok(key.and_then(|k| {
            let rest = k.strip_prefix("part:")?;
            let (part, branch) = rest.split_once(':')?;
            Some(PartRef {
                part: part.to_owned(),
                branch: branch.to_owned(),
            })
        }))
    }

    /// Every part this file holds anything of, by part then branch.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn parts(&self) -> StoreResult<Vec<PartRef>> {
        run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT part, branch FROM part_snapshots \
                     UNION SELECT part, branch FROM part_updates \
                     ORDER BY 1, 2",
                    (),
                )
                .await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(PartRef {
                    part: row.get(0)?,
                    branch: row.get(1)?,
                });
            }
            StoreResult::Ok(out)
        })
    }

    /// Whether `branch` holds anything of part `part`.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn has_part(&self, part: &str, branch: &str) -> StoreResult<bool> {
        Ok(self
            .parts()?
            .iter()
            .any(|p| p.part == part && p.branch == branch))
    }

    /// Append one commit of part `part` of `branch`: a Loro update.
    ///
    /// # Errors
    ///
    /// The database refused the write.
    pub fn append_part_update(&self, part: &str, branch: &str, update: &[u8]) -> StoreResult<()> {
        let packed = zstd::encode_all(update, super::LEVEL)?;
        run(self.conn.execute(
            "INSERT INTO part_updates (part, branch, at, data) VALUES (?1, ?2, ?3, ?4)",
            (part, branch, now(), packed),
        ))?;
        Ok(())
    }

    /// Part `part` of `branch`: its snapshot, then every update after it.
    /// An empty doc when the branch holds none of it.
    ///
    /// # Errors
    ///
    /// The database refused the read, or the history would not import.
    pub fn load_part(&self, part: &str, branch: &str) -> StoreResult<loro::LoroDoc> {
        let snapshot: Option<(i64, Vec<u8>)> = run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT upto, data FROM part_snapshots WHERE part = ?1 AND branch = ?2",
                    (part, branch),
                )
                .await?;
            StoreResult::Ok(match rows.next().await? {
                Some(row) => Some((row.get(0)?, row.get(1)?)),
                None => None,
            })
        })?;
        let doc = loro::LoroDoc::new();
        let upto = match snapshot {
            Some((upto, packed)) => {
                import(&doc, &packed)?;
                upto
            }
            None => 0,
        };
        let updates: Vec<Vec<u8>> = run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT data FROM part_updates WHERE part = ?1 AND branch = ?2 AND seq > ?3 \
                     ORDER BY seq",
                    (part, branch, upto),
                )
                .await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(row.get::<Vec<u8>>(0)?);
            }
            StoreResult::Ok(out)
        })?;
        let bytes = updates
            .iter()
            .map(|packed| zstd::decode_all(packed.as_slice()))
            .collect::<Result<Vec<Vec<u8>>, _>>()?;
        if !bytes.is_empty() {
            doc.import_batch(&bytes)
                .map_err(|e| StoreError::Loro(e.to_string()))?;
        }
        Ok(doc)
    }

    /// What part `part` of `branch` holds past `theirs` (a peer's version
    /// vector, `loro::VersionVector::encode`d; empty for everything), as
    /// one Loro update: the answer to a peer that says what it has.
    ///
    /// # Errors
    ///
    /// `theirs` is not a version vector, or the part would not load or
    /// export.
    pub fn part_updates_since(
        &self,
        part: &str,
        branch: &str,
        theirs: &[u8],
    ) -> StoreResult<Vec<u8>> {
        let doc = self.load_part(part, branch)?;
        let vv = if theirs.is_empty() {
            loro::VersionVector::default()
        } else {
            loro::VersionVector::decode(theirs).map_err(|e| StoreError::Loro(e.to_string()))?
        };
        doc.export(loro::ExportMode::updates(&vv))
            .map_err(|e| StoreError::Loro(e.to_string()))
    }

    /// Fold part `part` of `branch` into one snapshot of `doc` (the part as
    /// loaded, with every update appended so far) and drop the updates it
    /// covers. A Loro snapshot keeps every operation: history survives.
    ///
    /// # Errors
    ///
    /// The doc would not export, or the database refused the write.
    pub fn compact_part(&self, part: &str, branch: &str, doc: &loro::LoroDoc) -> StoreResult<()> {
        let _span = tracing::info_span!("store.compact_part", store.part = part).entered();
        let upto = self.part_upto(part, branch)?;
        let data = doc
            .export(loro::ExportMode::Snapshot)
            .map_err(|e| StoreError::Loro(e.to_string()))?;
        let packed = zstd::encode_all(data.as_slice(), super::LEVEL)?;
        run(async {
            self.conn.execute("BEGIN", ()).await?;
            self.conn
                .execute(
                    "INSERT INTO part_snapshots (part, branch, upto, at, data) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(part, branch) DO UPDATE SET upto = excluded.upto, \
                     at = excluded.at, data = excluded.data \
                     WHERE excluded.upto >= part_snapshots.upto",
                    (part, branch, upto, now(), packed),
                )
                .await?;
            self.conn
                .execute(
                    "DELETE FROM part_updates WHERE part = ?1 AND branch = ?2 AND seq <= ?3",
                    (part, branch, upto),
                )
                .await?;
            self.conn.execute("COMMIT", ()).await?;
            StoreResult::Ok(())
        })
    }

    /// The newest update of part `part` of `branch`, by its `seq`; 0 for
    /// none.
    fn part_upto(&self, part: &str, branch: &str) -> StoreResult<i64> {
        run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT COALESCE(MAX(seq), 0) FROM part_updates WHERE part = ?1 AND branch = ?2",
                    (part, branch),
                )
                .await?;
            StoreResult::Ok(match rows.next().await? {
                Some(row) => row.get(0)?,
                None => 0,
            })
        })
    }

    /// The parts of `parent`, each as one packed snapshot: what a branch
    /// forked from it starts from.
    pub(super) fn part_snapshots_of(&self, parent: &str) -> StoreResult<Vec<(String, Vec<u8>)>> {
        let mut out = Vec::new();
        for p in self.parts()?.into_iter().filter(|p| p.branch == parent) {
            let doc = self.load_part(&p.part, parent)?;
            let data = doc
                .export(loro::ExportMode::Snapshot)
                .map_err(|e| StoreError::Loro(e.to_string()))?;
            out.push((p.part, zstd::encode_all(data.as_slice(), super::LEVEL)?));
        }
        Ok(out)
    }

    /// Every blob's name (its SHA-256), in no order.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn blob_names(&self) -> StoreResult<Vec<String>> {
        run(async {
            let mut rows = self.conn.query("SELECT hash FROM blobs", ()).await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(row.get::<String>(0)?);
            }
            StoreResult::Ok(out)
        })
    }

    /// Checkpoint the file and let go of it: what is written is in the one
    /// file, and its write-ahead log is empty, so the file alone is the
    /// song — to copy, move to another disk or sync. What a song does when
    /// it is closed.
    ///
    /// # Errors
    ///
    /// The database refused the checkpoint (the store is let go of either
    /// way; the log is folded the next time the file is opened).
    pub fn close(self) -> StoreResult<()> {
        let _span = tracing::info_span!("store.close").entered();
        self.checkpoint()
    }
}
