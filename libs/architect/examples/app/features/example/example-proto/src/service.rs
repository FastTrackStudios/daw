//! `ExampleService` — the **custom service**, where you expand beyond CRUD.
//!
//! The [`architect::Entity`] derive on [`Example`](crate::Example) already
//! gives you the full CRUD surface for free (the `ExampleRepo` trait:
//! `get` / `list` / `create` / `update` / `delete`). `ExampleService` is a
//! plain, hand-written `#[vox::service]` trait for the operations that
//! *don't* map onto CRUD — full-text search, multi-row mutations, workflow
//! steps, aggregations, anything domain-specific.
//!
//! This is the extension point: **add a method here**, implement it on the
//! server (see `app-server`'s `ExampleServiceImpl`), and it's instantly
//! callable from every client over the same vox link — no new wire
//! plumbing. The macro emits `ExampleServiceClient` (caller proxy),
//! `ExampleServiceDispatcher` (server mount), and
//! `example_service_service_descriptor()` alongside the trait, exactly like
//! the derive does for the repo.
//!
//! The server mounts both the repo bundle *and* this service on one router
//! (`app_server::service_router`), so a client calls whichever surface fits
//! the call site. To add a second service, write another
//! `#[vox::service] trait` in its own module and mount its dispatcher too.

use uuid::Uuid;

use crate::entity::Example;
use crate::error::ExampleServiceError;

// Gated on the `vox` feature exactly like the architect-emitted
// `ExampleRepo` trait — drop the vox feature and the trait is plain
// async-in-trait without an RPC surface (usable in-process, no dispatcher).
#[cfg_attr(feature = "vox", vox::service)]
pub trait ExampleService {
    /// Full-text search across `name` + `description`. Returns at most
    /// `limit` matches (the server clamps to a sane max).
    async fn search(&self, query: String, limit: u32) -> Result<Vec<Example>, ExampleServiceError>;

    /// Duplicate an existing example, optionally overriding the name on
    /// the new row. Returns the freshly inserted record.
    async fn duplicate(
        &self,
        id: Uuid,
        new_name: Option<String>,
    ) -> Result<Example, ExampleServiceError>;
}
