//! Concrete [`ExampleService`] implementation.
//!
//! `ExampleService` is hand-written domain logic on top of whichever
//! `ExampleRepo` backend the build pulled in (db, memory, …). It owns
//! only operations that aren't pure CRUD: search, duplicate, etc.
//! Plain CRUD comes from `ExampleRepo` directly.

use example::architect::{Page, RepoError};
use example::{Example, ExampleCreate, ExampleRepo, ExampleService, ExampleServiceError};
use uuid::Uuid;

/// Upper bound on rows scanned for a search. The `Filter { raw }` AST is
/// still a placeholder (see architect's derive), so search runs as a
/// case-insensitive substring scan over a bounded page rather than a
/// pushed-down SQL `LIKE`. Bounded so a huge table can't wedge the call.
const SEARCH_SCAN_LIMIT: u32 = 500;

#[derive(Clone)]
pub struct ExampleServiceImpl<R: ExampleRepo + Clone + Send + Sync + 'static> {
    repo: R,
}

impl<R: ExampleRepo + Clone + Send + Sync + 'static> ExampleServiceImpl<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

impl<R: ExampleRepo + Clone + Send + Sync + 'static> ExampleService for ExampleServiceImpl<R> {
    async fn search(&self, query: String, limit: u32) -> Result<Vec<Example>, ExampleServiceError> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Err(ExampleServiceError::InvalidInput("query is empty".into()));
        }
        let limit = limit.clamp(1, SEARCH_SCAN_LIMIT) as usize;

        // Scan a bounded page and filter on the `fulltext` columns
        // (name + description). Backend-agnostic: works the same against
        // sqlite or the in-memory repo.
        let page = self
            .repo
            .list(
                Page {
                    index: 0,
                    size: SEARCH_SCAN_LIMIT,
                },
                None,
                None,
            )
            .await
            .map_err(map_repo_err)?;

        let hits = page
            .items
            .into_iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&needle)
                    || e.description.to_lowercase().contains(&needle)
            })
            .take(limit)
            .collect();
        Ok(hits)
    }

    async fn duplicate(
        &self,
        id: Uuid,
        new_name: Option<String>,
    ) -> Result<Example, ExampleServiceError> {
        let original = self.repo.get(id).await.map_err(map_repo_err)?;
        let name = new_name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("{} (copy)", original.name));

        self.repo
            .create(ExampleCreate {
                name,
                description: original.description,
            })
            .await
            .map_err(map_repo_err)
    }
}

/// Map the repo's error envelope onto the service's. `NotFound` and
/// `InvalidInput` carry through; `Conflict`/`Internal` collapse to
/// `Internal` (the service surface has no `Conflict` variant).
fn map_repo_err(e: RepoError) -> ExampleServiceError {
    match e {
        RepoError::NotFound => ExampleServiceError::NotFound,
        RepoError::InvalidInput(m) => ExampleServiceError::InvalidInput(m),
        RepoError::Conflict(m) => ExampleServiceError::Internal(format!("conflict: {m}")),
        RepoError::Internal(m) => ExampleServiceError::Internal(m),
    }
}
