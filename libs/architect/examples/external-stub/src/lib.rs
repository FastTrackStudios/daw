//! External `ExampleRepo` implementation — runnable demonstration of
//! the [Extensibility] pattern.
//!
//! [Extensibility]: https://codeberg.org/FastTrackStudios/architect/wiki/Architecture/Extensibility
//!
//! What this crate does:
//!
//! - Implements [`example_proto::ExampleRepo`] against an in-memory
//!   map. The implementation is intentionally trivial (no
//!   persistence, no concurrency tricks) — the *point* is to show
//!   that a struct in someone else's crate can plug into the same
//!   vox dispatcher the in-tree backends do.
//! - Depends on `example-proto` and `architect`, nothing else.
//!   Specifically: not `architect/server`, not `example-db`, not
//!   `example-memory`. A consumer who pulled this from crates.io
//!   would inherit only what the contract surface needs.
//!
//! How a consuming app uses it:
//!
//! ```ignore
//! use example_proto::ExampleRepoDispatcher;
//! use example_stub_backend::StubBackend;
//!
//! let backend = StubBackend::new();
//! let dispatcher = ExampleRepoDispatcher::new(backend);
//! // mount on vox WebSocket exactly the same way as in-tree backends:
//! //   conn.handle_with(dispatcher);
//! ```

use std::sync::Arc;

use architect::{Filter, Page, RepoError, Sort, SortOrder};
use chrono::Utc;
use example_proto::{Example, ExampleCreate, ExampleList, ExampleRepo, ExampleUpdate};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Hard-coded canned data shipped with the stub. Helps demonstrate
/// that an out-of-tree backend can ship with fixtures baked in.
const SEED_NAMES: &[&str] = &["stub.alpha", "stub.bravo", "stub.charlie"];

#[derive(Clone, Default)]
pub struct StubBackend {
    inner: Arc<RwLock<Vec<Example>>>,
}

impl StubBackend {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-populated with three canned `Example`s — useful for demos
    /// and quick exploration without going through a `create` cycle.
    pub fn with_seed_data() -> Self {
        let now = Utc::now();
        let items = SEED_NAMES
            .iter()
            .map(|n| Example {
                id: Uuid::new_v4(),
                name: (*n).into(),
                description: "shipped with example-stub-backend".into(),
                created_at: now,
                updated_at: now,
            })
            .collect();
        Self {
            inner: Arc::new(RwLock::new(items)),
        }
    }
}

// A third-party backend declares its Layer bundle exactly like an
// in-tree one — `StubBackend::with_seed_data().into_router()` is
// interchangeable with the SeaORM or in-memory backends at the call site.
#[cfg(feature = "vox")]
impl architect::Services for StubBackend {
    fn layers() -> impl architect::Layer<Self> {
        architect::layers![example_proto::ExampleRepoLayer]
    }
}

impl ExampleRepo for StubBackend {
    // r[impl repo.get.missing]
    async fn get(&self, id: Uuid) -> Result<Example, RepoError> {
        let guard = self.inner.read().await;
        guard
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or(RepoError::NotFound)
    }

    // r[impl repo.list.sort.name]
    // r[impl repo.list.sort.unknown]
    async fn list(
        &self,
        page: Page,
        sort: Option<Sort>,
        _filter: Option<Filter>,
    ) -> Result<ExampleList, RepoError> {
        let guard = self.inner.read().await;
        let mut items: Vec<Example> = guard.iter().cloned().collect();

        if let Some(s) = sort {
            match s.field.as_str() {
                "name" => {
                    items.sort_by(|a, b| a.name.cmp(&b.name));
                    if matches!(s.order, SortOrder::Desc) {
                        items.reverse();
                    }
                }
                other => {
                    return Err(RepoError::InvalidInput(format!(
                        "unsortable field: {other}"
                    )));
                }
            }
        }

        let total = items.len() as u32;
        let size = page.size.max(1) as usize;
        let start = (page.index as usize).saturating_mul(size);
        let items = items.into_iter().skip(start).take(size).collect();
        Ok(ExampleList { items, total, page })
    }

    // r[impl repo.create.id]
    async fn create(&self, input: ExampleCreate) -> Result<Example, RepoError> {
        let now = Utc::now();
        let row = Example {
            id: Uuid::new_v4(),
            name: input.name,
            description: input.description,
            created_at: now,
            updated_at: now,
        };
        self.inner.write().await.push(row.clone());
        Ok(row)
    }

    // r[impl repo.update.partial]
    async fn update(&self, id: Uuid, input: ExampleUpdate) -> Result<Example, RepoError> {
        let mut guard = self.inner.write().await;
        let row = guard
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(RepoError::NotFound)?;
        if let Some(v) = input.name {
            row.name = v;
        }
        if let Some(v) = input.description {
            row.description = v;
        }
        row.updated_at = Utc::now();
        Ok(row.clone())
    }

    // r[impl repo.delete.missing]
    async fn delete(&self, id: Uuid) -> Result<(), RepoError> {
        let mut guard = self.inner.write().await;
        let before = guard.len();
        guard.retain(|e| e.id != id);
        if guard.len() == before {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}
