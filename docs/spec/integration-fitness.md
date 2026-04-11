# Fitness Integration Spec

## Overview

### t[integration.fitness.overview]
The Fitness integration configures vault-core for workout and training plan management. It leverages RRULE-based recurrence for scheduled workouts, tracks rest days via `skippedInstances`, and uses `completedInstances` streaks to surface training consistency. The integration provides a "Training Cycle" project template for structured 8-week training blocks and gym-appropriate context conventions.

---

## Recurrence

### t[integration.fitness.recurrence]
Workout tasks use RRULE-based recurrence (see `t[task.recurrence]`) to define training schedules (e.g., `FREQ=WEEKLY;BYDAY=MO,WE,FR` for a three-days-per-week plan). `recurrenceAnchor` is set to `scheduled` so that the next occurrence is always computed from the calendar schedule, not from the actual completion date. This ensures training days stay anchored to the weekly plan regardless of whether a session was completed early or late.

---

## Rest Days

### t[integration.fitness.rest-days]
Deliberately skipping a workout is recorded by adding the scheduled date to `skippedInstances` (see `t[task.recurrence.skipped]`). Skipped days advance the recurrence schedule without contributing to the completion history. A training streak is computed as the number of consecutive dates in `completedInstances` with no gaps of unplanned absence; `skippedInstances` entries do not break the streak. Streak computation is a client-side display concern and is not stored in frontmatter.

---

## Areas

### t[integration.fitness.areas]
Recommended area wikilinks for fitness tasks and projects:

- `[[Health]]` — general health and wellness tracking.
- `[[Fitness]]` — training-specific tasks, workouts, and performance goals.

---

## Contexts

### t[integration.fitness.contexts]
The Fitness integration declares the following context conventions:

- `@gym` — sessions at a commercial gym or fitness facility.
- `@home` — bodyweight or equipment-at-home workouts.
- `@outdoors` — running, cycling, hiking, or outdoor training.
- `@pool` — swimming or aquatic training.

---

## Task Template

### t[integration.fitness.task-template]
Workout task templates use the following field conventions:

- `timeEstimate` — set to the planned session duration in minutes.
- `pomodoroCount` — set to the number of work intervals (e.g., circuit rounds or sets blocks) for Pomodoro-style tracking.
- `body` — freeform Markdown containing sets, reps, weights, and progression notes for the session.

These fields are populated by the project template or the user when creating individual workout tasks.

---

## Project Template

### t[integration.fitness.project-template]
The "Training Cycle" project template scaffolds an 8-week training block. It creates the following milestone tasks:

1. Week 1–2: Foundation — establish baseline loads and movement patterns
2. Week 3–4: Build — progressive overload phase
3. Week 5–6: Peak — highest intensity block
4. Week 7: Deload — reduced volume recovery week
5. Week 8: Test — benchmark or competition performance

Each milestone task is created with `status = open` and `recurrenceAnchor = scheduled`. Individual session tasks are created under each milestone as sub-tasks by the user or via session templates.
