//! Hand-written data hooks — the ones the `store` derive *can't* know.
//!
//! `#[architect(store, events)]` on the entity already derives the
//! CRUD-shaped state layer (`use_example`, `use_example_list`,
//! `ExampleMutations`, the live event subscription, the store). What's
//! left here is feature-specific:
//!
//! - [`use_examples`] — the search-aware list (merges the repo's `list`
//!   with the hand-written `ExampleService::search` by query).
//! - [`use_example_live`] — a refreshable, store-bypassing single read
//!   (the `use_async` demo).
//!
//! Both follow the same shape the derive emits: pull the app's single
//! `Connection<Caller>`, guard with `ready_or_pending!`, build the typed
//! client from the shared caller (zero extra connections), keep errors
//! typed.

use architect::vox::Caller;
use architect::{
    Async, AtomResult, ClientError, ConnectionState, Id, RepoError, ready_or_pending,
    try_use_reactivity, use_async, use_connection, use_store_list,
};
use dioxus::prelude::*;
use example::architect::{Page, Sort, SortOrder};
use example::{
    EXAMPLE_REACTIVITY_KEY, Example, ExampleClientError, ExampleRepoClient, ExampleServiceClient,
    ExampleServiceError, use_example_store,
};
use uuid::Uuid;

/// The list (or search results) — store-backed, stale-while-revalidate,
/// typed. An empty query reads the repo's `list`; anything else goes
/// through the service's full-text `search`. Tracks the entity's
/// reactivity key, so settled mutations re-fetch it.
pub fn use_examples(
    query: Signal<String>,
) -> AtomResult<Vec<(Id<Uuid>, Example)>, ExampleClientError> {
    let conn = use_connection::<Caller>();
    let store = use_example_store();
    let reactivity = try_use_reactivity();

    use_store_list(store, move || async move {
        if let Some(r) = reactivity {
            r.track(EXAMPLE_REACTIVITY_KEY);
        }
        let caller = ready_or_pending!(conn);
        let q = query();
        if q.trim().is_empty() {
            Some(
                ExampleRepoClient::new(caller)
                    .list(
                        Page {
                            index: 0,
                            size: 100,
                        },
                        Some(Sort {
                            field: "name".into(),
                            order: SortOrder::Asc,
                        }),
                        None,
                    )
                    .await
                    .map(|l| l.items)
                    .map_err(ClientError::from),
            )
        } else {
            Some(
                ExampleServiceClient::new(caller)
                    .search(q, 100)
                    .await
                    .map_err(|e| relabel_service_error(ClientError::from(e))),
            )
        }
    })
}

/// One example, fetched live from the server (no store, no optimism) and
/// **refreshable** — `.state()` is the phase, `.refresh()` re-runs it.
pub fn use_example_live(id: String) -> Async<Example, ExampleClientError> {
    let conn = use_connection::<Caller>();
    use_async(move || {
        let id = id.clone();
        async move {
            let caller = match conn.state() {
                ConnectionState::Ready(c) => c,
                ConnectionState::Connecting => {
                    return Err(ClientError::Connect("still connecting…".into()));
                }
                ConnectionState::Failed(e) => return Err(ClientError::Connect(e)),
            };
            let uuid: Uuid = id
                .parse()
                .map_err(|e| ClientError::App(RepoError::InvalidInput(format!("bad id: {e}"))))?;
            ExampleRepoClient::new(caller)
                .get(uuid)
                .await
                .map_err(ClientError::from)
        }
    })
}

/// Re-label the service's typed error into the entity's client error so
/// search results and repo reads share one store error type. The variants
/// map one-to-one; infrastructure arms pass through.
pub(crate) fn relabel_service_error(e: ClientError<ExampleServiceError>) -> ExampleClientError {
    match e {
        ClientError::App(svc) => ClientError::App(match svc {
            ExampleServiceError::NotFound => RepoError::NotFound,
            ExampleServiceError::InvalidInput(s) => RepoError::InvalidInput(s),
            ExampleServiceError::Internal(s) => RepoError::Internal(s),
        }),
        ClientError::Connect(s) => ClientError::Connect(s),
        ClientError::Transport { detail, retryable } => {
            ClientError::Transport { detail, retryable }
        }
    }
}
