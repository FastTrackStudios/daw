# Task

**Local-first. Realtime. Collaborative. Multiplayer. Extensible.**

A workspace for building cross-domain apps that *feel native*, work
*offline*, sync *instantly*, and never lock you into a vendor. Every
domain — projects, time tracking, invoicing, inventory, recipes,
agent chat, calendar — is a self-contained feature you can use
together or strip out, all written in Rust + Dioxus.

## What the words mean here

- **Local-first.** The user's data lives on the user's device. Every
  feature stores its source of truth in a [Loro](https://loro.dev/)
  CRDT document. Edits work offline; the server is a sync relay, not
  an authority.
- **Realtime.** Edits propagate in milliseconds over a WebSocket. Open
  two tabs on the same route and watch them stay in lockstep.
- **Collaborative / Multiplayer.** No "save" button. No conflict
  dialogs. CRDTs merge concurrent edits deterministically; the UI just
  reflects current state.
- **Extensible.** Every domain is a separate workspace member with a
  consistent shape: `*-proto` (wire types), `*-crdt` (Loro source of
  truth), `*-db` (SeaORM persistence), `*-ui` (Dioxus components).
  Adding a feature is mechanical; removing one is a directory delete.
  External integrations (Hermes-agent for AI dispatch, GitHub webhooks
  for PR linking, CalDAV for calendar sync, Anthropic/OpenAI/Ollama
  for chat models) plug into trait-shaped seams without touching the
  core.

## UI rules

**All UI components must be compatible with the theming system.**
This is non-negotiable.

- Build on `fts-ui` primitives (`Button`, `Card`, `Sheet`, `Dialog`,
  `Combobox`, `Sidebar`, etc.). Avoid hand-rolled equivalents unless
  there's a specific reason fts-ui can't cover the case — and then
  fix it upstream in fts-ui rather than working around it.
- Use **theme tokens** for color: `bg-background`, `text-foreground`,
  `bg-card`, `border-border`, `bg-primary`, `text-muted-foreground`.
  Never hardcode `bg-slate-*` or hex colors. The token values come
  from the active preset; switching preset (or flipping dark mode)
  must change the whole app's appearance without component edits.
- **Dark mode is the default.** Components must look correct in both
  light and dark with no `dark:` overrides — the CSS variables flip
  values per mode and your component just consumes them.
- **Two-tier theming.** Each *organization* picks a preset (default,
  violet-bloom, supabase, t3-chat, neo-brutalism, etc.). Each
  *project* can optionally override its org's theme. This is wired
  via `fts_ui::ThemeProvider` at the App root and `ThemeScope` inside
  the project route. New theme-aware surfaces should respect both
  tiers — don't bypass the provider.
- **Dumb components.** Feature `*-ui` crates own no state: data in,
  events out via `EventHandler<T>`. The route layer (in `task-ui`)
  wires repos to components. This keeps components portable across
  web/desktop/mobile and reusable in storybooks.

When a component you need doesn't exist in fts-ui, prefer:
1. Compose it from existing fts-ui primitives, or
2. Add it to fts-ui upstream (the workspace dep is a path checkout,
   so edits propagate immediately).

## Architecture in 30 seconds

```
features/<name>/
  <name>-proto/    architect-derive wire types (#[derive(Entity)])
  <name>-crdt/     EntityCrdt impl + <Name>RepoLoro (Loro source of truth)
  <name>-db/       SeaORM persistence (crdt-seaorm tables + projections)
  <name>-ui/       dumb Dioxus components — fts-ui only, theme-aware
  <name>/          facade crate with feature gates (vox / server / fake / full)
  spec/<name>.md   tracey spec rules
  tests/native/    Repo trait + replica-convergence tests

apps/server         task-server: WebSocket sync relay, webhook receivers,
                    SeaORM persistence, integration registry boot
apps/web/desktop    Dioxus platform launchers; thin shells over task-ui
apps/db             standalone migrator + seeder

crates/task-ui      Dioxus app shell, AppShell/Sidebar/router,
                    per-feature routes
crates/task-cli     CLI commands
```

The auth schema is the only state that isn't local-first — sessions,
credentials, and OAuth tokens are server-authoritative
(`features/auth/`). Everything else flows through Loro and syncs via
the WebSocket relay.

External integrations sit behind trait seams in `agent-proto`:
- `AgentIntegration` (`hermes`, `mock`) for task dispatch + agent runs
- `ChatModel` (`mock`, future: `anthropic` / `openai` / `ollama`) for
  conversational completion
- GitHub webhooks (PR → task status, commit ↔ branch linking)
- CalDAV bidirectional sync for the calendar feature

Each integration is a separate crate registered at server boot; the
trait surface is stable so adding `openai` or `linear` plugins
doesn't touch the rest of the codebase.

## Quick start

```bash
# Enter the dev shell (direnv loads it automatically on cd).
# Manual: nix develop .#ui

# Terminal 1 — the sync relay + webhook server
just server                   # listens on :9090, pre-seeded fake data

# Terminal 2 — the Dioxus dev server
just web                      # listens on :8765, hot-reload

# Or both in one process:
just dev

# Open http://localhost:8765 and try any of: /, /inbox, /projects-live,
# /chat-ai, /calendar, /timer, /invoice, /inventory, /agents/runs.
# Open a second tab on the same route — edits propagate instantly.
```

## Common recipes

```bash
just check         # cargo check --workspace
just build         # cargo build --workspace
just test          # cargo test --workspace
just fmt           # cargo fmt --all
just clippy        # cargo clippy --workspace --all-targets -- -D warnings
just ci            # fmt --check + clippy + nextest run
```

## Adding a feature

The scaffolder lives in `xtask`. Typical flow:

1. `cargo xtask new-feature <name>` writes the proto/crdt/db/ui/parent
   crates with the right `Cargo.toml`s and a placeholder entity.
2. Fill in the entity shape in `<name>-proto/src/lib.rs`.
3. Codec the fields in `<name>-crdt/src/lib.rs` (mirror the cookbook
   pattern).
4. Build the dumb components in `<name>-ui/src/lib.rs` using
   `fts_ui::prelude::*` and theme tokens.
5. Wire the route in `crates/task-ui/src/feature_routes/<name>.rs`
   and register it in `crates/task-ui/src/app.rs`.

Existing feature trios are the best reference — pick one whose shape
matches yours and adapt.

## Status

Active development. Demo data is seeded server-side on every boot;
nothing here is persisted across cold starts unless
`SYNC_DEMO_DATABASE_URL=sqlite://./data.db?mode=rwc` is set.

## License

Dual-licensed under either of

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
