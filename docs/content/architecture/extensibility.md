+++
title = "Extensibility"
description = "Two ways to add features: in-tree (monorepo) and external (third-party crate)."
weight = 25
+++

A project built on architect can be extended along two axes, and both
are first-class. Understanding the difference is mostly about
understanding **who depends on what**.

## The two axes

```text
       ┌──────────────────┐                  ┌──────────────────┐
       │  <feature>-proto │ ◄──── contract ──┤  <feature>-proto │
       │  (trait + types) │                  │  (trait + types) │
       └──────────────────┘                  └──────────────────┘
                ▲                                     ▲
                │                                     │
   ┌────────────┴───────────┐         ┌───────────────┴────────────┐
   │  in-tree implementations│         │  external implementations  │
   │                         │         │                            │
   │  features/<feature>/    │         │  someone-elses-crate       │
   │    <feature>-<backend>/ │         │  on crates.io              │
   │                         │         │                            │
   │  shipped with the app   │         │  brought in à la carte by  │
   │  binary by default      │         │  consumers who want it     │
   └─────────────────────────┘         └────────────────────────────┘
```

Both implement the same `#[vox::service]` trait that `<feature>-proto`
declares. From the trait's perspective, in-tree and external impls are
indistinguishable.

## Path 1 — In-tree (monorepo)

Crate sits inside the project's `features/<feature>/` tree alongside
the contract. Lives in the same workspace, ships from the same
`Cargo.lock`, gets pulled into the project's app binary directly.

This is what `example-db` and `example-memory` already are in this
repo:

```text
features/example/
  example-proto/        contract
  example-db/           in-tree impl  ←
  example-memory/       in-tree impl  ←
  example/              facade — pick which in-tree impls to enable
```

The facade crate (`example`) exposes cargo features:

```toml
[features]
backend-db = ["dep:example-db", "example-proto/server"]
backend-memory = ["dep:example-memory"]
```

The app binary (`app-cli`, `app-server`) opts in:

```toml
# apps/app/server/Cargo.toml
example = { workspace = true, features = ["backend-db"] }
```

**Use this when**:

- You own the implementation and it's specific to this project
- You want zero coordination cost (PR straight into the repo)
- The impl needs to track contract changes in lockstep
- Versioning is handled by `git tag` on the monorepo, not crates.io

**DAW analogue**: `daw-reaper`, `daw-protools`, `daw-ableton` —
written by the team that owns the `daw` repo, all live at
`features/<feature>/<feature>-reaper/` etc. The `daw` binary builds
with whichever combination of cargo features that project's release
needs.

## Path 2 — External (third-party crate)

Crate lives in someone else's repo, publishes to crates.io, depends
only on `<feature>-proto`. A consuming project pulls it in like any
other dependency.

**The contract crate is the only required surface.** Everything else
(facade, in-tree backends, app binary) is convenience — third parties
can replace any of it.

```toml
# Cargo.toml of a project consuming an external impl
[dependencies]
example-proto = "0.2"
daw-logic-backend = "0.1"   # third-party — implements example_proto::ExampleRepo
```

Then in the consumer's binary:

```rust
use example_proto::ExampleRepoDispatcher;
use daw_logic_backend::LogicBackend;

let backend = LogicBackend::new(/* … */);
let dispatcher = ExampleRepoDispatcher::new(backend);
// mount on vox WebSocket exactly like the in-tree backends
```

**Use this when**:

- The implementation is reusable across projects ("a Stripe backend
  for any architect-shaped billing feature")
- Different teams own the contract and the impl
- The impl wants its own release cadence
- The contract is stable enough that third parties can rely on it
  without coordinating

**DAW analogue**: someone outside the daw team writes `daw-logic` —
a Logic Pro backend that implements the same `timeline_proto::TimelineRepo`
+ `mixing_proto::MixingRepo` traits as `daw-reaper` does in-tree.
They publish to crates.io. A user can then build their own `my-daw`
binary that pulls `daw-cli` (in-tree first-party backends) AND
`daw-logic` (external) — both implement the same contracts, both
mount on the same vox dispatcher.

## What changes between the two paths

| Concern | In-tree | External |
|---------|---------|----------|
| Where does the source live? | `features/<feature>/<feature>-<x>/` | Wherever the author wants |
| What does it depend on? | `<feature>-proto` (path) | `<feature>-proto` (crates.io) |
| How is it enabled? | cargo feature on the facade | direct dep + manual wiring in the binary |
| Who maintains the version? | This project's git history | The author, via crates.io semver |
| Can it be added without a fork? | No — needs a PR | Yes — `cargo add` and wire it in |

The contract crate (`<feature>-proto`) is the boundary that makes the
difference invisible to the running code. Both paths produce a struct
that implements the same `#[vox::service]` trait; the vox dispatcher
takes either one and serves it on the same WebSocket.

## How to design a contract that supports both

Two rules that have to hold for external impls to be possible at all:

**1. `<feature>-proto` must publish without `server` features active.**

If the trait declaration depends on SeaORM or any other server-side
machinery, external impls inherit that constraint. The proto crate
should be wasm-clean by default (which architect's `Entity` derive
already enforces) so a third party can implement the trait against
a Redis backend or a remote HTTP API without pulling in a SQL
dependency they don't want.

**2. The facade is optional, not required.**

`<feature>` (the facade) is a convenience for first-party in-tree
selection. Third parties should be able to skip it entirely and
depend only on `<feature>-proto`. The example app's facade is fine
for first-party builds; consumers who pull in external impls just
won't use it.

## When you want both at once

A real DAW build will mix paths. The same binary might use
`daw-reaper` (in-tree) for timeline, `daw-mock` (in-tree) for tests,
and `daw-cloud-sync` (external, on crates.io) for collaboration.
Architect doesn't care — every backend implements the same trait,
the vox dispatcher takes anything that does, and cargo features
gate compilation.
