# Music Practice Integration Spec

## Overview

### t[integration.practice.overview]
The Music Practice integration configures vault-core for structured daily instrument practice management. It models practice sessions as recurring daily tasks, tracks repertoire pieces through a lifecycle of statuses, and provides a "Piece Study" project template for methodical learning of new repertoire. Pomodoro-style interval tracking maps naturally to timed practice blocks.

---

## Recurrence

### t[integration.practice.recurrence]
Practice session tasks use `FREQ=DAILY` recurrence with `recurrenceAnchor = completion` (see `t[task.recurrence.anchor]`). Using the completion anchor means the next practice session is scheduled relative to when the practitioner actually completed the last one, supporting flexible daily habits rather than rigid calendar slots. This is appropriate for self-directed practice where the goal is consistent daily work rather than practice on specific calendar days.

---

## Session Types

### t[integration.practice.session-types]
Practice tasks are categorized by context to distinguish the type of work within a session:

- `@scales` — technical exercises, scales, arpeggios, and finger patterns.
- `@pieces` — repertoire practice on specific works.
- `@sight-reading` — reading new material at sight.
- `@ear-training` — interval recognition, dictation, and aural skills.
- `@theory` — written or analytical theory study.
- `@improvisation` — free or structured improvisation practice.

---

## Areas

### t[integration.practice.areas]
Recommended area wikilinks for music practice tasks and projects:

- `[[Music/Practice]]` — daily practice sessions and technical development.
- `[[Music/Repertoire]]` — pieces under study, performance-ready works, and repertoire history.

---

## Piece Status

### t[integration.practice.piece-status]
The Music Practice integration defines the following custom statuses for repertoire pieces. None are completion statuses; pieces progress through the lifecycle and can be retired without being permanently done:

| Value | Label | is_completion |
|---|---|---|
| `learning` | Learning | false |
| `polishing` | Polishing | false |
| `performance-ready` | Performance Ready | false |
| `retired` | Retired | false |

`retired` causes the task to be excluded from active practice views while remaining accessible for history.

---

## Pomodoros

### t[integration.practice.pomodoros]
Each Pomodoro session represents a 25-minute focused practice block. `pomodoroCount` (see `t[task.pomodoros]`) tracks the number of completed blocks for a practice task. Practitioners use pomodoroCount to plan practice load (e.g., "3 pomodoros on this piece today") and to review how much time was invested in each area over a practice session or week.

---

## Project Template

### t[integration.practice.project-template]
The "Piece Study" project template scaffolds structured learning of a new piece of music. It creates the following task sequence:

1. Hands Separate — practice each hand independently until fluent.
2. Hands Together (slow) — combine hands at a reduced tempo.
3. Up to Tempo — bring the piece to the target performance tempo.
4. Performance Clean — run-throughs with performance-level consistency and no stops.

Each task is created with `status = learning` and a `timeEstimate` to be filled in by the user. Tasks inherit `projects` pointing to the piece's project note and `areas = ["[[Music/Repertoire]]"]`.
