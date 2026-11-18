# daw: agent instructions

This repo holds the DAW of FastTrackStudios' audio stack: its base crates
and the engine that plays a session (the tables in `README.md`). Studio
(FastTrackStudios/studio) and the repos split out of it (studio#510) take
them by git.

- **Nothing here may depend on a crate from Studio or from any repo that
  depends on this one.** A crate that needs one does not belong here. The
  one repo this depends on is FastTrackStudios/fts-fx (the effects: the
  engine's stretch render, `builtin`'s blocks), pinned by `rev` at
  Studio's; fts-fx must never depend on this repo, or the two would pin
  each other.
- **The paths are Studio's** (`crates/daw/...`, `features/midi`,
  `features/audio`, `features/plugin-host`, `features/sync`,
  `features/dsp/builtin`, `features/adapters/reaper/dawfile-reaper`), so
  history, docs and skills line up. Do not move them.
- **One of everything across the fleet.** The root `Cargo.toml` declares
  each external dependency with Studio's version, git pin and features,
  and the `[patch]` tables are Studio's (a git dependency's `[patch]` does
  not reach its consumers, so each workspace carries its own). Bump a pin
  here and in Studio together; `Cargo.lock` holds no package Studio's does
  not.
- **Consumers pin a revision of this repo** (a tag once there is one). A
  change here reaches Studio only when Studio moves its pin.
- **Processing cores** (`crates/daw/audio-graph`, the pack engine in
  `features/audio/sample`) build for native and wasm: no heap allocation
  on the hot path, no threads, no platform I/O. `features/audio/clippy.toml`
  adds the audio-thread lints; Studio's `features/{dsp,rigs}/clippy.toml`
  are copies of it.
- **Wasm**: `daw-proto`, `daw-control`, `daw-module`, `daw-transport-sync`,
  `utils`, `midi`, `audio`, `fts-sample`, and the engine's `daw`,
  `daw-standalone`, `session-format`, `dawfile-reaper` and `builtin`, are
  in Studio's browser builds; `daw-standalone`'s `web` feature is the
  AudioWorklet renderer. CI checks them for `wasm32-unknown-unknown` with
  the features those builds ask for (`.github/workflows/checks.yml`).
- **Timing tests**: the engine's `sync::daw::tests` and
  `audio_engine::ahead::tests` have millisecond wall-clock budgets; they
  fail on a loaded machine and pass alone. nextest runs them alone
  (`.config/nextest.toml`).
- Rust 1.99.1 (`rust-toolchain.toml`), the fleet's pin. Licence:
  GPL-3.0-or-later, inherited with `license.workspace = true`.
- A PR that changes a crate adds a change file in `.changes/`
  (`.changes/README`).

## Check

```bash
cargo check --workspace --all-targets
cargo test --workspace
```

and the wasm check from `checks.yml`.

## With Studio

To build Studio against a checkout of this repo, add to Studio's root
`Cargo.toml` and never commit it:

```toml
[patch."https://github.com/FastTrackStudios/daw"]
daw-proto = { path = "../daw/crates/daw/proto" }
```

one line per crate being changed (Studio's `AGENTS.md` has the table).
