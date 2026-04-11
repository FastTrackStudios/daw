# Vision

## Problem

Task and project management is fragmented. Generic tools lack domain context, while domain-specific tools don't talk to each other. People end up context-switching between apps and losing the thread of their work across projects, habits, and time.

## Core Idea

Build a **generic, open task and project management platform** that:

1. Works standalone as a complete productivity system
2. Serves as the integration layer for domain-specific workflow applications
3. Is fully compatible with Obsidian as a data backend and UI layer
4. Defines an open standard so any compliant tool can plug in

---

## Platform Layers

### 1. Core Engine (`vault-core`)

The foundation. A generic, domain-agnostic task and project model:

- Tasks with status, priority, scheduling, recurrence, time estimates, blocking relationships
- Projects with hierarchy and lifecycle states
- Query engine for filtering and sorting
- Time tracking and habit tracking primitives
- Obsidian vault as the canonical data store (Markdown + YAML frontmatter)

This layer is intentionally generic. It knows nothing about music, software, or any other domain.

### 2. Integration Layer

External systems and domain-specific tools can push tasks and projects into the core engine. A work task from a corporate system, a GitHub issue, or a music project milestone all become first-class tasks in the platform.

Each integration is scoped and segregated — you decide what surfaces where.

### 3. Workflow Definitions

Workflows define the **stages, checklists, and process** for a type of work. They are reusable templates that give tasks and projects domain-specific shape.

- **Custom workflows** — define your own for personal or team use
- **Community workflows** — shared, versioned workflow definitions published by the community
- Workflows are composable: a music release workflow can embed a mixing workflow

### 4. iPhone App

A native mobile app that serves as the primary daily-driver interface:

- Surfaces tasks from the core engine, filtered and organized by context, project, or workflow stage
- Full segregation: work tasks, personal tasks, and domain-specific tasks stay in their lanes unless you choose to view them together
- Time tracking, habit check-ins, and focus sessions
- Syncs with the Obsidian vault as the source of truth

### 5. Certification Program ("Task Compatible")

An open standard and certification that allows third-party tools and integrations to declare compatibility with this platform.

A **Task Compatible** tool or integration:

- Conforms to the task and project schema
- Respects workflow lifecycle hooks
- Can push/pull tasks and project state through the defined API
- Shows up natively in the task app, organized alongside everything else

This creates an ecosystem where tools built for specific domains (music production, software development, content creation) can all speak the same language.

---

## Domain Integrations

Domain-specific integrations are first-class citizens of the platform. Each one brings its own workflows, terminology, and lifecycle stages while remaining fully interoperable with the core engine and the iPhone app.

### Fast Track Studio *(music production)*

Covers the complete lifecycle of a music project — from initial concept through release — as a structured, opinionated workflow:

- Every stage (ideation, demo, arrangement, tracking, mixing, mastering, distribution, promotion) is modeled as workflow stages with tasks and checklists
- Projects in Fast Track Studio are projects in the core engine; their tasks are tasks in the core engine
- Time spent in sessions is tracked against tasks and projects
- The iPhone app surfaces Fast Track Studio projects alongside everything else

### Fitness & Training

Covers strength training, cardio, mobility, and general fitness programming:

- Training plans modeled as projects with recurring workout tasks
- Exercises tracked as structured sub-tasks with sets, reps, and load
- Progressive overload and periodization represented as workflow stages
- Rest days, deload weeks, and recovery check-ins as first-class scheduled items
- Habit tracking for consistency streaks and weekly volume targets

### Music Practice

Covers deliberate practice for instrumentalists and vocalists:

- Practice sessions structured as recurring tasks with focus areas (technique, repertoire, ear training, theory)
- Long-term pieces or skills modeled as projects with milestone stages (learning, polishing, performance-ready)
- Session logs with time tracked per focus area
- Habit tracking for daily practice streaks and weekly hour targets

### Learning & Study

Covers courses, books, certifications, and self-directed study:

- Learning goals modeled as projects broken into modules or chapters
- Study sessions as recurring tasks with spaced repetition scheduling
- Review milestones and knowledge-check tasks
- Habit tracking for daily study time and weekly progress
- Links to external resources (courses, books, notes) attached to tasks

---

## Goals

| Capability | Included |
|---|---|
| Task management | Yes |
| Project management | Yes |
| Time tracking | Yes |
| Habit tracking | Yes |
| Recurring tasks | Yes |
| Custom workflows | Yes |
| Community workflows | Yes |
| Obsidian compatibility | Yes |
| iPhone app | Yes |
| Domain integrations (e.g. Fast Track Studio) | Yes |
| Open certification standard | Yes |

---

## Design Principles

- **Generic core, specific edges** — the engine is universal; domain knowledge lives in workflows and integrations
- **Obsidian-native** — the vault is always the source of truth; the platform enhances it, never replaces it
- **Open by default** — the schema, workflow format, and certification standard are open so the ecosystem can grow
- **Segregation with unity** — different work contexts stay separate until you choose to unify them
