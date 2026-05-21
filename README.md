# Task

**Local-first. Realtime. Collaborative. Multiplayer. Extensible.**

A workspace for building cross-domain apps that *feel native*, work
*offline*, sync *instantly*, and never lock you into a vendor. Every
domain — projects, time tracking, invoicing, inventory, recipes,
agent chat, calendar — is a self-contained feature you can use
together or strip out, all written in Rust + Dioxus.

## The product

Task is organized around four top-level surfaces plus a per-org
operations panel. Each surface answers a distinct question.

### 📥 Inbox — *"what's on my mind right now, and have I processed it?"*

The capture-and-review surface. Two jobs:

1. **Capture** — fleeting notes, half-formed thoughts, links saved
   for later, daily reflections, dreams, prompts. Personal,
   private, messy by design.
2. **The temporal contract** — Inbox enforces the discipline that
   captured notes get *processed* on a schedule. Every fleeting
   note has a review SLA: a card you've ignored for too long
   surfaces as overdue. Processing means either promoting it
   (fleeting → atomic → maybe Wiki), linking it into a Project,
   archiving it, or deleting it. The Inbox is where you uphold the
   contract that says "anything I capture, I will return to."

Notes mature outward: fleeting → atomic note → consolidated Wiki
entry. Inbox notes link **out** to anything — Projects, Contacts,
Wiki entries. They're the source of the personal graph.

### 📚 Wiki — *"what is known?"*

A consolidated, depersonalized knowledge base. Facts, references,
explanations, definitions, how-tos. Built as an
[LLM-Wiki](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f) —
LLM reads source documents and incrementally maintains an
interconnected encyclopedia. Same shape Obsidian uses (Markdown +
YAML frontmatter + `[[wikilinks]]`), so the wiki dir works as an
Obsidian vault if you ever want to leave.

**The Wiki is a graph sink.** Other surfaces can link to it; it
cannot link out. The wiki can link within itself. This isolation
is deliberate — it keeps the encyclopedia clean enough to publish
(see Quartz integration) and prevents personal context from
leaking into shared knowledge. Promotion to the Wiki is a
conscious step, not an accidental tangle.

Wiki pages do **show** incoming links — a "referenced by" panel
lists tasks, project docs, inbox notes, and contacts that cite
this entry. Backlinks are queries against the rest of the graph,
not edges authored *by* the wiki page, so the wiki's source files
stay publishable in isolation.

### 📁 Projects — *"what am I building / doing?"*

Where Task shines. One pillar holds every project — personal,
work, side, archived — filterable by the organization switcher
("Personal", "FastTrackStudio", "Client X", …). Each project owns:

- Its own task list (with kanban, gantt, agent dispatch)
- Its own notes / spec docs / meeting logs
- Its own attachments
- Its own time + invoice records
- Its own subset of contacts (collaborators, clients, vendors)
- Its own **custom workflows** — status sets, kanban columns,
  automation, agent dispatch rules — defined per-project (or
  inherited from the org template).

**Crucially, a project's data lives where its actual artifacts
live** — alongside the code repo, the design files, the
spreadsheets. Task isn't a centralized knowledge silo you sync
into; it's the *editing surface for the data that's already where
your work lives*. Move a project, it's a folder move. Archive it,
it's a folder archive. Share it, push the folder to a collaborator.

### 👥 Contacts — *"who matters?"*

A CalDAV-synced relationship manager. Standard address-book fields
plus the interaction graph: which projects this person touched,
which meetings you've had, what they said, what you owe them,
upcoming gift / compliment / check-in reminders. Designed to help
you be *intentionally* attentive — the application equivalent of
keeping notes on people you care about.

Contacts link bidirectionally with Projects (collaborators) and
Inbox (mentions, reflections about people).

### 📨 Comms — *"what's in flight, and where does it belong?"*

The communications surface: email, chat, project threads, DMs.
Same triage shape as Inbox — incoming messages get processed and
either linked to a project, archived, snoozed, or deleted — but
the data lives differently from the other pillars because
communications are inherently **multi-party**.

**Why this surface looks different from the others:**

- **Not markdown files.** Messages carry too much structure
  (sender, recipient, thread parent, timestamps, read state,
  delivery state, attachments, encryption envelopes), too much
  volume (100k+ message archives), and too many simultaneous
  participants for plain files to handle. Storage is structured
  tables on the server (SeaORM / postgres in prod), with vox
  streaming to clients and a local read-through cache for offline
  access.
- **Server-authoritative.** Email comes from external systems
  (IMAP/SMTP). Chat needs real-time push with multi-party
  participation. Audit logs need legal-grade immutability. These
  break the local-first model — and that's correct, because the
  data belongs partly to the *other* people in the conversation.
- **Federated, not platformed.** The reconciliation with the
  guiding constraint: the server is *infrastructure*. You can
  self-host it. Data is exportable in open formats (mbox for
  email, Matrix/MLS for chat). The client always has a local
  replica of your view. No vendor lock-in — same model as running
  your own SMTP server.

**Capabilities:**

- **Email client** — IMAP/SMTP per-org accounts, unified inbox
  across accounts, project-link rules (auto-tag incoming mail by
  sender / subject / project membership), compose / reply /
  forward. Attachments route through the existing attachments
  feature.
- **Chat / threads** — every project gets a `#general` thread by
  default; add more as the project grows. Cross-project DMs for
  collaborator conversations that don't fit one project. Real-time
  via the existing vox WebSocket relay. Threading, reactions, read
  receipts, presence.
- **External bridges** — adapters for Matrix (federated), and
  optionally Slack / Discord / Teams (work accounts) so messages
  in those systems can be triaged into the same surface.
- **Search at scale** — full-text + recency-weighted ranking
  (tantivy in v1, embeddings for semantic search later).

**Triage flow** — the same temporal contract as Inbox, applied to
in-flight communications:

1. Incoming mail / chat lands in Comms with no project assignment.
2. The triage view surfaces overdue items honestly (no badges, no
   shame — just "these need a decision").
3. Triage actions: link to existing project, create new project,
   archive, delete, snooze.
4. Once linked, the message appears in that project's
   conversation panel — alongside its tasks, notes, and time
   entries. The full conversation history is one query.

**Permissions + audit:**

- Per-project ACL: who can read this project's conversation log.
- Per-thread ACL: implicit from the original to/cc + explicit
  grants.
- Every access is recorded in an append-only audit log. "Who read
  what, when" survives any future dispute.
- Linking a message to a project is itself a logged metadata
  event — visible to anyone with project access.

Comms threads through every other pillar: Contacts (the people in
the conversation), Projects (where the conversation belongs),
Inbox (capture of insights mid-conversation), Wiki (citations to
canonical references), Goals (which goal does this thread serve?).

### 🎯 Goals — *"am I building the life I said I wanted?"*

Goals aren't a separate pillar — they're **Projects with horizon
metadata and a charter**. The goal-as-project pattern means
everything you've already built (tasks, notes, financial tracking,
contacts, attachments) composes for free; the goal layer just adds
the spine that connects today's work to your future self.

Each goal-project carries:

- **Horizon** — `today / week / month / quarter / year / 5-year /
  10-year / life`. A project can sit at any level; long-horizon
  goals contain shorter-horizon sub-projects.
- **Charter** — one of the project's notes captures *why* this
  matters, what success looks like, the cost (financial / time /
  opportunity), and the contingency ("if not by date X, then Y").
  The charter is what makes the goal survive setbacks.
- **Financial target** — optional `target_amount` + `target_date`
  pulled from Operations.Finance. The system computes the monthly
  rate needed and shows progress against it.
- **Sub-projects** — a 5-year goal decomposes into a tree of
  shorter projects. "Buy a house" branches into "build credit,"
  "save down payment," "research neighborhoods," each with their
  own milestones.
- **Supporting habits** — habits linked to a goal answer the
  question they're for. Skipping a habit prompts an honest review:
  *is this still serving Goal X?* — making the trade-off explicit
  rather than letting it drift.

Examples:

> **Buy a car** *(1-year, $30k target)*
> Charter: why this car, why now, what it replaces.
> Sub-projects: research models, save monthly, sell current vehicle.
> Tasks: visit dealerships, get preapproval, test drive list.

> **Buy a house** *(5-year, $100k down payment)*
> Charter: target neighborhood, family timeline, mortgage tolerance.
> Sub-projects: build credit (1yr), save down payment (5yr),
> research neighborhoods (2yr), search → offer → close (year 5).
> Habits: monthly savings rate, weekly listings review.
> Contacts: spouse, realtor, loan officer.

### ⚙️ Operations — *per-organization business utilities*

Separate from the main nav because the concerns are different.
Where the 4 surfaces are about *thinking and doing*, Operations is
about *running the business*:

- **Time** — tracking, weekly/monthly/yearly reports, billable vs.
  non-billable, per-project rollups.
- **Invoicing** — generate invoices from time entries + line items,
  email PDFs, track paid/outstanding.
- **Finance** — income/expense ledger, category breakdowns,
  per-project P&L, tax-year snapshots.
- **Inventory** — locations + physical things they hold. Studios,
  warehouses, home offices, storage units; the gear, instruments,
  furniture, supplies inside each; restock triggers; assignment to
  active projects ("this mic is in Studio B for the next session").

These live in an Operations panel reachable from the org switcher,
not from the main nav — they're tools, not surfaces.

### 🔍 Lenses — *cross-cutting views over the pillars*

Lenses are **perspectives, not silos**. Each lens aggregates data
that already lives in the pillars and presents it through a domain-
specific UI. You can add new lenses without adding pillars or
duplicating data.

**Built-in lenses:**

- **📅 Calendar** — time-axis view of anything dated: task
  due/scheduled, project milestones, time entries, contact
  birthdays + follow-ups, wiki entries with `date:` frontmatter.
  CalDAV bidirectional sync interoperates with your existing
  calendar app.
- **🗺 Map** — location-axis view: inventory locations (studios,
  warehouses), project venues, contact addresses, meeting points.
- **🕸 Graph** — link-axis view: the wiki internal graph, the
  cross-pillar reference graph, backlinks panels per entity.
- **🔁 Habits** — recurrence-pattern view: which behaviors you
  committed to, what the last 30/90/365 days looked like, what
  goals they serve. **No streaks, no gamification** — see the
  guiding constraint below. The view surfaces honest information
  (12 of 30 days, last gap 4 days), and skipped habits route to
  Inbox for review.
- **💪 Training** — workout aggregation: PR graphs, volume per
  muscle group, program adherence, deload signals. Built from
  tasks tagged with `workout:` structured data.
- **🍳 Meals** — meal-planning view: week grid, prep-day visibility,
  shopping-list generator (`project meal plan` minus
  `pantry inventory` = grocery list), pantry-aware recipe
  suggestions, expiry prompts.
- **🎯 Goals** — horizon pyramid (life → 10yr → 5yr → year →
  quarter → month → week → today). Up-link from any task ("what
  goal does this serve?") and down-link from any goal ("what am I
  doing this week toward this?"). Drift detection surfaces stale
  goals to Inbox for re-charter or drop.
- **💰 Finance dashboard** — category trends, per-project P&L,
  goal-savings progress, cash-flow projection.

**Custom lenses:** any user can define a new lens as a *query +
layout*. "Reading log," "Journaling streaks," "Gardening,"
"Apartment hunt" — none of these need new code or new pillars.
A lens is a saved cross-pillar query rendered through a chosen
visualization (list, grid, calendar, map, graph, kanban, gantt).

This is what makes the architecture scale: **new life domains
compose** the existing pillars through new lenses. They don't
demand new silos.

### Why this shape

Most knowledge tools either *centralize everything* (Obsidian =
one vault, Notion = one workspace) or *scatter into silos* (loose
folders, separate apps for tasks vs. notes vs. contacts). Both
modes have failure cases:

- **Centralized**: every piece of knowledge tangles with every
  other. The encyclopedia becomes unsharable because your fleeting
  notes leaked into it. Migration is a database operation.
- **Siloed**: context-switching between "where I work" and "where
  I write about my work." Forgetting where you put something.

Task's bet is *project-colocated data with a one-way wiki promotion
path*. Your project notes live with the project. Your personal
capture lives in your inbox. The wiki only grows by intentional
promotion. The system is designed around the natural maturation
of ideas (capture → personal note → consolidated fact) rather
than around storage convenience.

### The guiding constraint

**Software meant to improve / augment your life, not consume your
life.**

Every design decision passes through this filter. Concretely:

- **Output beats input.** Features must produce visible life
  improvement (less forgotten work, better relationships, time
  reclaimed). Capture time that doesn't return value is dead
  weight. If a feature mostly grows the app's content without
  changing how the user lives, it's a smell.
- **In-and-out, not all-day.** Get information in fast, get
  answers out fast. Long sessions inside the app are a failure
  mode. Workflows route you back to the world.
- **Push to the world, not pull into the app.** Calendar entries
  sync out via CalDAV. Notes live as files on disk you can edit
  in any editor. Contacts federate. Task is a lens over your
  existing life-data, not a destination that owns it.
- **No engagement loops.** No streaks, no gamification, no
  daily-active-user hooks. The temporal contract is a real
  obligation surfaced honestly — "you've ignored these 12 notes
  for 3 weeks, here they are" — not a habit-trap dressed up as
  productivity.
- **Quiet by default.** Notifications only when they carry signal
  the user would want acted on (a contact's birthday tomorrow,
  a deadline approaching, a payment overdue). Never for
  attention-fishing.
- **Owns nothing, federates everything.** Your data is plain
  files in standard formats. Markdown, YAML frontmatter,
  iCalendar, vCard. You can stop using Task at any time and your
  data continues to work in every other tool that reads those
  formats.
- **Local-first is an ethic, with one honest exception.** No
  telemetry, no cloud dependency, no subscription lock-in. The
  sync relay you self-host or skip entirely. The exception is
  Comms: emails and chats involve other people's data, so the
  server is system-of-record rather than relay — but it's still
  *your* server (self-hostable), in open formats (mbox, Matrix),
  with a full local replica. Federated infrastructure, not
  platform lock-in.

The scope is *entire life management* — projects, knowledge,
relationships, communications, time, money, things, places. The
constraint is that all of it has to serve the life it's managing,
not become it.

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
