# Task

Local-first multiplayer task management built on the
[architect](https://git.starcommand.live/codywright/architect)
framework. Every domain (projects, comments, invoices, recipes,
workouts, …) is a self-contained feature trio of `<name>-proto`
(wire types), `<name>-crdt` (Loro-backed source of truth), and
`<name>-db` (SeaORM persistence), with a matching `<name>-ui`
(Dioxus components) for the browser.

Two browser tabs on the same route sync edits in real time
through a tiny WebSocket relay (`apps/sync-demo`). Server-side
auth lives in its own `features/auth/` feature.

## Quick start

```bash
# Enter the dev shell (rust toolchain + dx + wasm32 + node + tailwind)
nix develop

# In one terminal: the Loro sync relay
just sync-demo-server         # listens on :9090, sqlite-backed

# In another terminal: the Dioxus dev server
just task-web-dev             # listens on :8765, hot-reload

# Open two browsers at http://localhost:8765/timer (or /assets,
# /projects-live, /invoice, …) and watch edits propagate.
```

## Repo layout

```
features/<name>/
  <name>-proto/    architect-derive wire types (#[derive(Entity)])
  <name>-crdt/     EntityCrdt impl + <Name>RepoLoro newtype (Loro)
  <name>-db/       SeaORM persistence (crdt-seaorm tables + projections)
  <name>-ui/       dumb Dioxus components (List + Row + Form)
  <name>/          facade with feature gates (vox / server / fake / full)
  spec/<name>.md   tracey spec rules
  tests/native/    Repo trait + replica-convergence tests

apps/server         legacy task-server (auth + sync route, deprecated endpoints)
apps/sync-demo      Loro WebSocket relay + SeaORM snapshot/update store
crates/task-ui      Dioxus shell, hosts feature routes
crates/task-cli     CLI commands (still on the legacy task-core path)
crates/task-core    legacy entity definitions (slated for retirement)
```

## Architecture

See `ARCHITECTURE.md` for the design doc and `VISION.md` for the
product story.

The auth schema is the only state that isn't local-first — sessions,
account credentials, and OAuth tokens are server-authoritative
(`features/auth/auth-db/`). Everything else flows through Loro and
syncs via the WebSocket relay.

## Common recipes

```bash
just check         # cargo check --workspace
just build         # cargo build --workspace
just test          # cargo test --workspace
just fmt           # cargo fmt --all (+ excluded UI crates)
just clippy        # cargo clippy --workspace --all-targets
just ci            # check + fmt --check + clippy + nextest
```

## License

Dual-licensed under either of

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
