# Hermes Integration Layer Implementation Plan

> **For Hermes:** Use `subagent-driven-development` to implement this plan task-by-task.

**Goal:** Add a self-contained integration layer that lets the task system delegate selected Task-owned work items to [Hermes Agent](https://github.com/NousResearch/hermes-agent) profiles via Hermes's multi-agent Kanban. A user can choose which Task tasks are agent-delegated; Task mirrors those tasks into Hermes Kanban boards as execution cards; Hermes profiles work them; Task continuously reconciles Hermes execution signals back into Task-owned task state, history, comments, and run records.

**Architecture:**
Task is the system of record. It owns projects, tasks, dependencies, statuses, notes, execution history, and all canonical IDs in its own store. Hermes Kanban is **not** Task's backend; it is an integration surface / execution mirror. The Hermes integration is a side-car module behind `feature = "hermes"` with its own service trait, sync adapter, background worker, and CLI subcommand group. Delegation metadata lives in `task.properties.hermes` as foreign-system linkage (`board_slug`, `kanban_task_id`, sync cursors, profile, last observed run IDs). Hermes events are imported as external execution observations; Task applies them according to Task's own reconciliation policy.

**Tech Stack:**
Rust, Facet, Vox, SeaORM/SQLite for Task's own store, and a Hermes adapter that can start with Hermes CLI/WebUI-compatible API calls or a narrow schema-pinned SQLite reader/writer where necessary. Prefer public/stable Hermes surfaces (`hermes kanban ...`, WebUI API, or a future Hermes library API) over treating `kanban.db` as a permanent backend contract. `task-cli` / `task-server` expose the Task-side control plane.

---

## Design Rules

1. **Task owns everything.** Task's own store is canonical for projects, tasks, dependencies, statuses, notes, execution attempts, and long-term history. Hermes data is external integration state.
2. **Explicit delegation.** Only tasks the user marks for agent work are mirrored to Hermes Kanban. Ordinary Task tasks never silently appear in Hermes.
3. **Side-car, not core.** Hermes integration lives in its own crate (`crates/task-hermes/`) gated by a feature flag. Core compiles and runs without Hermes installed.
4. **Foreign IDs, not primary IDs.** Delegation state goes into `task.properties.hermes = {connection_id, board_slug, kanban_task_id, profile, last_synced_at, cursors, ...}`. Task IDs remain canonical; Hermes IDs are foreign references.
5. **Task reconciliation policy wins.** Task→Kanban publishes delegated task briefs, dependencies, priority, assignee/profile hints, and desired state. Kanban→Task imports observations: worker status, comments, run summaries, metadata, logs, and failure/block reasons. Task decides how imported observations affect canonical Task status.
6. **Idempotent delegation.** Re-running delegate on the same Task task updates the existing mirrored Hermes card rather than creating duplicates. Store an explicit mapping and, where Hermes supports it, an idempotency key derived from `task.id`.
7. **Public API first; schema pinning only as fallback.** Prefer Hermes CLI/WebUI/API-compatible operations. If v1 reads/writes `kanban.db` directly, keep the adapter narrow, probe schema/version, and treat it as a replaceable transport detail.
8. **Polling first, watching later.** v1 polls at a configurable interval. v2 can add event/WAL watching or WebUI/gateway event streams when motivated.
9. **No hidden Hermes runtime dependency.** Task should not require Hermes to be running for normal project/task management. Hermes is only needed for delegation sync/agent execution.

---

## Target End State

### Delegation flow
- mark a task as agent-eligible: `task task add --delegate research-triage --title "..."` or `task hermes delegate <task-ref> --profile <name>`
- card appears in Hermes Kanban, claimed by the named profile
- profile works in an isolated worktree per its own Hermes config
- on completion, the card's final state (output, status, comments) flows back to the task as comment threads + time entries
- `task task show <ref>` displays the linked Kanban card status inline when the integration is active

### Sync semantics
- newly delegated Task task → mirrored Kanban card with stored foreign mapping and idempotency key derived from `task.id`
- Task title/description/deps changes → propagated to the Kanban card on next sync tick if the task remains delegated
- Kanban card status transitions (`ready → running → blocked → done`) → imported into `task.properties.hermes.last_observed_status` and optionally mapped to Task status by explicit reconciliation rules
- Kanban card comments → imported as Hermes-attributed Task comments or execution observations, with source IDs/cursors so sync is idempotent
- Kanban card run summaries/metadata/log pointers → imported into Task-owned execution history
- card archived/deleted in Kanban → recorded as an external event; Task does **not** automatically delete or close canonical tasks unless the configured policy says so

### Profile + connection management
- `task hermes register --kanban-db <path>` or `task hermes register --api <url>` configures a `HermesConnection` row
- `task hermes profiles list` reads available profiles from Hermes through the selected transport when possible
- `task hermes boards list` discovers Hermes boards, and `task hermes board link <task-project> <board-slug>` records which Hermes board mirrors delegated work for a Task project
- multi-tenant: `--tenant <namespace>` for the optional Hermes tenant namespace field

### Observability
- `task hermes status` shows: connection health, last sync tick time, count of in-flight delegations, recent failures
- `task hermes log <task-ref>` tails the Kanban card's run history (Hermes records this in `kanban.db`)
- standard task-server health endpoint includes a `hermes_connection: HealthCheck` substatus

---

## Out of Scope (v1)

- Bidirectional comment sync (comment on a task → appear as Kanban comment). v1 can import Hermes observations first; v2 adds Task→Hermes human comments once conflict/attribution rules are clear.
- Full dependency-graph mirroring. v1 can mirror only direct dependencies for delegated tasks; v2 mirrors enough of the Task DAG for Hermes to respect larger project blocking constraints.
- Making Hermes the canonical project/task database. Task must remain usable and complete without Hermes.
- Project-context exposure (Hermes profiles read recipes/routines/glossary as working memory). Compelling but requires a stable "context bundle" format.
- Multi-Hermes-instance fanout. v1 supports one `HermesConnection` row at a time; multi-host comes later.
- Real-time WAL watching. v1 is poll-driven (every 5 s by default, configurable).

---

## Hermes WebUI Reference Notes

Hermes's Kanban WebUI/API treats boards as the user-facing integration boundary: board list, active board, columns, cards, assignees, config, events, task logs, task patching, comments, bulk operations, and dispatch nudging are exposed as board-scoped operations in current Hermes docs/skills. Task should mirror that shape rather than treating Hermes as a hidden database:

- Task project/workstream links to a Hermes board slug.
- Task delegated item links to a Hermes card/task ID plus sync cursors.
- Task UI can show Hermes-like execution columns as an integration view, but the cards remain Task tasks.
- Dispatch controls are explicit: selecting “delegate to agent” publishes/updates the Hermes mirror; it does not move all Task work into Hermes.
- Imported Hermes runs/events/logs become Task execution evidence.

Starcommand inspection note from 2026-05-10: the relevant Hermes install for Task-server integration is the Starcommand `agent` WebUI service, not THEBATTLESHIP's local Hermes dashboard. On Starcommand, `hermes-webui` listens on `127.0.0.1:12490` and exposes live Kanban JSON endpoints with no dashboard token required on loopback:

```text
GET /api/kanban/boards
GET /api/kanban/board
GET /api/kanban/stats
GET /api/kanban/config
GET /api/kanban/assignees
GET /api/kanban/events
```

Observed board response shape includes `boards[].{slug,name,description,icon,color,db_path,is_current,counts,total}`, with the default board backed by `/home/agent/.hermes/kanban.db`. `GET /api/kanban/board` returns `columns[]` where each column has `name` and `tasks[]`; task objects include `id`, `title`, `body`, `assignee`, `status`, `priority`, `created_by`, timestamps, `workspace_kind`, `workspace_path`, `tenant`, `result`, `idempotency_key`, failure fields, `current_run_id`, `skills`, `link_counts`, and `comment_count`. This API is the best near-term integration reference for Task's Hermes adapter.

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
