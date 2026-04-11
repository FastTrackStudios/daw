# Query Engine Spec

## Overview

### t[query.model]
A query is a declarative description of which tasks to return, how to sort them, and how many to include. Queries are composed from filters, sorters, and an optional limit. They operate over a vault's full task set loaded at query time.

---

## Filters

### t[query.filter]
Filters narrow the task set. All filters in a query are combined with AND semantics — a task must pass every filter to be included.

Supported filters:

| Filter | Description |
|---|---|
| `NotComplete` | Excludes tasks with a completion status |
| `NotBlocked` | Excludes tasks with unresolved `blockedBy` dependencies |
| `NotArchived` | Excludes tasks with `status = archived` or the `archive` tag |
| `NotCancelled` | Excludes tasks with `status = cancelled` |
| `DueToday` | Tasks where `due` equals today |
| `DueThisWeek` | Tasks where `due` is within the current calendar week |
| `Overdue` | Tasks where `due` is before today and task is not complete |
| `Scheduled` | Tasks where `scheduled` equals today |
| `HasStarted` | Tasks where `start` is unset or on/before today |
| `HasProject(wikilink)` | Tasks whose `projects` list contains the given wikilink |
| `HasContext(string)` | Tasks whose `contexts` list contains the given string |
| `HasTag(string)` | Tasks whose `tags` list contains the given tag |
| `HasArea(wikilink)` | Tasks whose `areas` list contains the given wikilink |
| `Status(string)` | Tasks with a specific status value |
| `Priority(string)` | Tasks with a specific priority value |
| `ExternalSource(string)` | Tasks from a specific external integration |

### t[query.filter.date-range]
Date range filters take an inclusive start and end date:
- `DueBetween(start, end)` — tasks with `due` within the range
- `ScheduledBetween(start, end)` — tasks with `scheduled` within the range
- `CreatedBetween(start, end)` — tasks with `dateCreated` within the range

### t[query.filter.text]
`TitleContains(string)` matches tasks whose `title` contains the given substring (case-insensitive).

---

## Sorting

### t[query.sort]
Results may be sorted by one or more fields. Each sort specifies a field and direction (ascending/descending). Multiple sort fields are applied in order (primary, secondary, etc.).

Sortable fields:
- `urgency` (descending by default)
- `priority`
- `due`
- `scheduled`
- `dateCreated`
- `dateModified`
- `title` (alphabetical)
- `status`
- `totalTimeLogged`
- `timeEstimate`

### t[query.sort.default]
When no sort is specified, results are sorted by `urgency` descending.

### t[query.sort.nulls]
Tasks without a value for the sort field sort last regardless of direction.

---

## Grouping

### t[query.group]
Results may be grouped by a single field. Grouping produces named buckets. Supported group-by fields:
- `project`
- `context`
- `area`
- `status`
- `priority`
- `due` (groups: overdue, today, this-week, later, no-date)
- `scheduled` (groups: today, this-week, later, no-date)
- `tag`

### t[query.group.ungrouped]
Tasks that have no value for the group field (e.g., no project) appear in an implicit "No [Field]" bucket.

---

## Limit & Pagination

### t[query.limit]
An optional `limit` (positive integer) caps the number of results returned after filtering and sorting.

---

## Execution

### t[query.execute]
Query execution order:
1. Load all tasks from vault
2. Apply `HasStarted` semantics (exclude tasks with future `start` dates) unless explicitly overridden
3. Apply all filters (AND)
4. Sort by specified sorters (or default urgency sort)
5. Group if specified
6. Apply limit

### t[query.execute.snapshot]
Queries operate on a snapshot of vault state at execution time. They are not live/reactive; callers are responsible for re-executing when vault state changes.
