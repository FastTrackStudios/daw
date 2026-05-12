+++
title = "Reference"
description = "Crate map, derive attributes, and macro outputs."
weight = 30
+++

## Crate map

| Crate | Path | Role |
|-------|------|------|
| `architect` | `macros/architect` | User-facing — re-exports the derive + runtime helper types. |
| `architect-derive` | `macros/architect-derive` | The proc-macro itself. |
| `example` | `features/example/example` | Facade for the example feature. |
| `example-proto` | `features/example/example-proto` | Wire contract — `#[derive(architect::Entity)]` lives here. |
| `example-db` | `features/example/example-db` | SeaORM/SQLite implementation. |
| `example-memory` | `features/example/example-memory` | In-memory implementation. |
| `example-ui` | `features/example/example-ui` | Per-feature Dioxus components. |
| `example-tests-native` | `features/example/tests/native` | Native cargo tests. |
| `example-tests-web` | `features/example/tests/web` | Browser tests (wasm-bindgen-test). |
| `app-server` | `apps/app/server` | axum + vox runtime. |
| `app-db` | `apps/app/db` | sea-orm-migration CLI. |
| `app-ui` | `apps/app/ui` | Runtime shell — composes feature-ui crates. |
| `app-web` | `apps/app/web` | Dioxus web (wasm32) binary. |
| `app-desktop` | `apps/app/desktop` | Dioxus desktop binary. |
| `app-tests-e2e` | `apps/app/tests/e2e` | Native end-to-end test scaffold. |

## Cargo features

| Crate | Feature | Effect |
|-------|---------|--------|
| `architect` | `server` | Re-exports the SeaORM-side storage helpers (`DbConn`). |
| `architect` | `server-axum` | Pulls in axum + adds `architect::axum_ws` (Link + `serve` helper). |
| `example` | `backend-db` | Re-export `example_db::*` at `example::backend_db::*`. |
| `example` | `backend-memory` | Re-export `example_memory::*` at `example::backend_memory::*`. |
| `example-proto` | `server` | Forward to `architect/server` so the derive emits the SeaORM bits. |
| `app-server` | `backend-db` (default) | Build with SeaORM/SQLite. |
| `app-server` | `backend-memory` | Build with in-memory storage. |

## Vox dependency convention

`vox = { default-features = false, features = ["runtime"] }` is the
wasm-clean baseline used in every `*-proto` and shared library crate.
Native servers add `transport-websocket` if they go through
`vox::serve` directly (architect's bundled `axum_ws` adapter doesn't
need it). Wasm test/client crates depend on `vox-core` and
`vox-websocket` directly via `[target.'cfg(target_arch = "wasm32")']`.

## What the derive emits

See [The architect pattern](@/architecture/pattern.md) for the full
list of fields and the input → output mapping.
