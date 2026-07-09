//! The `Note` entity — the **collaborative** counterpart to [`Example`].
//!
//! Where `Example` is server-owned (optimistic store, reconcile-or-
//! rollback), `Note` lives in a CRDT replica: `#[architect(crdt)]` emits
//! the `EntityCrdt` codec, the Loro-backed `NoteRepoLoro`, and the
//! replica-backed hooks (`use_note_crdt_list`, `use_note_crdt_actions`).
//! Every client holds the full doc — writes are instant and offline-
//! capable, peers' edits merge in live over `crdt::sync`.
//!
//! [`Example`]: crate::Example

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The one shared collaboration boundary of the example app: server and
/// every client open the same doc id, `crdt::sync` keeps them converged.
/// Real apps mint one doc per project/workspace/session instead.
pub const COLLAB_DOC_ID: Uuid = uuid::uuid!("8d8ac610-566d-4ef0-9c22-186b2a5ed793");

#[cfg_attr(feature = "fake", derive(::fake::Dummy))]
#[derive(architect::Entity, ::facet::Facet, Clone, Debug, PartialEq)]
#[architect(
    // The root container key inside the shared CrdtDoc (and the table
    // name, were a SQL backend mounted instead).
    table_name = "notes",
    // The NoteRepo trait — NoteRepoLoro implements it, so the same
    // contract serves in-process use and plain RPC reads if wanted.
    repo,
    // The CRDT story: EntityCrdt codec + NoteRepoLoro + the replica-
    // backed hooks. Gated on this crate's `crdt` feature (hooks
    // additionally on `atom`).
    crdt,
    // Typed form bindings for the create flow, same as Example.
    form,
)]
pub struct Note {
    #[architect(primary_key, auto_increment = false, on_create = Uuid::new_v4())]
    pub id: Uuid,

    /// The note body. Required in the generated form.
    #[architect(sortable, fulltext)]
    pub text: String,

    /// Who wrote it — free-form display name in this demo.
    #[architect(sortable, form(label = "Author"))]
    pub author: String,

    #[architect(exclude(create, update), on_create = Utc::now())]
    pub created_at: DateTime<Utc>,

    #[architect(exclude(create, update), on_create = Utc::now(), on_update = Utc::now())]
    pub updated_at: DateTime<Utc>,
}
