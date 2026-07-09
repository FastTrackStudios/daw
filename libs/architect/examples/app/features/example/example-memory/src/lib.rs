//! In-memory `ExampleRepo` implementation.
//!
//! Same trait surface as `example_db::ExampleRepoStorage` — different
//! backend. The contract is `example_proto::ExampleRepo`; both crates
//! prove the same code can be dispatched against either backend without
//! the consumer caring.

use std::sync::Arc;

use architect::{Filter, Page, RepoError, Sort, SortOrder};
use chrono::Utc;
use example_proto::{Example, ExampleCreate, ExampleList, ExampleRepo, ExampleUpdate};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct ExampleRepoMemory {
    inner: Arc<RwLock<Vec<Example>>>,
}

impl ExampleRepoMemory {
    pub fn new() -> Self {
        Self::default()
    }
}

// This backend's Layer bundle: it provides exactly the repo service.
// `ExampleRepoMemory::into_router()` mounts it, interchangeable with any
// other `ExampleRepo` backend at the call site.
#[cfg(feature = "vox")]
impl architect::Services for ExampleRepoMemory {
    fn layers() -> impl architect::Layer<Self> {
        architect::layers![example_proto::ExampleRepoLayer]
    }
}

impl ExampleRepo for ExampleRepoMemory {
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

// ── Tag: the String-primary-key entity ────────────────────────────────
//
// Same backend pattern as `ExampleRepoMemory`, but the key is a
// caller-supplied `slug: String`. Create rejects duplicate slugs with
// `RepoError::Conflict` — with client-supplied ids, uniqueness is the
// backend's job.

use example_proto::{Tag, TagCreate, TagList, TagRepo, TagUpdate};

#[derive(Clone, Default)]
pub struct TagRepoMemory {
    inner: Arc<RwLock<Vec<Tag>>>,
}

impl TagRepoMemory {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(feature = "vox")]
impl architect::Services for TagRepoMemory {
    fn layers() -> impl architect::Layer<Self> {
        architect::layers![example_proto::TagRepoLayer]
    }
}

impl TagRepo for TagRepoMemory {
    async fn get(&self, id: String) -> Result<Tag, RepoError> {
        let guard = self.inner.read().await;
        guard
            .iter()
            .find(|t| t.slug == id)
            .cloned()
            .ok_or(RepoError::NotFound)
    }

    async fn list(
        &self,
        page: Page,
        sort: Option<Sort>,
        _filter: Option<Filter>,
    ) -> Result<TagList, RepoError> {
        let guard = self.inner.read().await;
        let mut items: Vec<Tag> = guard.iter().cloned().collect();

        if let Some(s) = sort {
            match s.field.as_str() {
                "label" => {
                    items.sort_by(|a, b| a.label.cmp(&b.label));
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
        Ok(TagList { items, total, page })
    }

    async fn create(&self, input: TagCreate) -> Result<Tag, RepoError> {
        let mut guard = self.inner.write().await;
        if guard.iter().any(|t| t.slug == input.slug) {
            return Err(RepoError::Conflict(format!(
                "tag `{}` already exists",
                input.slug
            )));
        }
        let now = Utc::now();
        let row = Tag {
            slug: input.slug,
            label: input.label,
            created_at: now,
            updated_at: now,
        };
        guard.push(row.clone());
        Ok(row)
    }

    async fn update(&self, id: String, input: TagUpdate) -> Result<Tag, RepoError> {
        let mut guard = self.inner.write().await;
        let row = guard
            .iter_mut()
            .find(|t| t.slug == id)
            .ok_or(RepoError::NotFound)?;
        if let Some(v) = input.label {
            row.label = v;
        }
        row.updated_at = Utc::now();
        Ok(row.clone())
    }

    async fn delete(&self, id: String) -> Result<(), RepoError> {
        let mut guard = self.inner.write().await;
        let before = guard.len();
        guard.retain(|t| t.slug != id);
        if guard.len() == before {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}
