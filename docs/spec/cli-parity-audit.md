# Headless CLI Parity Audit

Date: 2026-05-05
Parent issue: #18

## Result

Core Task workflows can now be operated headlessly through the CLI. The audit found two remaining follow-up areas that are not blockers for local headless operation:

- #30: remote Vox parity for local-only asset, location, and revenue commands
- #31: event materialization for venue defaults

## Covered Workflows

| Workflow | CLI surface | JSON output | Notes |
| --- | --- | --- | --- |
| Tasks | `list`, `add`, `show`, `update`, `complete`, `delete`, `link`, `assign`, `for`, `due-by`, `search` | Yes where automation needs it | Includes comments, reactions, subscribers, reminders, recurrence, relations, and soft delete. |
| Inbox/capture | `capture`, `inbox ...` | Yes | Capture and promotion are scriptable. |
| Projects | `project ...` | Yes | Includes edit, dashboard, comments, files/context, and metadata body preservation. |
| People/organizations | `people ...` | Yes | Headless contact and relationship context paths exist. |
| Operating model | `operate ...` | Yes | Review and operating model reports are available from CLI. |
| Calendar | `calendar list/show/add/update/delete/carddav ...` | Yes | Add/update support freeform `--location` plus stable `--venue` and repeated `--space` refs. |
| Locations/venues | `location add/list/show/update/space-add/space-list/default-add/delete` | Yes | Markdown-backed under `locations/<name>.md`; supports spaces and effective defaults. |
| Assets | `asset create/list/show/report/update/move/status/maintain/repair/reserve/release/conflicts/delete` | Yes | Markdown-backed under `assets/<name>.md`; repair opens linked tasks and reservations report conflicts. |
| Clients | `client ...` | Yes | Billable client management is available headlessly. |
| Invoices | `invoice ...` | Yes | Markdown-backed invoice lifecycle and payment tracking are available. |
| Expenses | `expense create/list/show/report/update/delete` | Yes | Includes project/client/deliverable/vendor/category/reference attribution. |
| Revenue | `revenue create/list/show/report/delete` | Yes | Local vault mode; remote parity tracked in #30. |
| Time tracking | `start`, `stop`, `time ...` | Yes | Active timers, logs, reports, edits, and deletion are available. |
| Email | `email ...` | Yes | Search/link/sweep/account paths are available for automation. |
| Sync/providers | `sync`, `github ...`, `nc ...`, `doctor`, `server ...` | Yes | Includes dry-run/status paths and provider diagnostics. |
| Agent automation | `agent ...` | Yes | Stable JSON command surface for agent snapshots and planning. |

## Verification Commands

These checks were run while completing the final audit work:

```bash
cargo fmt --check
cargo check -p task-cli
cargo test -p task-core location::tests
```

Additional smoke checks were run against temporary vaults for:

- location add, space add, default add, show defaults, calendar add with venue/space refs
- asset create, maintenance log, repair open, reserve with conflict output, list by location

## Remaining Follow-Ups

#30 tracks server/remote parity for newer local-only workflows. Local headless operation is complete, but remote automation should support the same command shapes.

#31 tracks materializing inherited venue/space defaults into event bundles. The reusable data model and CLI default inheritance exist; the follow-up is the file-copy/link workflow for event packages.
