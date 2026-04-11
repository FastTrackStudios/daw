# Task Spec

## Identity

### t[task.file]
Each task is stored as a single Markdown file in the vault. The filename is the canonical task identifier. Tasks are not embedded inside other notes as inline checkboxes.

### t[task.id]
Tasks have a unique `id` field (string) in frontmatter. Generated on creation, immutable thereafter. Used to reference tasks in dependency chains and external integrations without coupling to filename.

### t[task.title]
The `title` field (string, required) holds the human-readable task name. Stored in frontmatter. The filename may differ from the title.

### t[task.body]
The Markdown body below the frontmatter is the task's freeform description/notes. Not structured; may contain checklists, links, and arbitrary content.

---

## Status

### t[task.status]
The `status` field (string) represents the current lifecycle state of a task.

Built-in statuses, in lifecycle order:

| Value | Label | Completes task | Notes |
|---|---|---|---|
| `none` | None | No | Unclassified/unset. Compatibility default for tasks without an explicit status |
| `open` | Open | No | Ready to be worked on |
| `in-progress` | In Progress | No | Actively being worked on right now |
| `on-hold` | On Hold | No | Paused — waiting on something external, not a dependency block |
| `planned` | Planned | No | Intentionally deferred; not yet actionable |
| `done` | Done | Yes | Successfully completed |
| `cancelled` | Cancelled | No | Explicitly abandoned; will not be done |
| `archived` | Archived | No | Hidden from all normal views; retained for history |

Default status for newly created tasks is `open`. `none` exists for compatibility with tasks that have no status set in frontmatter — it is treated identically to `open` for query and urgency purposes.

Any unrecognized value is treated as `open` for query purposes.

`cancelled` and `archived` are distinct: `cancelled` means the task was consciously dropped and may still be visible in project history views; `archived` means it is fully hidden from all default queries.

### t[task.status.custom]
Status values are configurable. Each status definition has:
- `value` (string) — the stored frontmatter value
- `label` (string) — display name
- `color` (string, optional) — hex color for UI
- `icon` (string, optional) — icon identifier
- `is_completion` (bool) — whether reaching this status counts as completing the task
- `auto_archive_delay_minutes` (u32, optional) — if set, task is automatically archived this many minutes after entering this status

### t[task.status.transition]
When a task transitions to a status with `is_completion = true`, `completedDate` is set to today if not already set. When transitioning away from a completion status, `completedDate` is cleared.

Transitioning to `cancelled` does not set `completedDate`. Transitioning to `archived` from any non-completion status also does not set `completedDate`.

The cycle order for quick-cycle UI gestures (e.g. tapping a status badge) follows the lifecycle order: `open` → `in-progress` → `done` → (back to `open`). `on-hold`, `planned`, `cancelled`, and `archived` are only reachable via explicit selection, not the quick-cycle.

---

## Priority

### t[task.priority]
The `priority` field (string) indicates urgency relative to other tasks.

Valid values (ascending): `none`, `low`, `normal`, `high`, `urgent`.

Default: `none`.

---

## Dates & Scheduling

### t[task.due]
The `due` field (ISO 8601 date string, optional) is the deadline by which the task must be complete.

### t[task.scheduled]
The `scheduled` field (ISO 8601 date string, optional) is the date the task is intended to be worked on. A task is not overdue based on its scheduled date alone.

### t[task.start]
The `start` field (ISO 8601 date string, optional) is the earliest date the task should appear in active views. Tasks with a future start date are hidden from default queries.

### t[task.due-time]
The `dueTime` field (HH:MM string, optional) specifies a time-of-day component for the due date. Only meaningful when `due` is also set.

### t[task.dates.created-modified]
`dateCreated` and `dateModified` (ISO 8601 datetime strings) are set and maintained automatically by the system. They are not edited by the user directly.

### t[task.dates.completed]
`completedDate` (ISO 8601 date string) is set when the task transitions to a completion status. Cleared if the task is reopened.

### t[task.computed.overdue]
A task is overdue if: `due` is set, the task is not complete, and `due` is before today. This is a computed property, not stored in frontmatter.

### t[task.computed.days-until]
`daysUntilDue` is computed as `(due - today)` in whole days. Negative values indicate overdue. Only defined when `due` is set.

---

## Urgency Scoring

### t[task.urgency]
Urgency is a computed numeric score used for sorting. Formula:

```
priority_weight:
  none   → 0
  low    → 1
  normal → 2
  high   → 3
  urgent → 5

date_pressure:
  days_until = min(days_until_due ?? ∞, days_until_scheduled ?? ∞)
  date_pressure = max(0, 10 - days_until)
  If no date is set, date_pressure = 0.
  Overdue tasks (days_until < 0): date_pressure = 10 + abs(days_until)

urgency = priority_weight + date_pressure
```

### t[task.urgency.overdue-boost]
Overdue tasks receive escalating urgency: one additional point per day past due, uncapped.

---

## Organization

### t[task.projects]
The `projects` field (list of wikilinks, optional) associates a task with one or more project notes. A task may belong to multiple projects.

### t[task.contexts]
The `contexts` field (list of strings, optional) represents GTD-style contexts where a task can be done (e.g., `@home`, `@computer`, `@errands`). Not wikilinks.

### t[task.tags]
The `tags` field uses native Obsidian tags. Tags are used for cross-cutting categorization and are not the same as contexts.

### t[task.areas]
The `areas` field (list of wikilinks, optional) links a task to areas of responsibility — persistent life/work domains (e.g., `[[Health]]`, `[[Work/Engineering]]`). Areas are not projects and have no due date.

---

## Time Tracking

### t[task.time.estimate]
`timeEstimate` (integer, minutes, optional) is the user's estimate of how long the task will take.

### t[task.time.entries]
`timeEntries` (list of objects, optional) stores logged work sessions. Each entry:
- `startTime` (ISO 8601 datetime, required)
- `endTime` (ISO 8601 datetime, optional — omitted for an active/open timer)
- `description` (string, optional)

### t[task.time.computed]
The following are computed from `timeEntries`:
- `totalTimeLogged` — sum of all completed entry durations in minutes
- `efficiencyRatio` — `totalTimeLogged / timeEstimate` as a percentage; only defined when `timeEstimate > 0`

### t[task.time.active-timer]
At most one `timeEntry` may have no `end` value at a time. This represents an active running timer. Starting a new timer when one is active automatically closes the previous entry with the current timestamp.

### t[task.pomodoros]
`pomodoroCount` (integer, optional) tracks the number of completed Pomodoro sessions logged against this task.

---

## Recurrence

### t[task.recurrence]
Recurring tasks use RFC 5545 RRULE format stored in the `recurrence` field (string, optional). Example: `FREQ=WEEKLY;BYDAY=MO,WE,FR`.

### t[task.recurrence.anchor]
`recurrenceAnchor` (`'scheduled' | 'completion'`, default `'scheduled'`) controls how the next occurrence date is calculated:

- `scheduled` — next occurrence is calculated from the scheduled date regardless of when the task was actually completed. Use for fixed-calendar habits (e.g., every Monday).
- `completion` — next occurrence is calculated from the actual completion date. Use for flexible routines (e.g., every 3 days after last workout).

### t[task.recurrence.instances]
Completed instances are tracked in `completedInstances` as a list of ISO 8601 date strings (`["YYYY-MM-DD"]`). Completing a recurring task adds today to this list rather than marking the task as permanently done.

### t[task.recurrence.skipped]
`skippedInstances` (list of ISO 8601 date strings, optional) records dates where the task was consciously skipped — acknowledged but not completed. Skipped instances advance the recurrence schedule without contributing to the completion history. Used for rest days, deliberate deferrals, etc.

### t[task.recurrence.next]
The next occurrence date is computed from the RRULE, `recurrenceAnchor`, and either the most recent `completedInstances` entry (anchor: `completion`) or the scheduled date (anchor: `scheduled`). Falls back to `dateCreated` if neither is available. This is a computed property.

---

## Dependencies

### t[task.blocked-by]
`blockedBy` (list of dependency objects, optional) lists tasks that must be satisfied before this task can proceed. Each entry is a structured object per RFC 9253:

```yaml
blockedBy:
  - uid: "[[Some Task]]"       # wikilink or task id
    reltype: FINISHTOSTART     # relationship type
    gap: PT2H                  # optional ISO 8601 duration offset
```

Supported `reltype` values:
- `FINISHTOSTART` — blocker must finish before this task starts (default)
- `FINISHTOFINISH` — blocker must finish before this task finishes
- `STARTTOSTART` — blocker must start before this task starts
- `STARTTOFINISH` — blocker must start before this task finishes

`gap` is an optional ISO 8601 duration (e.g., `PT2H`, `P1D`) representing a required delay between the blocker's transition and this task becoming available.

### t[task.blocks]
`blocking` (list of task paths/wikilinks, optional) explicitly records tasks that this task is blocking. While this can be derived as the inverse of `blockedBy`, storing it directly enables efficient querying without a full vault scan. Both `blockedBy` and `blocking` must be kept consistent on mutation.

### t[task.computed.is-blocked]
A task is blocked if any task in its `blockedBy` list has a non-completion status. This is a computed property used by query filters.

---

## Reminders

### t[task.reminders]
`reminders` (list of objects, optional) defines notification triggers. Each reminder has a unique `id` (string) for stable referencing. Two forms:

**Relative reminder** — fires at an offset from the due or scheduled datetime:
```yaml
reminders:
  - id: rem_abc123
    type: relative
    relatedTo: due          # "due" or "scheduled"
    offset: "-PT30M"        # ISO 8601 duration; negative = before, positive = after
```

**Absolute reminder** — fires at a specific datetime:
```yaml
reminders:
  - id: rem_def456
    type: absolute
    absoluteTime: "2025-06-01T09:00:00"
```

Offset uses ISO 8601 duration format: `PT30M` = 30 minutes, `PT1H` = 1 hour, `P1D` = 1 day.

### t[task.reminders.delivery]
Reminder delivery is handled by the host environment (Obsidian plugin, iOS app, or external integration). `vault-core` only stores and parses reminder definitions; it does not dispatch notifications.

---

## Manual Ordering

### t[task.sort-order]
`sortOrder` (string, optional) stores a LexoRank value used to preserve manual drag-and-drop ordering within Kanban columns and list views. When present, views that respect manual order sort by this field before applying any other sort criteria. When absent, the task falls to the end of manually-ordered lists.

---

## Time Blocking

### t[task.timeblock]
Time blocks are scheduled chunks of calendar time stored in daily note frontmatter (not in task files). A time block may optionally reference one or more tasks or notes via Markdown links in its `attachments` list.

Daily note frontmatter structure:
```yaml
timeblocks:
  - id: tb_abc123
    title: "Deep work"
    startTime: "09:00"      # HH:MM
    endTime: "11:00"        # HH:MM
    attachments:
      - "[[Some Task]]"
      - "[[Project Note]]"
    color: "#6366f1"        # optional hex color
    description: ""         # optional notes
```

### t[task.timeblock.link]
A task is considered time-blocked on a given day if it appears in any `attachments` list of a time block in that day's daily note. This is a computed relationship; tasks do not store time block references directly.

---

## External Integration

### t[task.external-id]
`externalId` (string, optional) stores a reference to a corresponding record in an external system (e.g., a calendar event UID, GitHub issue number, or domain integration record). Format: `{integration}:{id}`, e.g., `gcal:abc123`, `github:42`.

### t[task.external-source]
`externalSource` (string, optional) names the integration that owns this task (e.g., `gcal`, `github`, `fasttracksudio`). Tasks with an `externalSource` may be read-only depending on integration config.
