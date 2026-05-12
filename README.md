# architect

A facet-native, vox-friendly entity framework for Rust. One
`#[derive(Entity)]` on a plain struct, and you get:

- A wasm-safe wire struct (with `facet::Facet`) usable from any
  client crate (Dioxus web, Dioxus desktop, future iOS via FFI).
- `<Entity>Create` / `<Entity>Update` / `<Entity>List` payload types.
- An auto-generated `<Entity>Repo` `#[vox::service]` trait — typed
  RPC over WebSocket, no JSON hand-rolling.
- Under `--features server`: the full SeaORM `Model` + `Entity` +
  `Column` + `Relation` + `ActiveModel`, plus a
  `<Entity>RepoStorage<C>` that implements the repo trait against a
  SeaORM connection.

```rust
#[derive(architect::Entity)]
#[architect(table_name = "examples", repo)]
pub struct Example {
    #[architect(primary_key, on_create = Uuid::new_v4())]
    pub id: Uuid,

    #[architect(filterable, sortable, fulltext)]
    pub name: String,

    #[architect(exclude(create, update), on_create = Utc::now())]
    pub created_at: DateTime<Utc>,
}
```

That's the whole entity. No `cfg_attr`. No parallel struct in another
crate. No manual `From<Model> for Example` glue.

## Layout

```
macros/
  architect/         the user-facing crate (re-exports the derive + runtime traits)
  architect-derive/  the proc-macro crate

crates/
  example-proto/  uses #[derive(architect::Entity)] — wasm-clean by default
  example-db/     pulls example-proto with --features server, owns the migrations
  example-ui/     shared Dioxus components (consumes example-proto over vox)

apps/
  web/            Dioxus web (wasm) — talks to apps/server via vox
  desktop/        Dioxus desktop — same UI as web
  server/         axum + vox; implements the ExampleService alongside the auto-gen repo
  db/             sea-orm-migration CLI
```

The `apps/` and `crates/example-*` directories are the **reference
example**. Read them to learn the pattern, then template them when
spinning up a new project.

## Why facet-only

vox uses facet for its wire encoding. By dropping serde derives
entirely, the wire format is one cohesive system — every architect
type is automatically Facet-able, which means vox can transport it
without any per-type glue. No parallel `serde` derives, no
`#[serde(rename = …)]` mismatches with `#[architect(...)]`.

## Status

Scaffold + macro entry point. The derive currently emits the wire
struct only; the SeaORM Model + storage emission lands in subsequent
commits. See `docs/pattern.md` for the design notes that drive the
emission shape.
