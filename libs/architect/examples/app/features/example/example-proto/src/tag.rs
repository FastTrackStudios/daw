//! The `Tag` entity — the **String-primary-key** counterpart to
//! [`Example`].
//!
//! Where `Example` mints a server-side `Uuid` (`on_create =
//! Uuid::new_v4()`), `Tag` is keyed by a caller-supplied `slug: String`
//! — the shape of wiki page ids, booking references, and any other
//! human-readable identifier. The derive threads the pk type through
//! every emission, so nothing here is Uuid-specific:
//!
//! - the wire types carry `slug: String` (`TagCreate` includes it —
//!   no `on_create`, the caller picks the id),
//! - `TagRepo` methods take `id: String`,
//! - `TagEvent::Deleted(String)` and the `TagEvented<R>` wrapper
//!   broadcast String ids,
//! - the client store keys rows by `String` (`StoreEntity::Key =
//!   String`) and the optimistic mutations clone the key where a
//!   `Copy` id would have been copied.
//!
//! The pk only needs `Clone + Eq + Hash + FromStr` (with a
//! `Display`able parse error) — `String` qualifies, `Copy` is not
//! required.
//!
//! [`Example`]: crate::Example

use chrono::{DateTime, Utc};

#[cfg_attr(feature = "fake", derive(::fake::Dummy))]
#[derive(architect::Entity, ::facet::Facet, Clone, Debug, PartialEq)]
#[architect(
    table_name = "tags",
    // The TagRepo vox trait + (on server builds) TagRepoStorage<C>.
    repo,
    // The client state layer: TagStore, use_tag / use_tag_list, and the
    // optimistic TagMutations — all keyed by the String slug.
    store,
    // The live-event story: TagEvent / TagEvents / TagEvented<R> + the
    // use_tag_events() client hook. Deleted events carry the String id.
    events,
    // Typed form bindings. The slug itself is a required create field.
    form,
)]
pub struct Tag {
    /// Caller-supplied, human-readable identifier (e.g. `"rust-lang"`).
    /// No `on_create` expression: the slug travels in `TagCreate`.
    #[architect(primary_key, auto_increment = false)]
    pub slug: String,

    /// Display label.
    #[architect(filterable, sortable, fulltext)]
    pub label: String,

    #[architect(exclude(create, update), on_create = Utc::now())]
    pub created_at: DateTime<Utc>,

    #[architect(exclude(create, update), on_create = Utc::now(), on_update = Utc::now())]
    pub updated_at: DateTime<Utc>,
}
