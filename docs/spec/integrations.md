# Integrations Spec

## Framework

### t[integration.definition]
An integration is a named configuration bundle that customizes vault-core behavior for a specific domain or workflow. Integrations contain no executable code; they are purely declarative. An integration may configure custom status sets, project templates, task templates, area conventions, and context conventions. The same vault-core binary serves all integrations.

### t[integration.status-set]
An integration may define a `status_set` — a list of `StatusDefinition` objects that replaces the built-in status list (see `t[task.status.custom]`) for tasks and projects belonging to that integration. Each `StatusDefinition` must supply `value`, `label`, and `is_completion`. The `color`, `icon`, and `auto_archive_delay_minutes` fields are optional. If an integration does not define a `status_set`, the built-in statuses apply.

### t[integration.project-template]
A `project_template` is a named set of default project field values combined with a list of task templates that are auto-created when a new project is created using that template. The template specifies the project's initial `state`, suggested `up` links, and the ordered list of milestone or phase task templates to scaffold. Project templates are referenced by name in the integration schema.

### t[integration.task-template]
A task template defines the shape of a recurring or scaffolded task within a project template or a workflow. Fields:

- `title` (string) — may include `{{variable}}` placeholders substituted at creation time.
- `status` (string, optional) — initial status value.
- `priority` (string, optional) — initial priority.
- `recurrence` (string, optional) — RRULE string.
- `contexts` (list of strings, optional) — initial contexts.
- `tags` (list of strings, optional) — initial tags.
- `timeEstimate` (integer, minutes, optional) — planned duration.
- `body` (string, optional) — freeform Markdown body content.

### t[integration.area-convention]
An integration may declare an `area_conventions` list — recommended area wikilinks for tasks and projects belonging to this integration. These are displayed as suggestions in the area picker UI but are not enforced; users may assign any area. Example: `["[[Health]]", "[[Fitness]]"]`.

### t[integration.context-convention]
An integration may declare a `context_conventions` list — recommended context strings for this domain. These appear as quick-select options in context pickers within the integration's scope. Example: `["@gym", "@home", "@outdoors"]`.

### t[integration.activation]
Integrations are activated per-vault in Settings > Integrations. Multiple integrations may be active simultaneously in the same vault. When multiple active integrations define status sets, the status set used for a given task is determined by the task's `externalSource` field matching an integration's `name`; if no match, the built-in statuses apply.

### t[integration.schema]
Integration configuration is stored as a TOML file at `.config/task/integrations/<name>.toml` inside the vault root. The `<name>` must be a lowercase alphanumeric slug. A minimal integration TOML example:

```toml
name = "example"
display_name = "Example Integration"

[status_set]
statuses = [
  { value = "todo",  label = "To Do",  is_completion = false },
  { value = "done",  label = "Done",   is_completion = true  },
]

[area_conventions]
areas = ["[[Example/Projects]]"]

[context_conventions]
contexts = ["@example-context"]

[[project_templates]]
name = "Default Project"
initial_state = "active"

[[project_templates.task_templates]]
title = "Kickoff — {{project_title}}"
status = "open"
priority = "normal"
```
