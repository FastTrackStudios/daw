# Views Spec

## Today

### t[views.today]
The Today view shows tasks that are immediately actionable today. Inclusion criteria: `due` equals today, OR `scheduled` equals today, OR the task is overdue (see `t[task.computed.overdue]`), OR `status` is `in-progress`. Tasks whose `start` date is in the future are excluded regardless of other fields. Results are sorted by urgency score descending (see `t[task.urgency]`) and then grouped by project.

### t[views.today.empty]
When no tasks match the Today inclusion criteria, the view displays the message "You're all caught up" in place of the task list. No further content or prompts are shown.

---

## Inbox

### t[views.inbox]
The Inbox view surfaces unorganized tasks that need triage. Inclusion criteria: the task has no `projects` entry AND no `due` date AND no `scheduled` date AND `status` is `open`, `none`, or `in-progress`. Tasks with a future `start` date are excluded. Results are sorted by `dateCreated` descending so the newest captures appear first. The Inbox is the intended landing zone for all quick-capture input (see `t[capture.quick-add]`).

### t[views.inbox.empty]
When no tasks match the Inbox criteria, the view displays the message "Inbox zero" in place of the task list.

---

## Upcoming

### t[views.upcoming]
The Upcoming view shows tasks with a future `due` or `scheduled` date. A task is included if either field is set to a date after today, its `status` is not a completion status, and it is not archived or cancelled. Tasks with a future `start` date are excluded. Results are grouped into time buckets — Today, Tomorrow, This Week, This Month, Later — and within each bucket tasks are sorted by urgency score descending.

### t[views.upcoming.empty]
When no tasks match the Upcoming criteria, the view displays the message "Nothing scheduled" in place of the task list.

---

## Cross-View Rules

### t[views.hidden-by-start]
A task whose `start` field is set to a date in the future is hidden from all default views (Today, Inbox, Upcoming, and any other view that does not explicitly opt in to showing future-start tasks). This rule is applied before any other filter so future-start tasks cannot accidentally surface through status or date matches.

### t[views.filter-defaults]
Each view has the following implicit default filters applied before any user-specified filters:

- **Today**: `start` not in future; status not `cancelled`, `archived`, or completion status (unless `in-progress`).
- **Inbox**: no `projects`; no `due`; no `scheduled`; `start` not in future; status is `open`, `none`, or `in-progress`.
- **Upcoming**: has future `due` or `scheduled`; `start` not in future; status is not `cancelled`, `archived`, or a completion status.

These defaults are applied at the query layer and cannot be overridden by the user within the standard view. Custom queries (see `query.md`) may bypass them.
