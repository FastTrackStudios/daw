//! Example feature UI — what's left to hand-write once the entity derives
//! its own client state layer.
//!
//! `#[architect(store)]` on `Example` (in `example-proto`) already emits
//! the CRUD-shaped state: the optimistic `ExampleStore` + its hooks, the
//! typed data hooks (`use_example`, `use_example_list`), and the
//! optimistic `ExampleMutations` — re-exported here so pages have one
//! import root. This crate adds only what the derive can't know:
//!
//! - [`data`] — the search-aware list ([`use_examples`]) and the
//!   refreshable live read ([`use_example_live`]).
//! - [`mutations`] — the service-specific verb ([`ExampleActions`] /
//!   `duplicate`).
//! - [`components`] — dumb presentation: `ExampleCard`, `ExampleRow`,
//!   `ExampleList`, `SearchBar`, and the `forms`.
//!
//! Navigation is *not* here — the shell composes these with normal Dioxus
//! `Link` / `nav`. State management is the `AtomResult` phases the hooks
//! return; the shell `match`es them.

pub mod components;
pub mod data;
pub mod mutations;

pub use components::{
    ExampleCard, ExampleCreateForm, ExampleEditForm, ExampleList, ExampleRow, SearchBar,
};
pub use data::{use_example_live, use_examples};
pub use mutations::{ExampleActions, use_example_actions};

// The derived client-state layer, re-exported from the proto crate so
// shells import everything example-shaped from one place.
pub use example::{
    EXAMPLE_REACTIVITY_KEY, ExampleClientError, ExampleMutations, ExampleStore, provide_example,
    provide_example_store, use_example, use_example_events, use_example_list,
    use_example_mutations, use_example_store,
};
