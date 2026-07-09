//! The `Example` entity — one struct, one source of truth.
//!
//! The [`architect::Entity`] derive turns this single definition into the
//! whole data surface:
//!
//! - the wire struct `Example` (with `facet::Facet`) plus the
//!   `ExampleCreate` / `ExampleUpdate` / `ExampleList` payload types,
//! - the `ExampleRepo` `#[vox::service]` CRUD trait (→ the auto-generated
//!   `ExampleRepoClient` you call from the browser) and an
//!   `ExampleRepoLayer` token so the repo composes through the layer
//!   system (`layers![…]` / `.into_router()`),
//! - on server builds (`--features server-seaorm`), the SeaORM `Model` +
//!   `Entity` + `Column` + `Relation` + `ActiveModel`, plus
//!   `ExampleRepoStorage<C>` implementing the repo against a SeaORM
//!   connection.
//!
//! No `cfg_attr` on the struct, no parallel definitions, no manual `From`
//! impls — architect emits the storage↔wire bridge.

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[cfg_attr(feature = "fake", derive(::fake::Dummy))]
#[derive(architect::Entity, ::facet::Facet, Clone, Debug, PartialEq)]
#[architect(
    // SeaORM table this entity persists to. Architect emits the
    // `#[sea_orm(table_name = "examples")]` decoration internally.
    table_name = "examples",
    // Emit the `ExampleRepo` vox trait + the server-side
    // `ExampleRepoStorage<C>` impl. Most entities want this; opt out
    // by removing this flag if you're using a custom storage layer.
    repo,
    // Also emit the client state layer (gated on this crate's `atom` +
    // `vox` features): the optimistic `ExampleStore`, the typed data
    // hooks (`use_example`, `use_example_list`), and the optimistic
    // `ExampleMutations` — everything a Dioxus page needs to `match`.
    store,
    // Also emit typed form bindings (gated on `form`):
    // `ExampleCreateFields` / `ExampleUpdateFields` — one validated
    // `Field` per payload field, `submit()` returning the typed payload.
    form,
    // Also emit the live-event story: `ExampleEvent`, the `ExampleEvents`
    // subscribe trait, the `ExampleEvented<R>` publish-through wrapper
    // (server wraps its backend once; `into_router()` mounts CRUD + the
    // event feed together), and the `use_example_events()` client hook
    // that folds pushed changes into the store — live pages.
    events,
)]
pub struct Example {
    /// Stable identifier. `on_create = Uuid::new_v4()` runs the
    /// expression server-side when inserting a new row.
    #[architect(primary_key, auto_increment = false, on_create = Uuid::new_v4())]
    pub id: Uuid,

    /// Free-text label. `filterable` adds query-param support on the
    /// repo's `list` method; `fulltext` joins it into FTS5 search.
    #[architect(filterable, sortable, fulltext)]
    pub name: String,

    /// Longer description. Searchable via fulltext as well. The generated
    /// form field accepts empty input (`form(optional)`); `name` above
    /// stays required, the form default.
    #[architect(filterable, fulltext, form(optional))]
    pub description: String,

    /// Audit timestamp. Excluded from create/update payloads (clients
    /// can't set it); populated by the macro's storage layer on insert.
    #[architect(exclude(create, update), on_create = Utc::now())]
    pub created_at: DateTime<Utc>,

    #[architect(exclude(create, update), on_create = Utc::now(), on_update = Utc::now())]
    pub updated_at: DateTime<Utc>,
}
