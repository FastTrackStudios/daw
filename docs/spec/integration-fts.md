# Fast Track Studio Integration Spec

## Overview

### t[integration.fts.overview]
The Fast Track Studio (FTS) integration configures vault-core for professional audio recording project management. It provides a status set tailored to music production workflows, a Recording Project template with production milestone tasks, studio-specific contexts, and area conventions for music work. Tasks created by or synced from Fast Track Studio carry `externalSource = fts`.

---

## Statuses

### t[integration.fts.statuses]
The FTS integration replaces the built-in status set with the following production-lifecycle statuses:

| Value | Label | is_completion |
|---|---|---|
| `session-prep` | Session Prep | false |
| `tracking` | Tracking | false |
| `mixing` | Mixing | false |
| `mastering` | Mastering | false |
| `revision` | Revision | false |
| `delivered` | Delivered | true |
| `archived` | Archived | false |

`delivered` is the sole completion status. `archived` hides tasks from active views without marking them complete.

---

## Project Template

### t[integration.fts.project-template]
The "Recording Project" project template scaffolds a new project with the following ordered milestone tasks, one per production phase:

1. Pre-production — arrangements, charts, and session planning
2. Tracking — recording all live performances and primary takes
3. Overdubs — additional layers, doubles, and supplemental parts
4. Mixing — balance, processing, and spatial arrangement
5. Mastering — loudness normalization, sequencing, and format export
6. Delivery — final files delivered to client or distributor

Each task is created with `status = session-prep` and `externalSource = fts`. The project's initial `state` is `active`.

---

## Contexts

### t[integration.fts.contexts]
The FTS integration declares the following context conventions:

- `@studio` — work performed at the primary recording facility.
- `@home-studio` — work performed at a personal or home setup.
- `@remote` — remote collaboration or file-based work.
- `@rehearsal` — pre-session rehearsal and arrangement work.

---

## Areas

### t[integration.fts.areas]
Recommended area wikilinks for FTS tasks and projects:

- `[[Music/Production]]` — active production work.
- `[[Music/Clients]]` — client-specific project tracking.
- `[[Music/Releases]]` — release pipeline and distribution management.

---

## Time Tracking

### t[integration.fts.time-tracking]
Time entries logged against FTS tasks (via `timeEntries`, see `t[task.time.entries]`) map directly to billable studio session time. Each entry's `description` field should record the session type (e.g., "Tracking drums", "Mix revision 2"). The `totalTimeLogged` computed value for a task represents the total billable hours for that production phase. Integration tooling may export `timeEntries` to invoicing systems using the FTS billing API.

---

## External ID

### t[integration.fts.external-id]
Tasks that originate from or are synchronized with Fast Track Studio carry `externalSource = fts` and an `externalId` in the format `fts:{record_id}` (e.g., `fts:session-4821`). Tasks with `externalSource = fts` may be read-only for certain fields depending on integration configuration; the FTS integration defines which fields the external system owns.
