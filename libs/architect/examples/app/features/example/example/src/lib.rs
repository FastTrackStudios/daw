//! Facade for the `example` feature.
//!
//! Single import root. Re-exports the wire contract from `example-proto`
//! unconditionally and gates everything else behind cargo features so
//! a consumer chooses their feature surface from one dep declaration:
//!
//! ```toml
//! # Cargo.toml of a binary that wants everything
//! example = { workspace = true, features = ["full"] }
//!
//! # Cargo.toml of a wasm client — just types + vox client/dispatcher
//! example = { workspace = true, features = ["vox"] }
//!
//! # Cargo.toml of a custom server with its own repo impl
//! example = { workspace = true, features = ["vox", "server-axum"] }
//!
//! # Cargo.toml of an in-process tool — no RPC, no storage opinion
//! example = { workspace = true }
//! ```
//!
//! See `docs/content/architecture/extensibility.md` (wiki:
//! [Architecture/Extensibility]) for when each combination is useful.

pub use example_proto::*;

/// SeaORM-backed in-tree implementation.
#[cfg(feature = "backend-db")]
pub mod backend_db {
    pub use example_db::{ExampleRepoStorage, Migrator};
}

/// Pure in-memory in-tree implementation. No external services needed.
#[cfg(feature = "backend-memory")]
pub mod backend_memory {
    pub use example_memory::{ExampleRepoMemory, TagRepoMemory};
}

/// Loro-backed CRDT implementation. Source of truth lives in a
/// LoroDoc; persistence + sync are layered on top.
#[cfg(feature = "backend-crdt")]
pub mod backend_crdt {
    pub use example_crdt::ExampleRepoLoro;
}

/// axum WebSocket adapter — `AxumWsLink`, `serve`, `lane_acceptor_fn`,
/// `acceptor_on`. Re-exported from `architect::axum_ws` so consumers
/// only ever need to import from `example::*`.
#[cfg(feature = "server-axum")]
pub use architect::axum_ws;
