# Threaded Conversations, Mentions, and Review Comments Implementation Plan

> **For Hermes:** Use `subagent-driven-development` to implement this plan task-by-task.

**Goal:** Make comments a universal, threaded conversation system across tasks, projects, files, outputs, and review artifacts, with @mentions, timestamped comments, and Nextcloud-backed sync.

**Architecture:**
Task already has a comment entity and some task-body comment handling, but comments are not yet a truly universal collaboration layer. This plan turns comments into a first-class threaded conversation model with a shared target abstraction, mention extraction/notifications, and specialized renderers for files and media review. The source of truth remains markdown/YAML plus synced backing stores; the database is the operational index for thread lookup, mentions, notifications, and Nextcloud sync state.

**Tech Stack:**
Rust, Facet, Vox, SeaORM/task-db, Nextcloud WebDAV/Deck/possibly comments APIs, markdown frontmatter, timestamped review metadata, and the existing `task-cli` / `task-server` surfaces.

---

## Design Rules

1. **Comments are universal.** Any supported entity can accept threaded comments: task, project, file, output, event, session, recipe, meal plan, inventory item, etc.
2. **Threads are structured.** A comment can reply to another comment, and the tree is preserved in storage and in rendered views.
3. **Mentions are typed.** `@username` should resolve to a known person/agent/member when possible and create notifications/mentions records.
4. **Timestamped comments are first-class.** For audio/video/review items, comments may attach to a time range or exact timestamp.
5. **Nextcloud stays interoperable.** File comments and collaborative review comments should map cleanly onto Nextcloud-compatible storage/sync where possible.
6. **Task remains the kernel.** Conversation data should augment tasks/projects/files rather than becoming a parallel silo.

---

## Target End State

### Universal commenting
- add comment to task, project, file, output, or other entity
- reply to a comment
- resolve/reopen a thread
- mention one or more people or agents in any comment
- list comments as a thread tree
- filter by unresolved / mentions / author / time

### Review-specific commenting
- timestamped notes on audio/video/files
- ranged comments, not just point comments
- comment threads on review artifacts like mix versions, session files, or exports
- render timestamps as clickable links in CLI/UI

### Nextcloud-aware collaboration
- file comments backed by Nextcloud storage/sync where feasible
- imported comments from external sources preserve author, timestamp, thread parent, and resolution state
- comments on shared files can sync into the Task graph and back out again

---

## Task 1: Define a universal comment target model

**Objective:** Make comments attach to many entity types, not just tasks.

**Files:**
- Modify: `crates/task-db/src/entities/comment.rs`
- Modify: `crates/task-core/src/workflows/external.rs` if needed for file/review refs
- Modify: `crates/task-core/src/lib.rs`
- Modify: `crates/task-core/src/service.rs`
- Modify: `crates/task-core/src/service_impl.rs`
- Modify: `crates/task-db/src/migration/m20260412_000001_create_tables.rs` and/or a new migration if schema changes are needed

**Step 1: Write failing tests**

Add tests that prove a comment can target:
- task
- project
- file
- output
- review artifact

Tests should also verify:
- reply_to creates a thread
- entity_type / entity_id roundtrip correctly
- mentions are preserved

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-db comment
```
Expected: fail until the target model is expanded.

**Step 3: Write minimal implementation**

Add a typed target model such as:
```rust
pub enum CommentTarget {
    Task { id: Uuid },
    Project { id: Uuid },
    File { path: String },
    Output { id: Uuid },
    ReviewAsset { id: Uuid, kind: ReviewKind },
}
```

Map it onto the current DB entity fields in a backward-compatible way.

**Step 4: Run tests to verify pass**

Run the focused comment tests and then the relevant workspace package tests.

**Step 5: Commit**

```bash
git add crates/task-db/src/entities/comment.rs crates/task-db/src/migration crates/task-core/src/service.rs crates/task-core/src/service_impl.rs
```

---

## Task 2: Add threaded comment trees and query APIs

**Objective:** Store, fetch, and render comment threads in order.

**Files:**
- Modify: `crates/task-db/src/entities/comment.rs`
- Modify: `crates/task-core/src/service.rs`
- Modify: `crates/task-core/src/service_impl.rs`
- Modify: `crates/task-cli/src/main.rs`
- Create: `crates/task-core/src/comment_thread.rs` if a helper module is needed

**Step 1: Write failing tests**

Add tests for:
- retrieving all comments for an entity as a thread tree
- top-level comments ordered by timestamp
- replies nested under the correct parent
- unresolved threads filtering
- thread rendering for CLI output

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core thread
```

**Step 3: Write minimal implementation**

Provide a recursive thread builder that returns a tree or flat list with parent/child grouping, plus helper formatting for the CLI.

**Step 4: Run tests to verify pass**

Run the focused test module and then `cargo test -p task-cli`.

**Step 5: Commit**

```bash
git add crates/task-core/src crates/task-cli/src/main.rs
```

---

## Task 3: Make @mentions universal and actionable

**Objective:** Resolve mentions in comments, create notification records, and support people/agents.

**Files:**
- Modify: `crates/task-core/src/team.rs`
- Modify: `crates/task-core/src/service.rs`
- Modify: `crates/task-core/src/service_impl.rs`
- Modify: `crates/task-db/src/entities/comment.rs`
- Modify: `crates/task-db/src/entities/reaction.rs` if reactions/notifications should share mention logic
- Modify: `crates/task-cli/src/main.rs`

**Step 1: Write failing tests**

Add tests for:
- extracting mentions from comment text
- resolving mentions to known team members and bot accounts
- mention notifications on task/project/file comments
- rejecting malformed mention tokens or leaving them as text

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core mention
```

**Step 3: Write minimal implementation**

Use the existing mention extraction logic if present, but ensure it works for the universal comment model and produces notification/mention records for all supported target types.

**Step 4: Run tests to verify pass**

Run the focused tests and any notification/activity tests that cover comment creation.

**Step 5: Commit**

```bash
git add crates/task-core/src/team.rs crates/task-core/src/service.rs crates/task-core/src/service_impl.rs crates/task-db/src/entities/comment.rs
```

---

## Task 4: Add timestamped review comments for files and media

**Objective:** Support point-in-time and ranged comments on reviewable assets.

**Files:**
- Modify: `crates/task-db/src/entities/comment.rs`
- Modify: `crates/task-core/src/service.rs`
- Modify: `crates/task-core/src/service_impl.rs`
- Modify: `crates/task-cli/src/main.rs`
- Modify: `docs/architecture/project-outputs.md`

**Step 1: Write failing tests**

Add tests that prove:
- a comment can store a timestamp or a time range
- comments render with timecode syntax in CLI
- file comments and output review comments are associated with a specific asset
- threaded replies can be attached to timecoded parent comments

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core timestamp
```

**Step 3: Write minimal implementation**

Use the existing `time_start` / `time_end` fields, expand the target model as needed, and ensure formatting clearly shows:
- exact timestamp
- time range
- linked asset name/path
- reply indentation

**Step 4: Run tests to verify pass**

Run the focused tests and the review-related suite.

**Step 5: Commit**

```bash
git add crates/task-db/src/entities/comment.rs crates/task-core/src/service.rs crates/task-core/src/service_impl.rs docs/architecture/project-outputs.md
```

---

## Task 5: Add file comment storage and Nextcloud sync hooks

**Objective:** Make comments on files first-class and syncable.

**Files:**
- Create: `crates/task-core/src/file_comments.rs`
- Modify: `crates/task-core/src/service.rs`
- Modify: `crates/task-core/src/service_impl.rs`
- Modify: `integrations/nextcloud/README.md`
- Modify: `integrations/nextcloud/` adapter code once created

**Step 1: Write failing tests**

Add tests for:
- adding a comment to a file path
- listing file comments by path
- preserving thread parent IDs on sync/import
- mapping comments to Nextcloud-backed review artifacts when available

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-core file_comments
```

**Step 3: Write minimal implementation**

Introduce a file-comment adapter that stores comments in Task and exposes a sync layer for Nextcloud-compatible file review metadata.

**Step 4: Run tests to verify pass**

Run the focused file-comment tests and the broader Nextcloud smoke tests.

**Step 5: Commit**

```bash
git add crates/task-core/src/file_comments.rs crates/task-core/src/service.rs crates/task-core/src/service_impl.rs integrations/nextcloud
```

---

## Task 6: Expose comment threading in the CLI

**Objective:** Make thread-aware collaboration usable from `task`.

**Files:**
- Modify: `crates/task-cli/src/main.rs`
- Possibly create: `crates/task-cli/src/comment.rs`

**Step 1: Write failing tests**

Add CLI tests for:
- `task comment add <ref>` with `--reply-to`
- `task comment list <ref>` showing nested threads
- `task comment resolve <ref> <comment_id>`
- `task comment reopen <ref> <comment_id>`
- `task comment add <file-path>` for file review comments
- `task comment add <ref> --time-start ... --time-end ...`

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test -p task-cli comment
```

**Step 3: Write minimal implementation**

Keep the CLI aligned with the shared comment model and avoid task-body-only hacks for new comments.

**Step 4: Run tests to verify pass**

Run the task-cli comment suite and one remote smoke test through Vox.

**Step 5: Commit**

```bash
git add crates/task-cli/src/main.rs crates/task-cli/src/comment.rs
```

---

## Task 7: Add Nextcloud-facing collaboration docs and smoke tests

**Objective:** Document the collaboration model and prove it works in the hermetic server test harness.

**Files:**
- Modify: `docs/self-host.md`
- Modify: `ARCHITECTURE.md`
- Modify: `VISION.md`
- Modify: `apps/server/tests/remote_doctor.rs`
- Modify: `integrations/README.md`

**Step 1: Write failing tests**

Add remote smoke coverage for:
- creating a comment on a task over Vox
- replying to a comment over Vox
- adding a timestamped review comment over Vox
- listing a threaded conversation
- filtering by mention or unresolved status

**Step 2: Run tests to verify failure**

Run:
```bash
nix --extra-experimental-features 'nix-command flakes' develop -c cargo test --workspace
```

**Step 3: Write minimal implementation**

Wire docs to the new collaboration primitives and make the remote server fixture hermetic so host Nextcloud state cannot leak into thread/mention expectations.

**Step 4: Run tests to verify pass**

Run the focused remote test plus the full workspace suite.

**Step 5: Commit**

```bash
git add docs/self-host.md ARCHITECTURE.md VISION.md apps/server/tests/remote_doctor.rs integrations/README.md
```

---

## Recommended build order

1. Universal comment target model
2. Thread trees and queries
3. Mentions and notifications
4. Timestamped review comments
5. File comment storage and sync hooks
6. CLI exposure
7. Docs and remote smoke tests

---

## Success Criteria

- A comment can be attached to a task, project, file, or review artifact.
- Replies preserve threaded structure.
- `@mentions` resolve to people or agent identities and generate notifications.
- File review comments can carry timestamps or ranges.
- Comments are visible and usable through the CLI and Vox services.
- The model is compatible with Nextcloud-backed collaboration and file review workflows.

---

## Notes / Assumptions

- The current `comments` DB entity already contains some thread/time fields, so this should be an extension rather than a rewrite.
- If there is already task-body comment parsing, keep backward compatibility and migrate toward the universal comment model gradually.
- The exact Nextcloud file-comment API may vary; start with normalized Task storage + sync adapters, then add direct Nextcloud interoperability where the API supports it cleanly.
- For review comments, exact timestamps and time ranges should be rendered in a way that is easy to copy into audio/video/file review contexts.
