+++
title = "Monorepo layout"
description = "apps/<app>/<role> + features/<feature>/<feature>-<role>."
weight = 30
+++

The template's directory shape is built so a project can host multiple
apps and an unlimited number of features without naming collisions or
ambiguity about what each crate is.

## The shape

```
apps/
  <app>/                       runtime apps. one app = one binary suite.
    server/                    package: <app>-server
    db/                        package: <app>-db   (migration CLI)
    ui/                        package: <app>-ui   (shell)
    web/                       package: <app>-web
    desktop/                   package: <app>-desktop
    tests/
      e2e/                     package: <app>-tests-e2e

crates/                        publishable libraries.

features/
  <feature>/                   one feature = one capability.
    <feature>/                 facade — selects backend via cargo features.
    <feature>-proto/           wire contract.
    <feature>-<backend>/       one impl per backend (db, memory, ...).
    <feature>-ui/              feature-scoped Dioxus components.
    spec/                      tracey-tracked rules for this feature.
    tests/
      native/                  cargo test against in-memory backend.
      web/                     wasm-bindgen browser tests against a server.

macros/                        proc-macros (architect, architect-derive).
```

## Naming rules

- **Path prefix matches package name prefix.** A crate at
  `apps/<app>/<role>/` is named `<app>-<role>`; a crate at
  `features/<feature>/<feature>-<role>/` is named `<feature>-<role>`.
- **Cargo names use only `[a-z0-9_-]`.** The `<>` notation in this
  template is documentation shorthand for "fill in the blank" — Cargo
  itself rejects literal angle brackets in names.
- **App names and feature names live in different namespaces.** It's
  legal for an app to share a name with a feature, but the prefix split
  (`<app>-` vs `<feature>-`) keeps packages disambiguated.

## Why duplicate the prefix in the path *and* the name

Glanceability. Reading `features/timeline/timeline-reaper/Cargo.toml`
tells you (a) this is the `timeline` feature and (b) the package is
named `timeline-reaper` without opening the file. The slight redundancy
pays back in PR diffs, `cargo tree` output, and crate-graph thinking.

## Adding a second app

Same shape, different prefix:

```
apps/daw-reaper/
  server/                      package: daw-reaper-server
  ui/                          package: daw-reaper-ui
  ...
```

The `daw-reaper-ui` shell composes whichever `<feature>-ui` crates the
Reaper build needs; `daw-ableton-ui` composes the same feature-ui
crates (or a different set, if Ableton exposes a different surface).

## Workspace `members` vs `exclude`

The root `Cargo.toml`'s workspace `members` list everything that
compiles for the host target. Crates with target-cfg deps (wasm-only
test crates, Dioxus apps that pull `dioxus-web`/`dioxus-desktop`) sit
in `exclude` and are invoked with `cargo` directly from inside their
own directory. The `just check` recipe wraps this so a single command
verifies both.
