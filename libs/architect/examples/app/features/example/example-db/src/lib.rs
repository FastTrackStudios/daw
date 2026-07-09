//! Server-only persistence layer.
//!
//! Re-exports the SeaORM bits emitted by the `Entity` derive macro on
//! [`example_proto::Example`] (the actual `Model` lives in the hidden
//! `__example_storage` module — re-exported below as `ExampleModel`).
//! Anything that needs to read or write the `examples` table — the
//! axum server, a CLI tool, a migration binary — depends on this
//! crate. Wasm clients never see it.

// The architect-derive macro puts the SeaORM `Entity`/`Model`/`Column`/
// `Relation`/`ActiveModel` items inside a hidden `__<snake>_storage`
// module so multiple `#[derive(Entity)]` structs in one crate don't
// collide on those bare names. Re-export them from there.
pub use example_proto::__example_storage::Entity as ExampleEntity;
pub use example_proto::__example_storage::{
    ActiveModel as ExampleActiveModel, Column as ExampleColumn, Model as ExampleModel,
    Relation as ExampleRelation,
};
// crudcrate-generated repo + storage glue:
pub use example_proto::{
    Example, ExampleCreate, ExampleList, ExampleRepo, ExampleRepoStorage, ExampleUpdate,
};

pub mod migrations;

pub use migrations::Migrator;
