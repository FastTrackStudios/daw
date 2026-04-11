# Learning Integration Spec

## Overview

### t[integration.learning.overview]
The Learning integration configures vault-core for structured self-directed learning, course tracking, and knowledge acquisition. It models spaced-repetition review tasks as recurring daily items, scaffolds courses with module-level tasks, and uses milestone tasks to mark certifications, exams, and major submissions. Tasks originating from external course platforms carry `externalSource = course`.

---

## Recurrence

### t[integration.learning.recurrence]
Spaced repetition review tasks use `FREQ=DAILY` recurrence with `recurrenceAnchor = completion` (see `t[task.recurrence.anchor]`). The `gap` field on a `blockedBy` dependency entry (see `t[task.blocked-by]`) is used to model increasing review intervals: the first review fires 1 day after completion, the second after 3 days, the third after 7 days, and so on. The caller configures the `gap` duration on successive review tasks to implement the spaced repetition schedule; vault-core does not compute spacing automatically.

---

## Areas

### t[integration.learning.areas]
Recommended area wikilinks for learning tasks and projects:

- `[[Learning]]` — top-level area for all learning activity.
- `[[Learning/{{Subject}}]]` — subject-specific area (e.g., `[[Learning/Mathematics]]`, `[[Learning/Spanish]]`); the `{{Subject}}` placeholder is substituted at project creation time.

---

## Contexts

### t[integration.learning.contexts]
The Learning integration declares the following context conventions:

- `@reading` — reading textbooks, articles, or documentation.
- `@flashcards` — active recall via flashcard review (Anki or physical).
- `@video` — watching lectures, tutorials, or course videos.
- `@practice-problems` — working through exercises, problem sets, or coding challenges.
- `@writing` — essays, summaries, reflections, or note synthesis.

---

## Project Template

### t[integration.learning.project-template]
The "Course" project template scaffolds a structured learning project. It creates module tasks with the following status progression: `planned` → `in-progress` → `review` → `done`. Each module task is created with `status = planned`. The template also creates a final milestone task (see `t[integration.learning.milestones]`) for the course completion event. Users populate the module list by duplicating the module task template and adjusting titles for each course unit.

---

## Milestones

### t[integration.learning.milestones]
Major learning outcomes — certificate exams, final submissions, degree assessments — are represented as tasks with an `is_completion = true` status (such as `done`). These milestones are created by the "Course" project template as the final task in the sequence. Completing a milestone task signals that the project's primary learning goal has been achieved and triggers `completedDate` to be set (see `t[task.status.transition]`).

---

## External ID

### t[integration.learning.external-id]
Tasks that originate from or are synchronized with an external course platform carry `externalSource = course` and an `externalId` in the format `course:{platform}:{record_id}` (e.g., `course:coursera:mod-1042`). Tasks with `externalSource = course` may be read-only for fields managed by the external platform, depending on integration configuration.
