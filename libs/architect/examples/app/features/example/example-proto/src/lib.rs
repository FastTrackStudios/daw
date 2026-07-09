//! `example-proto` — the canonical `architect` usage example. **Start
//! here:** these files are the contract the whole stack is generated from.
//! End-to-end walkthrough: `docs/content/getting-started/walkthrough.md`.
//!
//! The crate is split by concern so it reads as a template for your own
//! feature-proto crate:
//!
//! - `entity` — the `Example` struct. `#[derive(architect::Entity)]`
//!   emits the wire types, the `ExampleRepo` CRUD service, the layer token,
//!   and (on server builds) the SeaORM bridge.
//! - `error` — `ExampleServiceError`, the wire error for the custom
//!   service.
//! - `service` — `ExampleService`, a hand-written `#[vox::service]` for
//!   domain operations beyond CRUD (search, duplicate). **The extension
//!   point** — add methods or whole new services here.
//! - `tag` — the `Tag` entity: a **String** primary key (caller-supplied
//!   slug), proving the derive is not Uuid-specific.
//! - `atom` (client-only, `atom` feature) — binds `Example` into the
//!   `architect::Store` optimistic cache (`impl StoreEntity` +
//!   `Example::draft`).
//!
//! Everything public is re-exported flat (`example_proto::Example`,
//! `example_proto::ExampleRepoClient`, `example_proto::ExampleService`, …),
//! so the macro-emitted types land at the crate root as before.
//!
//! Backends (`example-memory`, `example-db`, `example-crdt`,
//! `examples/external-stub`) each `impl ExampleRepo` + declare a `Services`
//! bundle; the server composes them via `app_server::service_router`;
//! clients consume them over a remote or in-process `Transport`
//! (`examples/app/ui/src/client.rs`).

// Re-exported so downstream test/client crates can reach Page/Sort/Filter
// without taking a direct architect dep.
pub use architect;

mod entity;
mod error;
mod note;
mod service;
mod tag;

// Flat re-exports — the derive (`entity`) and `#[vox::service]` (`service`)
// macros emit their client/dispatcher/descriptor types into their own
// modules; surface them all at the crate root.
pub use entity::*;
pub use error::ExampleServiceError;
pub use note::*;
pub use service::*;
pub use tag::*;
