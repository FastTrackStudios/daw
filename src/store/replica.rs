//! The project file as a sync replica (`docs/design/sync.md`).
//!
//! Every branch of a project is its own synced doc, with an id of its own
//! kept in `meta` (`doc:<branch>` → UUID): made the first time it is asked
//! for, or adopted from another device when the project arrived from there.
//! [`Replica`] is the store as a [`crdt::Persistence`]: what a sync session
//! receives is appended to the branch like any undo step, and a fold is
//! [`Store::compact`]. The file stays the one place the project lives —
//! sync is one more writer of it, never a second copy.

use uuid::Uuid;

use super::{Store, StoreError, StoreResult, run};

/// The `meta` key a branch's doc id is kept under.
fn key(branch: &str) -> String {
    format!("doc:{branch}")
}

/// What a step that came in by sync is named in the branch's history.
pub const SYNC_LABEL: &str = "Sync";

impl Store {
    /// `branch`'s doc id, made now if it has none.
    ///
    /// # Errors
    ///
    /// No such branch, or the database refused.
    pub fn doc_id(&self, branch: &str) -> StoreResult<Uuid> {
        if !self.branches()?.iter().any(|b| b.name == branch) {
            return Err(StoreError::NoBranch(branch.to_owned()));
        }
        if let Some(id) = self
            .meta(&key(branch))?
            .and_then(|v| Uuid::parse_str(&v).ok())
        {
            return Ok(id);
        }
        let id = Uuid::new_v4();
        self.set_meta(&key(branch), &id.to_string())?;
        Ok(id)
    }

    /// Take `id` as `branch`'s doc id: the project came from another
    /// device, and its branches keep the ids they were made with there.
    ///
    /// # Errors
    ///
    /// No such branch, or the database refused.
    pub fn adopt_doc_id(&self, branch: &str, id: Uuid) -> StoreResult<()> {
        if !self.branches()?.iter().any(|b| b.name == branch) {
            return Err(StoreError::NoBranch(branch.to_owned()));
        }
        self.set_meta(&key(branch), &id.to_string())
    }

    /// The branch whose doc is `id`, if it is one of this project's.
    ///
    /// # Errors
    ///
    /// The database refused the read.
    pub fn branch_of(&self, id: Uuid) -> StoreResult<Option<String>> {
        run(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT key FROM meta WHERE key LIKE 'doc:%' AND value = ?1",
                    (id.to_string(),),
                )
                .await?;
            Ok(match rows.next().await? {
                Some(row) => row
                    .get::<String>(0)?
                    .strip_prefix("doc:")
                    .map(str::to_owned),
                None => None,
            })
        })
    }
}

/// A project file as a [`crdt::Persistence`]: one sync session per branch,
/// each naming its branch by the doc id ([`Store::doc_id`]).
pub struct Replica {
    store: std::sync::Mutex<Store>,
    /// The name the next appended update is recorded under
    /// ([`Self::name_next`]); [`SYNC_LABEL`] when nobody named it.
    next: std::sync::Mutex<Option<String>>,
}

impl Replica {
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self {
            store: std::sync::Mutex::new(store),
            next: std::sync::Mutex::new(None),
        }
    }

    /// Record the next update under `label`: an app editing the synced doc
    /// names its step just before committing it, and the doc's own writer
    /// appends it — so the branch's history keeps the step's name instead
    /// of [`SYNC_LABEL`]. An update another device sent in that moment
    /// would take the name instead; the history is a list of names, and
    /// the operations are the same either way.
    pub fn name_next(&self, label: &str) {
        *self
            .next
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(label.to_owned());
    }

    fn take_name(&self) -> String {
        self.next
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_else(|| SYNC_LABEL.to_owned())
    }

    /// The store, for the rest of what the app does with the file. Held
    /// only for synchronous calls: the store drives its database to
    /// completion on the calling thread, so nothing awaits under it.
    pub fn with<T>(&self, f: impl FnOnce(&Store) -> T) -> T {
        let store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&store)
    }

    /// The branch `id` names, or why it names none here.
    fn branch(store: &Store, id: Uuid) -> Result<String, crdt::PersistError> {
        store
            .branch_of(id)
            .map_err(backend)?
            .ok_or_else(|| crdt::PersistError::Backend(format!("doc {id} is not in this project")))
    }
}

fn backend(e: StoreError) -> crdt::PersistError {
    crdt::PersistError::Backend(e.to_string())
}

#[async_trait::async_trait]
impl crdt::Persistence for Replica {
    /// The branch as it loads — its snapshot or fork point, and every
    /// step after — in one snapshot, so nothing comes from
    /// [`Self::load_updates`].
    async fn load_snapshot(&self, doc_id: Uuid) -> Result<Option<Vec<u8>>, crdt::PersistError> {
        self.with(|store| {
            let Some(branch) = store.branch_of(doc_id).map_err(backend)? else {
                return Ok(None);
            };
            let doc = store.load(&branch).map_err(backend)?;
            doc.export(loro::ExportMode::Snapshot)
                .map(Some)
                .map_err(|e| crdt::PersistError::Backend(e.to_string()))
        })
    }

    async fn write_snapshot(&self, doc_id: Uuid, bytes: &[u8]) -> Result<(), crdt::PersistError> {
        self.compact(doc_id, bytes).await
    }

    async fn append_update(&self, doc_id: Uuid, bytes: &[u8]) -> Result<(), crdt::PersistError> {
        let label = self.take_name();
        self.with(|store| {
            let branch = Self::branch(store, doc_id)?;
            store.append_update(&branch, &label, bytes).map_err(backend)
        })
    }

    async fn load_updates(&self, _doc_id: Uuid) -> Result<Vec<Vec<u8>>, crdt::PersistError> {
        Ok(Vec::new())
    }

    async fn compact(&self, doc_id: Uuid, snapshot: &[u8]) -> Result<(), crdt::PersistError> {
        self.with(|store| {
            let branch = Self::branch(store, doc_id)?;
            let doc = loro::LoroDoc::new();
            doc.import(snapshot)
                .map_err(|e| crdt::PersistError::Backend(e.to_string()))?;
            store.compact(&branch, &doc).map_err(backend)
        })
    }
}
