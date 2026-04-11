# Project Spec

## Identity & Schema

### t[project.file]
Each project is a single Markdown file in the vault identified by the presence of `project` in its `tags` list. There is no separate projects directory requirement; projects may live anywhere in the vault.

### t[project.schema]
Project frontmatter fields:
- `tags` (list) — must include `project`
- `title` (string, optional) — display name; falls back to filename if absent
- `state` (string) — lifecycle state (see `t[project.state]`)
- `start` (ISO 8601 date, optional) — when work begins
- `due` (ISO 8601 date, optional) — target completion date
- `up` (list of wikilinks, optional) — parent projects or areas of responsibility
- `description` (string, optional) — brief summary of the project's outcome

### t[project.state]
Valid states:
- `planning` — defined but not yet active
- `active` — currently being worked on

Archiving a project is done by adding the `archive` tag (not by changing `state`). Archived projects are excluded from active views but retained in the vault.

### t[project.hierarchy]
Projects may be nested via the `up` field. A project whose `up` points to another project is a sub-project. A project whose `up` points to an area note is a top-level project within that area. Cycles in the `up` graph are invalid and must be rejected.

### t[project.area]
Area notes are identified by `area` in their `tags` list. Areas represent persistent domains of responsibility (e.g., `[[Health]]`, `[[Work]]`). Areas have no `due` date and no `state` field. They serve as organizational parents for projects.

---

## Computed Properties

### t[project.computed.task-counts]
The following are computed by scanning all tasks whose `projects` list references this project:
- `openTaskCount` — tasks with non-completion status
- `completedTaskCount` — tasks with completion status
- `totalTaskCount` — sum of both

### t[project.computed.completion-percent]
`completionPercent` = `completedTaskCount / totalTaskCount * 100`. Defined only when `totalTaskCount > 0`.

### t[project.computed.is-overdue]
A project is overdue if `due` is set, it is not archived, and `due` is before today.

### t[project.computed.next-task]
The "next task" for a project is the single most actionable open task associated with that project. It is computed, not stored. Selection criteria applied in order:

1. Exclude tasks that are complete, cancelled, archived, or blocked (have unresolved `blockedBy` entries).
2. Exclude tasks whose `start` date is in the future.
3. From the remaining candidates, return the task with the highest urgency score (see `t[task.urgency]`).
4. If urgency scores are tied, prefer the task with the earliest `due` date. If due dates are also tied, prefer the earliest `scheduled` date. If still tied, prefer the task with the earliest `dateCreated`.
5. If no eligible tasks exist, `nextTask` is null.

---

## Dashboard View

### t[project.dashboard]
The project dashboard is the primary entry point for getting work done. It displays all projects where `state = active` and the project is not archived, sorted by urgency (projects with overdue tasks or near-due dates first).

Each project card in the dashboard shows:
- Project title
- `nextTask` — the title of the next task to work on (see `t[project.computed.next-task]`), or a "nothing left" indicator if null
- A progress bar representing `completionPercent` (see `t[project.computed.completion-percent]`)
- An overdue indicator if `t[project.computed.is-overdue]` is true

### t[project.dashboard.sort]
Projects on the dashboard are sorted by the following priority, in order:

1. Projects with at least one overdue task (task `due` < today, task not complete) — sorted by how many days overdue their most-overdue task is, descending
2. Projects with a `due` date within the next 7 days — sorted by `due` date ascending
3. All other active projects — sorted by the urgency score of their `nextTask` descending
4. Projects with no open tasks — sorted alphabetically, shown last

### t[project.dashboard.tap]
Tapping or clicking a project card navigates to the project detail view, which shows the full task list for that project filtered to open, unblocked tasks sorted by urgency.

### t[project.dashboard.complete-task]
The `nextTask` on a project card can be marked complete directly from the dashboard without navigating into the project. Completing it immediately recomputes and displays the new `nextTask` for that project.

---

## Workflow Integration

### t[project.workflow]
A project may reference a workflow definition via the `workflow` field (string, optional). The workflow defines the stages the project passes through and what tasks or checklists are expected at each stage.

### t[project.workflow.stage]
`workflowStage` (string, optional) stores the project's current stage within its assigned workflow. Valid values are defined by the referenced workflow definition.
