# FastTrackStudio + Cooking Platform Implementation Plan

> **For Hermes:** Use `subagent-driven-development` to implement this plan task-by-task.

**Goal:** Turn Task into a platform kernel with two first-class domain packs: a FastTrackStudio planner pack for live/recording production and a Cooking pack that syncs with Nextcloud Cookbook and related recipe sources.

**Architecture:**
Task keeps the shared primitives: tasks, projects, calendar, time, people, external references, files, and workflows. Domain packs layer on top of those primitives without duplicating them: FastTrackStudio adds session/setlist/input/stage/patch abstractions, while Cooking adds recipes, meal plans, pantry, shopping, and prep workflows. Nextcloud remains the primary sync surface for recipes and shared content; the cookbook adapter should normalize whatever Cookbook exposes into Task-native recipe objects and workflows.

**Tech Stack:**
Rust, Facet, Vox, Nextcloud/WebDAV/CalDAV/Deck, markdown/YAML frontmatter, existing `task-core` workflow modules, `task-cli`, and the `integrations/` adapter tree.

---

## Design Rules

1. **Task remains the kernel.** Every domain object must degrade to core Task primitives where possible.
2. **Packs are additive.** A pack can add custom schema, commands, workflows, and sync adapters, but not a parallel source of truth.
3. **Nextcloud is the main bridge.** FastTrackStudio should use Nextcloud files/CalDAV/Deck for collaboration, and Cooking should integrate with Nextcloud Cookbook when available.
4. **Workflow-first.** Specialized apps should be generated from workflows and templates, not hard-coded UI state.
5. **Interop over silos.** FastTrackStudio and Cooking must both emit tasks, calendar events, time entries, and external refs.

---

## Target End State

### FastTrackStudio pack
- event/session templates
- setlists
- input lists
- stage plots
- patch/channel maps
- cue sheets / show flow
- linked REAPER/Samply/session assets
- venue and client-facing deliverable bundles

### Cooking pack
- recipes
- meal plans
- prep sessions
- pantry / inventory
- shopping lists
- leftovers / batch-cooking tracking
- integration with Nextcloud Cookbook as an upstream/downstream source

### Shared platform capabilities
- pack manifest + registry
- workflow DSL and execution log
- dry-run / explain mode
- sync adapters for Nextcloud Cookbook and other sources
- CLI and agent commands for pack inspection, sync, and workflow runs

---

## Task 1: Add a pack/runtime core in `task-core`

**Objective:** Introduce the shared abstractions that every domain pack uses.

**Files:**
- Create: `crates/task-core/src/packs/mod.rs`
- Create: `crates/task-core/src/packs/manifest.rs`
- Create: `crates/task-core/src/packs/registry.rs`
- Modify: `crates/task-core/src/lib.rs`
- Modify: `crates/task-core/src/workflows/mod.rs`
- Modify: `crates/task-core/src/service.rs` if pack-aware Vox service methods are added

**Step 1: Write failing tests**

Add tests that assert:
- a pack manifest can be serialized/deserialized with Facet
- the registry can list built-in packs
- a pack can declare capabilities such as `tasks`, `calendar`, `files`, `sync`, and `workflows`

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core pack
```
Expected: fail because pack modules/types do not exist yet.

**Step 3: Write minimal implementation**

Add a small manifest model such as:
```rust
#[derive(Debug, Clone, Default, Facet)]
pub struct PackManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<PackCapability>,
    #[facet(default)]
    pub workflows: Vec<WorkflowSpec>,
}
```

**Step 4: Run tests to verify pass**

Run the focused `task-core` tests again. Expect pack manifest and registry tests to pass.

**Step 5: Commit**

```bash
git add crates/task-core/src/packs crates/task-core/src/lib.rs crates/task-core/src/workflows/mod.rs
```

---

## Task 2: Model FastTrackStudio as a first-class pack

**Objective:** Add the domain types and file conventions for studio planning.

**Files:**
- Create: `crates/task-core/src/workflows/fasttrackstudio.rs`
- Modify: `crates/task-core/src/workflows/mod.rs`
- Modify: `crates/task-core/src/lib.rs`
- Create: `integrations/fasttrackstudio/README.md`

**Step 1: Write failing tests**

Add tests that prove the pack can represent:
- a live show or recording session
- a setlist with ordered songs
- an input list with sources and monitor notes
- a stage plot with positions and backline requirements
- a patch list / channel map with inputs and outputs

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core fasttrackstudio
```

**Step 3: Write minimal implementation**

Model the pack around objects that can already map back to existing `Event`, `Setlist`, `StagePlot`, and `InputList` types.

Add a small adapter projection layer so a FastTrackStudio session can emit:
- a project folder
- a task checklist
- calendar events for rehearsal/show times
- `ExternalRef` links for REAPER/Samply/session assets

**Step 4: Run tests to verify pass**

Run the focused pack tests and then:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core
```

**Step 5: Commit**

```bash
git add crates/task-core/src/workflows/fasttrackstudio.rs crates/task-core/src/workflows/mod.rs crates/task-core/src/lib.rs integrations/fasttrackstudio/README.md
```

---

## Task 3: Model Cooking as a first-class pack

**Objective:** Add the shared recipe, meal plan, pantry, and shopping abstractions.

**Files:**
- Create: `crates/task-core/src/workflows/cooking.rs`
- Create: `crates/task-core/src/workflows/recipe.rs`
- Modify: `crates/task-core/src/workflows/mod.rs`
- Modify: `crates/task-core/src/lib.rs`
- Create: `integrations/cooking/README.md`

**Step 1: Write failing tests**

Add tests for:
- recipe parsing/serialization
- meal plan generation from recipes
- shopping list generation from planned meals
- pantry depletion / leftovers tracking
- mapping a recipe or meal plan into tasks and calendar blocks

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core cooking
```

**Step 3: Write minimal implementation**

Start with a compact schema:
```rust
#[derive(Debug, Clone, Default, Facet)]
pub struct Recipe {
    pub title: String,
    #[facet(default)]
    pub ingredients: Vec<Ingredient>,
    #[facet(default)]
    pub steps: Vec<String>,
    pub source: Option<ExternalRef>,
}
```

**Step 4: Run tests to verify pass**

Run the focused `task-core` tests and confirm serialization plus workflow projections pass.

**Step 5: Commit**

```bash
git add crates/task-core/src/workflows/cooking.rs crates/task-core/src/workflows/recipe.rs crates/task-core/src/workflows/mod.rs crates/task-core/src/lib.rs integrations/cooking/README.md
```

---

## Task 4: Build a Nextcloud Cookbook adapter

**Objective:** Add an integration layer that can import/export recipes from Nextcloud Cookbook.

**Files:**
- Create: `integrations/nextcloud/cookbook/README.md`
- Create: `integrations/nextcloud/cookbook/src/lib.rs`
- Create: `integrations/nextcloud/cookbook/src/client.rs`
- Create: `integrations/nextcloud/cookbook/src/sync.rs`
- Modify: `integrations/README.md`
- Modify: `integrations/nextcloud/README.md`

**Step 1: Write failing tests**

Tests should cover at least:
- importing a recipe into the normalized Task recipe model
- exporting a Task recipe back to the Cookbook adapter format
- preserving recipe titles, ingredients, tags/categories, instructions, and URLs
- handling missing or partial Cookbook fields without data loss

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core recipe
```

**Step 3: Write minimal implementation**

Prefer a normalized adapter boundary:
- core `Recipe` type in `task-core`
- Cookbook-specific client/mapper under `integrations/nextcloud/cookbook`
- support WebDAV/file-backed import first if Cookbook API details are limited

**Step 4: Run tests to verify pass**

Run adapter tests plus a core roundtrip test.

**Step 5: Commit**

```bash
git add integrations/nextcloud/cookbook crates/task-core/src/workflows/recipe.rs crates/task-core/src/workflows/cooking.rs
```

---

## Task 5: Add workflow DSL and runner hooks

**Objective:** Make the pack system executable instead of only descriptive.

**Files:**
- Create: `crates/task-core/src/workflows/trigger.rs`
- Create: `crates/task-core/src/workflows/action.rs`
- Create: `crates/task-core/src/workflows/runner.rs`
- Modify: `crates/task-core/src/workflows/mod.rs`
- Modify: `crates/task-core/src/service.rs`
- Modify: `crates/task-core/src/service_impl.rs`

**Step 1: Write failing tests**

Add tests for:
- task-created trigger
- date/time trigger
- manual run
- dry-run planning output
- idempotent run keys / dedupe

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core workflow
```

**Step 3: Write minimal implementation**

Support a minimal DSL with:
- triggers
- conditions
- actions
- run history
- explain mode

**Step 4: Run tests to verify pass**

Run the focused workflow tests and then the full `task-core` suite.

**Step 5: Commit**

```bash
git add crates/task-core/src/workflows crates/task-core/src/service.rs crates/task-core/src/service_impl.rs
```

---

## Task 6: Expose pack and workflow commands in the CLI

**Objective:** Make packs and workflow runs visible to users and agents.

**Files:**
- Modify: `crates/task-cli/src/main.rs`
- Create: `crates/task-cli/src/commands/pack.rs`
- Create: `crates/task-cli/src/commands/workflow.rs`
- Modify: `crates/task-cli/src/commands/mod.rs` if introduced
- Modify: `apps/server/tests/remote_doctor.rs` for remote coverage if needed

**Step 1: Write failing tests**

Add CLI tests that assert:
- `task pack list` shows FastTrackStudio and Cooking packs
- `task pack show <id>` prints manifest details
- `task workflow run --dry-run` returns a plan instead of mutating state

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-cli
```

**Step 3: Write minimal implementation**

Add CLI entrypoints that call the Vox services or local service implementation.

**Step 4: Run tests to verify pass**

Run the task-cli suite and a small remote smoke test.

**Step 5: Commit**

```bash
git add crates/task-cli/src/main.rs crates/task-cli/src/commands
```

---

## Task 7: Add end-to-end smoke tests and docs

**Objective:** Verify the new pack system works across local and remote modes.

**Files:**
- Modify: `apps/server/tests/remote_doctor.rs`
- Modify: `docs/self-host.md`
- Modify: `ARCHITECTURE.md`
- Modify: `integrations/README.md`
- Modify: `integrations/nextcloud/README.md`
- Create: `docs/plans/2026-05-03-fasttrackstudio-cooking-platform.md` if this plan is being preserved separately from implementation notes

**Step 1: Write failing tests**

Add smoke tests for:
- pack listing over Vox
- FastTrackStudio session projection over Vox
- Cooking recipe import/export over Vox
- Cookbook sync dry-run with no host Nextcloud leakage

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test --workspace
```

**Step 3: Write minimal implementation**

Wire the new docs and smoke coverage to the actual pack/runtime behavior.

**Step 4: Run tests to verify pass**

Run the focused tests first, then the whole workspace suite.

**Step 5: Commit**

```bash
git add apps/server/tests/remote_doctor.rs docs/self-host.md ARCHITECTURE.md integrations/README.md integrations/nextcloud/README.md
```

---

## Recommended build order

1. Pack/runtime core
2. FastTrackStudio pack
3. Cooking pack
4. Nextcloud Cookbook adapter
5. Workflow runner
6. CLI exposure
7. E2E tests/docs

---

## Success Criteria

- A FastTrackStudio project can express a full session with setlist, inputs, stage plot, and deliverables.
- A Cooking project can express recipes, meal plans, pantry, and shopping workflows.
- Nextcloud Cookbook can roundtrip recipes into and out of Task’s normalized model.
- Packs can trigger workflows and generate task/calendar outputs.
- The CLI can inspect and run pack/workflow operations locally and remotely.

---

## Notes / Assumptions

- If Nextcloud Cookbook exposes a stable API, use it directly.
- If the Cookbook API is limited or awkward, use a normalized import/export adapter over WebDAV/files first and add deeper API support later.
- Keep any specialized FastTrackStudio or Cooking UI out of the integration layer; UI belongs in a separate client or plugin.
- Prefer keeping pack data in markdown/YAML files so it stays compatible with Task’s file-first model.
