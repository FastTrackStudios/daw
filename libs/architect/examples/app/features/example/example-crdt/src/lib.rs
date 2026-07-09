//! Loro-backed `ExampleRepo`. The doc + repo machinery lives in
//! `features/crdt/crdt`; this crate provides the entity-specific glue:
//!
//! 1. `impl EntityCrdt for Example` — codec, id/timestamp policy,
//!    sort-field lookup. The architect derive will emit this from
//!    field attributes (`primary_key`, `sortable`, `on_create`).
//! 2. `impl ExampleRepo for LoroRepo<Example>` — thin forwarders
//!    onto `LoroRepo`'s generic methods. The derive will emit this
//!    too whenever `#[architect(repo)]` is set.
//!
//! Container layout:
//!
//! ```text
//! doc
//!  └── "examples" (LoroMap)
//!       └── <uuid string>  → (LoroMap)
//!            ├── "id"          (String)        uuid
//!            ├── "name"        (String)
//!            ├── "description" (String)
//!            ├── "created_at"  (String)        RFC3339
//!            └── "updated_at"  (String)        RFC3339
//! ```

use architect::{Page, RepoError, SortOrder};
use chrono::Utc;
use crdt::EntityCrdt;
use crdt::codec::{read_dt, read_str, read_uuid, write_dt, write_str, write_uuid};
use example_proto::{Example, ExampleCreate, ExampleList, ExampleRepo, ExampleUpdate};
use loro::LoroMap;
use uuid::Uuid;

pub use crdt::{CrdtDoc, LoroRepo};

/// Zero-sized marker for the `EntityCrdt` impl. The wire types live
/// in `example-proto`; this crate owns the marker so we satisfy
/// Rust's orphan rules when implementing `EntityCrdt` (foreign trait)
/// and `ExampleRepo` (foreign trait) for `LoroRepo<ExampleEntity>`.
pub struct ExampleEntity;

/// Loro-backed `ExampleRepo`. Wraps the generic `LoroRepo<ExampleEntity>`
/// in a newtype so we can `impl ExampleRepo` on it — Rust's orphan
/// rules don't allow `impl ExampleRepo for LoroRepo<ExampleEntity>`
/// (foreign trait, foreign generic wrapper around a local type).
#[derive(Clone)]
pub struct ExampleRepoLoro {
    inner: LoroRepo<ExampleEntity>,
}

impl ExampleRepoLoro {
    /// Build from an open `CrdtDoc`. Multiple repos over different
    /// entity types can share one doc.
    pub fn new(doc: &CrdtDoc) -> Self {
        Self { inner: doc.repo() }
    }

    /// Borrow the underlying generic repo — for test bodies that
    /// want to drive `LoroRepo` directly (export bundle assembly,
    /// inspecting the doc).
    pub fn inner(&self) -> &LoroRepo<ExampleEntity> {
        &self.inner
    }

    /// Borrow the underlying LoroDoc — for export/import round-trips,
    /// subscribing to updates, etc. Same handle as `inner().doc()`.
    pub fn doc(&self) -> &loro::LoroDoc {
        self.inner.doc()
    }
}

// ── EntityCrdt impl ───────────────────────────────────────────────────

impl EntityCrdt for ExampleEntity {
    type Wire = Example;
    type Create = ExampleCreate;
    type Update = ExampleUpdate;
    type List = ExampleList;

    const ROOT: &'static str = "examples";

    fn id(wire: &Example) -> Uuid {
        wire.id
    }

    fn from_create(input: ExampleCreate) -> Example {
        let now = Utc::now();
        Example {
            id: Uuid::new_v4(),
            name: input.name,
            description: input.description,
            created_at: now,
            updated_at: now,
        }
    }

    fn encode_into(m: &LoroMap, e: &Example) -> Result<(), RepoError> {
        write_uuid(m, "id", e.id)?;
        write_str(m, "name", &e.name)?;
        write_str(m, "description", &e.description)?;
        write_dt(m, "created_at", e.created_at)?;
        write_dt(m, "updated_at", e.updated_at)?;
        Ok(())
    }

    fn decode_from(m: &LoroMap) -> Result<Example, RepoError> {
        Ok(Example {
            id: read_uuid(m, "id")?,
            name: read_str(m, "name")?,
            description: read_str(m, "description")?,
            created_at: read_dt(m, "created_at")?,
            updated_at: read_dt(m, "updated_at")?,
        })
    }

    fn apply_update(m: &LoroMap, input: ExampleUpdate) -> Result<(), RepoError> {
        if let Some(name) = input.name {
            write_str(m, "name", &name)?;
        }
        if let Some(description) = input.description {
            write_str(m, "description", &description)?;
        }
        write_dt(m, "updated_at", Utc::now())?;
        Ok(())
    }

    fn sort_items(items: &mut [Example], field: &str, order: SortOrder) -> Result<(), RepoError> {
        match field {
            "name" => items.sort_by(|a, b| a.name.cmp(&b.name)),
            "description" => items.sort_by(|a, b| a.description.cmp(&b.description)),
            "created_at" => items.sort_by_key(|a| a.created_at),
            other => {
                return Err(RepoError::InvalidInput(format!(
                    "unsortable field: {other}"
                )));
            }
        }
        if matches!(order, SortOrder::Desc) {
            items.reverse();
        }
        Ok(())
    }

    fn build_list(items: Vec<Example>, total: u32, page: Page) -> ExampleList {
        ExampleList { items, total, page }
    }
}

// ── ExampleRepo impl ──────────────────────────────────────────────────
//
// One-line forwarders; the macro will emit this block.

// Layer bundle: the CRDT backend provides the repo service like any
// other, so `ExampleRepoLoro::into_router()` / `serve_local(loro_repo)`
// work — a local-first client can serve a Loro-backed repo in-process.
#[cfg(feature = "vox")]
impl architect::Services for ExampleRepoLoro {
    fn layers() -> impl architect::Layer<Self> {
        architect::layers![example_proto::ExampleRepoLayer]
    }
}

impl ExampleRepo for ExampleRepoLoro {
    async fn get(&self, id: Uuid) -> Result<Example, RepoError> {
        self.inner.get(id).await
    }

    async fn list(
        &self,
        page: architect::Page,
        sort: Option<architect::Sort>,
        filter: Option<architect::Filter>,
    ) -> Result<ExampleList, RepoError> {
        self.inner.list(page, sort, filter).await
    }

    async fn create(&self, input: ExampleCreate) -> Result<Example, RepoError> {
        self.inner.create(input).await
    }

    async fn update(&self, id: Uuid, input: ExampleUpdate) -> Result<Example, RepoError> {
        self.inner.update(id, input).await
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepoError> {
        self.inner.delete(id).await
    }
}
