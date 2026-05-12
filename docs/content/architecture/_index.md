+++
title = "Architecture"
description = "The patterns the architect template is showcasing."
weight = 20
+++

architect's architecture is opinionated: contracts are the source of
truth, implementations are pluggable, and the monorepo layout makes the
distinction visible at a glance.

## Pages in this section

- [The architect pattern](@/architecture/pattern.md) — what
  `#[derive(architect::Entity)]` emits and how to write an entity.
- [Multi-backend features](@/architecture/backends.md) — one contract,
  multiple implementations behind one facade.
- [Testing strata](@/architecture/testing.md) — native unit, native
  integration, browser e2e: which lives where.
- [Monorepo layout](@/architecture/layout.md) — `apps/<app>/<role>` +
  `features/<feature>/<role>` and why the prefixes are duplicated in
  both the directory and the package name.
- [Spec coverage](@/architecture/specs.md) — per-feature
  `features/<feature>/spec/*.md` tracked by tracey.
