# The example-proto pattern

This is the canonical layout for any new Rust + Dioxus + SeaORM
project. It's written so that adding a new entity is a single-file
change in `crates/<entity>-proto`, with the macro doing the rest.

## What you write per entity

```rust
// crates/example-proto/src/lib.rs

#[cfg(feature = "server")]
mod server {
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, EntityToModels)]
    #[sea_orm(table_name = "examples")]
    #[crudcrate(api_struct = "Example", generate_vox_service)]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        #[crudcrate(primary_key, exclude(create, update), on_create = Uuid::new_v4())]
        pub id: Uuid,
        #[crudcrate(filterable, sortable, fulltext)]
        pub name: String,
        // …
    }
}

#[cfg(not(feature = "server"))]
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct Example {
    pub id: Uuid,
    pub name: String,
    // …
}

#[vox::service]
pub trait ExampleService {
    async fn list_examples(&self) -> Result<Vec<Example>, ExampleServiceError>;
    async fn rename_example(&self, id: Uuid, new_name: String)
        -> Result<Example, ExampleServiceError>;
    // …
}
```

## What you get for free

| Symbol                    | From                                | Used by              |
|---------------------------|-------------------------------------|----------------------|
| `Example`                 | `EntityToModels` (server) / hand (wasm) | every crate     |
| `ExampleCreate` / `Update` / `List` | `EntityToModels`          | server, db           |
| `ExampleRepo` (vox trait) | `EntityToModels(generate_vox_service)` | server, clients   |
| `ExampleRepoStorage`      | `EntityToModels`                    | server               |
| `ExampleEntity` / `Column` | `DeriveEntityModel`                | server, db           |
| `ExampleService` (vox trait) | hand-written                     | server impl, clients |
| `ExampleServiceClient`    | `#[vox::service]` macro             | wasm + desktop       |
| `ExampleServiceDispatcher` | `#[vox::service]` macro            | server               |

## Repo vs Service — when to use which

- **Repo** = mechanical CRUD. The auto-generated trait. Clients that
  are essentially admin tools (data inspector, migrations dashboard,
  raw editor) can call it directly. New entity → free CRUD.
- **Service** = domain operations. Hand-written. Always the surface
  your end-user-facing client (web, desktop, iOS) calls. Composes
  one or more repos plus validation, auth, audit, events.

If you find yourself adding many methods to the service that are
"call repo, return result", consider whether the client should just
talk to the repo directly. The service exists so that *business
rules* live in one place, not so that everything passes through it.

## Crate graph

```
                    example-proto
                  /  (wasm-clean)  \
                 /                  \
        example-db (server)          \
              ↑                       \
        apps/server  →  vox WS  →   apps/web (wasm)
        apps/db          ↑          apps/desktop (native)
                         |              ↑
                         └─ both use ExampleServiceClient
                            from example-proto
```

## Adding a new entity

1. New crate `crates/<entity>-proto/` mirroring `example-proto`.
2. New crate `crates/<entity>-db/` mirroring `example-db` (entity
   re-exports + migrations).
3. Register both in workspace `Cargo.toml`.
4. Migrate the SQLite schema: `just migrate`.
5. Implement `<Entity>Service` in `apps/server`.
6. Mount the service dispatcher on the vox factory.
7. Frontend imports the proto crate and calls `<Entity>ServiceClient`.
