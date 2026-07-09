//! Hand-written mutations — the ones the `store` derive can't know.
//!
//! CRUD writes (`create` / `update` / `delete`) come derived as
//! `ExampleMutations` / `use_example_mutations`. What's left is the
//! service-specific verb: `duplicate`, which goes through the
//! hand-written `ExampleService` but drives the same optimistic store —
//! and the same single shared connection.

use architect::vox::Caller;
use architect::{ClientError, Connection, Mutation, use_connection, use_mutation};
use example::{
    EXAMPLE_REACTIVITY_KEY, Example, ExampleClientError, ExampleCreate, ExampleServiceClient,
    ExampleStore, use_example_store,
};
use uuid::Uuid;

use crate::data::relabel_service_error;

/// Service-backed optimistic actions for the example feature.
#[derive(Clone, Copy)]
pub struct ExampleActions {
    conn: Connection<Caller>,
    store: ExampleStore,
    dup: Mutation<ExampleClientError>,
}

/// Grab the feature's service-backed actions.
pub fn use_example_actions() -> ExampleActions {
    ExampleActions {
        conn: use_connection::<Caller>(),
        store: use_example_store(),
        dup: use_mutation().invalidating(&[EXAMPLE_REACTIVITY_KEY]),
    }
}

impl ExampleActions {
    /// Optimistically insert a copy of `id`, then reconcile against the
    /// server's duplicated row. No-op while the connection isn't up.
    pub fn duplicate(&self, id: Uuid, name: String, description: String) {
        let Some(caller) = self.conn.ready() else {
            return;
        };
        let client = ExampleServiceClient::new(caller);
        let draft = Example::draft(&ExampleCreate { name, description });
        self.dup.run(
            self.store,
            move |s| s.insert_optimistic(draft).0,
            move || async move {
                client
                    .duplicate(id, None)
                    .await
                    .map(Some)
                    .map_err(|e| relabel_service_error(ClientError::from(e)))
            },
        );
    }

    /// The last duplicate failure.
    pub fn error(&self) -> Option<ExampleClientError> {
        self.dup.error()
    }

    /// True while a duplicate is in flight.
    pub fn is_pending(&self) -> bool {
        self.dup.is_pending()
    }
}
