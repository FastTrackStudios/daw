+++
title = "Testing strata"
description = "Native unit, native integration, browser e2e — which lives where."
weight = 40
+++

architect has three test layers that exercise the same contract from
progressively heavier setups.

## Layer 1: native, in-process

`features/<feature>/tests/native/`

Drives the auto-generated `<T>Repo` trait against an in-memory backend.
No socket, no server, no async runtime beyond `tokio::test`. Sub-second
test runs. Use this for everything that's about the *contract* — sort
ordering, validation errors, payload-shape exclusions, etc.

```rust
#[tokio::test]
async fn list_sorted_by_name_ascending() {
    let r = ExampleRepoMemory::new();
    for n in ["charlie", "alpha", "bravo"] {
        r.create(ExampleCreate { name: n.into(), description: String::new() })
            .await.unwrap();
    }
    let page = r.list(
        Page { index: 0, size: 100 },
        Some(Sort { field: "name".into(), order: SortOrder::Asc }),
        None,
    ).await.unwrap();
    let names: Vec<_> = page.items.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
}
```

Run with `cargo test -p example-tests-native`.

## Layer 2: native, real server

`apps/<app>/tests/e2e/`

Spawns the full axum + vox stack on an OS-assigned port, then drives
it from a vox-core/vox-websocket client over native TCP. Validates the
whole transport-and-dispatcher pipeline without involving a browser.

This is where you put tests that span features (auth + multi-row
updates + permission checks together).

Run with `cargo test -p app-tests-e2e`.

## Layer 3: real browser, real server

`features/<feature>/tests/web/`

`wasm-bindgen-test` crate. Spawned in headless Firefox via
`wasm-bindgen-test-runner`. Loads the wasm module, opens a real
WebSocket against a running `app-server`, and exercises
`<T>RepoClient::create / list / get / delete`.

This is the test that proves "wasm-clean" is more than aspirational —
every byte goes through facet encoding on a real transport, into a real
database, and back.

Run with `just test-e2e` (sqlite-backed server) or `just test-e2e-memory`
(in-memory backend). Both run the same three browser tests.

## What goes where

| Test concern | Layer |
|--------------|-------|
| Trait method behavior, edge cases | 1 — native, in-process |
| Validation errors, sort ordering | 1 |
| Cross-feature integration | 2 — native, real server |
| Auth + permission flows | 2 |
| Wire format compatibility | 3 — browser |
| Wasm32 build correctness | 3 |
| End-user latency / UX | dx serve + manual |
