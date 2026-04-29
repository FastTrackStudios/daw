# API Spec

## Vox Services

### t[api.service]
Task exposes split Rust service traits over Vox RPC: `TaskService`, `ProjectService`, `TimeService`, `CalendarService`, `ClientService`, `InvoiceService`, and `ActivityService`. `VaultServiceImpl` implements those traits, but callers should use the service-specific Vox entry points rather than a compatibility facade.

### t[api.service.list-tasks]
`list_tasks() -> Vec<Task>` returns every task present in the current vault snapshot. The returned list is unfiltered and unsorted; callers should pass the result through the query engine (see `t[api.service.execute-query]`) to apply filters and ordering. The snapshot is the in-memory state from the most recent vault load or file-watch reload.

### t[api.service.execute-query]
`execute_query(query: Query) -> Vec<Task>` evaluates a `Query` value against the vault snapshot and returns matching tasks in the order specified by the query's sort clauses. The `Query` type is defined in `query.md`. Execution is synchronous over the in-memory snapshot; it does not perform file I/O.

### t[api.service.create-task]
`create_task(fields: TaskInput) -> Task` creates a new task file in the vault. A UUIDv4 `id` is auto-generated and must not be supplied by the caller. `dateCreated` and `dateModified` are both set to the current UTC datetime. The task is written atomically (see `t[sync.atomic-write]`) and the in-memory snapshot is updated before returning.

### t[api.service.update-task]
`update_task(id: String, fields: TaskInput) -> Task` applies a partial update to an existing task identified by its `id`. `dateModified` is set to the current UTC datetime on every successful call. The task is located in the vault by its stored `id` field (not by filename), updated in place, and written back atomically.

### t[api.service.complete-task]
`complete_task(id: String) -> Task` transitions a task to its completion state. For recurring tasks (those with a `recurrence` field), the current date is appended to `completedInstances` and the task status remains open; the next occurrence date is recomputed. For non-recurring tasks, `status` is set to `done` and `completedDate` is set to today. Both paths write atomically and update the snapshot.

### t[api.service.list-projects]
`list_projects() -> Vec<Project>` returns all notes in the vault identified as projects (those with `project` in their `tags` list). Archived projects (those with `archive` in their `tags` list) are included in the raw result; callers filter them as needed. Computed properties (`nextTask`, `completionPercent`, task counts) are populated on each returned `Project` value.

### t[api.service.project-stats]
`project_stats(project_id: String) -> ProjectStats` returns a `ProjectStats` struct for a single project. `ProjectStats` contains `open_task_count`, `completed_task_count`, `total_task_count`, `completion_percent`, `is_overdue`, and `next_task` (an `Option<Task>`). The `next_task` field is computed using the algorithm defined in `t[project.computed.next-task]`.

### t[api.service.next-task]
`next_task(project_id: String) -> Option<Task>` returns the single most actionable open task for the given project using the algorithm in `t[project.computed.next-task]`: blocked and future-start tasks are excluded, then the highest urgency score wins, with tie-breaking by earliest due date, then earliest scheduled date, then earliest `dateCreated`. Returns `None` when no eligible tasks exist.

---

## Errors

### t[api.error]
`VaultError` is the top-level error type returned by core service methods. Variants:

- `NotFound(String)` — the requested task, project, or file does not exist in the vault snapshot.
- `ParseError { path: PathBuf, message: String }` — a vault file could not be parsed as valid frontmatter or YAML.
- `IoError(std::io::Error)` — an underlying filesystem operation failed (read, write, rename, watch).

All variants implement `std::error::Error` and `Display`. RPC transports serialize these into structured error responses rather than opaque strings.

---

## Vox RPC

### t[api.vox]
task-core uses Vox for inter-process and in-process communication. The server accepts service connections on `/vox` and dispatches directly to the matching split service. Integrations and future clients should call those Vox services directly; domain REST compatibility endpoints are intentionally not maintained in pre-alpha.
