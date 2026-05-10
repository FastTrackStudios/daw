# Hermes Integration Layer Implementation Plan

> **For Hermes:** Use `subagent-driven-development` to implement this plan task-by-task.

**Goal:** Add a self-contained integration layer that lets the task system delegate work to [Hermes Agent](https://github.com/NousResearch/hermes-agent) profiles via Hermes's multi-agent Kanban (introduced in v0.12.0, hardened in v0.13.0). A user can mark a task as agent-eligible, the integration mirrors it to `~/.hermes/kanban.db` as a Kanban card, a Hermes profile claims it and works in an isolated worktree, and the integration syncs status + comments back into the task system.

**Architecture:**
Side-car module behind `feature = "hermes"`. The core `Task` entity is unchanged — delegation metadata lives in the existing polymorphic `properties: JsonObject` column under a `hermes` key. The integration owns its own service trait, its own SQLite adapter for `kanban.db`, its own background sync worker, and its own CLI subcommand group. Source of truth stays in the task system for *what to do* (title, description, deps); Hermes is authoritative for *progress* (status transitions, comments, time).

**Tech Stack:**
Rust, Facet, Vox, SeaORM (separate connection to Hermes's `kanban.db`), `notify` for inotify-based WAL watching, `task-cli` / `task-server`. No Python — we read/write `kanban.db` directly rather than calling a Hermes HTTP API.

---

## Design Rules

1. **Side-car, not core.** Hermes integration lives in its own crate (`crates/task-hermes/`) gated by a feature flag. Core compiles and runs without Hermes installed.
2. **Properties, not columns.** Delegation state goes into `task.properties.hermes = {profile, kanban_id, last_synced_at, ...}`. No schema migration on the Task entity.
3. **Schema-pinned to a Hermes version.** The adapter knows which Hermes Kanban schema version it understands. On startup the sync worker probes and refuses to run against an unknown version (logged warning, skipped — not crashed).
4. **Two columns of truth.** Task→Kanban: title, description, dependencies, priority. Kanban→Task: status, comments, time entries, final output. Conflicts on either column are decided by their owning side.
5. **Idempotent delegation.** Card IDs derive deterministically from `task.id` — re-running delegate on the same task is a no-op upsert, not a duplicate card.
6. **Polling first, watching later.** v1 polls `kanban.db` every N seconds. v2 can swap in inotify on the WAL file when motivated.
7. **No Hermes runtime dependency.** Task system never spawns Hermes processes. The user runs Hermes themselves; we just read/write its DB.

---

## Target End State

### Delegation flow
- mark a task as agent-eligible: `task task add --delegate research-triage --title "..."` or `task hermes delegate <task-ref> --profile <name>`
- card appears in Hermes Kanban, claimed by the named profile
- profile works in an isolated worktree per its own Hermes config
- on completion, the card's final state (output, status, comments) flows back to the task as comment threads + time entries
- `task task show <ref>` displays the linked Kanban card status inline when the integration is active

### Sync semantics
- newly-delegated task → upserted Kanban card with deterministic ID `task-system:<task.id>`
- task title/description/deps changes → propagated to the Kanban card on next sync tick
- Kanban card status transitions (`ready → claimed → in_progress → blocked → done`) → mirrored to `task.properties.hermes.kanban_status` and surfaced in `task task show`
- Kanban card comments → appended as threaded comments on the task (using the existing comment system from the threaded-conversations plan)
- Kanban card final output → appended as a "result" comment with provenance metadata
- card archived in Kanban → task closed with a closure reason indicating Hermes provenance

### Profile + connection management
- `task hermes register --kanban-db <path>` configures a `HermesConnection` row
- `task hermes profiles list` reads available profiles from `~/.hermes/profiles/` (or wherever Hermes stores them — schema probe at register time)
- per-`project_type` default profile mapping (e.g. `cooking` → `recipe-research`, `audio-production` → `mix-review`) so common delegations don't need an explicit `--profile`
- multi-tenant: `--tenant <namespace>` for the optional Hermes tenant namespace field

### Observability
- `task hermes status` shows: connection health, last sync tick time, count of in-flight delegations, recent failures
- `task hermes log <task-ref>` tails the Kanban card's run history (Hermes records this in `kanban.db`)
- standard task-server health endpoint includes a `hermes_connection: HealthCheck` substatus

---

## Out of Scope (v1)

- Bidirectional comment sync (comment on a task → appear as Kanban comment). v1 only flows kanban→task. v2 adds the reverse.
- Dependency-graph mirroring (task `blocked_by` → kanban `link`). v1 is one-way for delegation only; v2 mirrors the dep DAG so Hermes can plan around blocking work.
- Project-context exposure (Hermes profiles read recipes/routines/glossary as working memory). Compelling but requires a stable "context bundle" format.
- Multi-Hermes-instance fanout. v1 supports one `HermesConnection` row at a time; multi-host comes later.
- Real-time WAL watching. v1 is poll-driven (every 5 s by default, configurable).

---

## Implementation Beads

### Bead 1 — `task-hermes` crate scaffold + `HermesConnection` entity

Create the crate, gate behind `feature = "hermes"`, add `HermesConnection` (deterministic UUID per kanban_db path; one row per Hermes install). Migration. ServerContext factory. Repo arm. Capability listing. CLI: `task hermes register / list / unregister`.

### Bead 2 — `kanban.db` adapter (read-only first)

SeaORM entity definitions for Hermes's Kanban tables (probe schema, document the version we target). Read-only methods: `list_cards`, `get_card_by_id`, `read_card_comments`, `list_profiles`. Live test against a real `~/.hermes/kanban.db` (or a fixture pulled from Hermes's repo).

### Bead 3 — Delegation push (one-way: task → kanban)

`HermesService` trait with `delegate_task(task_id, profile, tenant?) -> Result<KanbanCardId>`. Writes a card with deterministic `task-system:<task.id>` id. Upsert semantics. Stamps `task.properties.hermes` with `{profile, kanban_id, delegated_at}`. CLI: `task hermes delegate <task-ref> --profile <name>`. `task task add --delegate <profile>` shortcut.

### Bead 4 — Sync worker (poll-driven kanban → task)

`HermesSyncWorker` runs in `task-server` as a tokio task. Every 5 s (configurable via `HermesConnection.poll_interval_seconds`), enumerates delegated tasks, fetches each card's current state, applies diffs:
- card status → `task.properties.hermes.kanban_status`
- new card comments → new task comments (threaded under a Hermes-attributed thread root)
- card final output → "result" comment with provenance
- card archived → task close with reason
Idempotent on every tick (deterministic comment IDs derived from `kanban_card_id + comment_index`).

### Bead 5 — `task task show` rendering

When a task has `properties.hermes`, render an inline summary in `task task show` (CLI + JSON path): profile name, kanban status, last-sync timestamp, comment count. Color-code status (claimed=cyan, in_progress=yellow, blocked=red, done=green).

### Bead 6 — Demo seed + e2e test

Demo seed adds two delegated tasks (one in each of two project types) for `organization = "personal"`. Seed creates a fixture `kanban.db` if Hermes isn't available, so the e2e test runs deterministically. New flow in `cli_e2e.rs::cli_e2e_golden_paths`: `flow_hermes_delegate_and_sync` exercises the delegate → wait-for-sync → show-task path.

### Bead 7 — Schema version probing + graceful degradation

On `HermesConnection` register and on each sync worker startup, query Hermes's schema version table. If the version is newer than what we target, log a warning and skip sync (the integration goes dormant rather than crashing). Document the supported Hermes version range in the crate's README. Bump as needed.

---

## File Layout

```
crates/task-hermes/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── connection/
│   │   ├── mod.rs
│   │   └── model.rs          # HermesConnection entity
│   ├── adapter/
│   │   ├── mod.rs
│   │   ├── schema.rs         # Hermes Kanban schema (read-only mirror)
│   │   └── version.rs        # schema version probe
│   ├── service/
│   │   ├── mod.rs            # HermesService trait + request/response structs
│   │   └── impl.rs           # HermesServiceImpl
│   ├── sync/
│   │   ├── mod.rs
│   │   └── worker.rs         # HermesSyncWorker tokio task
│   └── properties.rs         # task.properties.hermes shape + helpers
└── tests/
    ├── adapter_read.rs
    ├── delegation_push.rs
    ├── sync_worker.rs
    └── fixtures/
        └── kanban-v0.13.db   # checked-in fixture for offline tests

crates/task-cli/src/commands/hermes.rs   # task hermes <subcommand>

apps/server/tests/cli_e2e.rs             # add flow_hermes_delegate_and_sync
```

---

## Open Questions

1. **Hermes schema stability.** How often does `kanban.db` schema change between Hermes minor versions? If frequently, we may need a thin adapter shim per Hermes release. Mitigation: keep the adapter narrow (only the columns we actually need), pin to a tested rev range.
2. **Comment attribution.** When syncing Hermes comments back as task comments, how should we attribute them? Options: a synthetic `hermes:<profile-name>` author, the Hermes profile's configured display name, or the underlying LLM model identifier. Defer to bead 4 design.
3. **Time entries from runs.** Hermes records run start/end timestamps. Should those become task `time_entries` automatically? Pro: complete visibility into delegated work. Con: time entries get noisy fast for chatty agents. v1 default off, configurable per `HermesConnection`.
4. **Worktree visibility.** Each Hermes profile may work in its own git worktree. Should the task system surface that worktree path in `task task show` so a human can review the work-in-progress? Tracked as v2.
5. **Failure modes.** What happens when a delegated task's Hermes profile crashes / hits a stale-agent timeout? Hermes auto-blocks the card; sync surfaces this as a blocked task with the failure reason as a comment. User can `task hermes retry <task-ref>` to reset. Make sure bead 4 handles the retry path.

---

## Success Criteria

- `cargo build --workspace --features hermes` succeeds; `cargo build --workspace` (without the feature) also succeeds and produces a binary that has no Hermes code linked.
- `task hermes register --kanban-db ~/.hermes/kanban.db` followed by `task task add --delegate research-triage --title "test delegation"` lands a card in the real Hermes Kanban, the named profile claims it, and within a few sync ticks the task's `properties.hermes.kanban_status` reflects the card's current state.
- `cli_e2e.rs::flow_hermes_delegate_and_sync` passes against a fixture `kanban.db`.
- Hermes integration stays silent when not configured — no logs, no startup overhead, no behavior change for users who don't run Hermes.
