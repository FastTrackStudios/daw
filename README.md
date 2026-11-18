# daw

The DAW of FastTrackStudios' audio stack: its base (the crates everything
else is built on) and the engine that plays a session. Nothing here depends
on anything above it except the effects (FastTrackStudios/fts-fx, which
depends on nothing here). Studio (FastTrackStudios/studio), and the repos
being split out of it (studio#510), take them from here by git.

## The base

| Crate | Path | What it is |
|---|---|---|
| `daw-proto` | `crates/daw/proto` | The DAW's wire types and services (`#[architect::rpc]`), wasm-clean |
| `daw-control` | `crates/daw/control` | The client handles over `daw-proto` |
| `daw-transport-sync` | `crates/daw/transport-sync` | Several transports playing as one: shared clock, drift correction |
| `daw-module` | `crates/daw/module` | Module-system primitives (`DawModule`, `ActionDef`, `PanelDef`) |
| `daw-audio-graph` | `crates/daw/audio-graph` | The audio processing graph: `AudioNode`, `BufferPool` |
| `daw-allocator` | `crates/daw/allocator` | Realtime-aware allocator and main-thread dispatch |
| `daw-test-macro` | `crates/daw/macros` | `#[daw_test]` / `#[reaper_test]` |
| `utils` | `crates/daw/utils` | Paths and preferences |
| `midi` | `features/midi` | MIDI events, the byte codec, each platform's input behind a feature |
| `audio` | `features/audio` | Audio devices, sample IO, the core audio types |
| `fts-sample` | `features/audio/sample` | Sample loading, the pack engine |

## The engine

| Crate | Path | What it is |
|---|---|---|
| `daw` | `crates/daw/daw` | The facade: the one public API of the DAW; its engine is opt-in (`standalone`) |
| `daw-standalone` | `crates/daw/standalone` | The engine: transport, audio graph, render, plugin hosting, project load and save; a web renderer for an AudioWorklet (`web`) |
| `session-format` | `crates/daw/session-format` | The session document (`DawDocument`, `DawProject`) and its store |
| `dawfile-reaper` | `features/adapters/reaper/dawfile-reaper` | The `.rpp` reader and writer the engine loads and saves with |
| `signal-plugin-host` | `features/plugin-host` | CLAP and VST3 plugins hosted over `daw` |
| `builtin` | `features/dsp/builtin` | fts-fx's effects as engine blocks; its examples compare them with commercial plugins |
| `daw-sync` | `features/sync` | DAWs kept in step: the sync engine, the drift corrector, the peer mesh (`network`), Ableton Link (`link`) |

The paths are Studio's, so history and docs line up.

## History

Until October 2026 this repo was the whole DAW (the engine, the REAPER
backend, the project-file parsers, the UI). That code was folded into the
Studio monorepo and carried on there; it is in this repo's history up to
`5c2570e7` and under the tags `v0.0.6` to `v0.0.9`, which older consumers
still pin. In November 2026 the eleven base crates came back from Studio
with their history, and the rest of the old tree was removed here; the
engine followed, with its history, the same month. `daw-actions` (actions
that drive a live REAPER) stays in Studio with the REAPER adapter.

## Build

```bash
cargo check --workspace --all-targets
cargo test --workspace
```

Several crates are in Studio's browser builds; CI also checks them for
`wasm32-unknown-unknown` with the features those builds ask for (the
commands are in `.github/workflows/checks.yml`).

The engine's timing tests (`sync::daw::tests`, `audio_engine::ahead::tests`)
have wall-clock budgets of a few milliseconds and fail on a loaded machine;
`cargo nextest run` runs them alone (`.config/nextest.toml`), and with
`cargo test` run them alone when they fail beside the rest.

Play an `.rpp` through the engine (release: a debug render path can xrun):

```bash
cargo run --release -p daw-standalone --features rpp-loader --example play_rpp -- song.rpp
```

The browser setlist player's AudioWorklet bundle (the engine's `web`
renderer, committed in the task repo at `apps/web/assets/worklet`):

```bash
cargo build -p daw-standalone --lib --target wasm32-unknown-unknown --release \
    --no-default-features --features decode,web
wasm-bindgen --target web --out-dir ../task/apps/web/assets/worklet \
    --out-name daw_standalone target/wasm32-unknown-unknown/release/daw_standalone.wasm
```

Rust 1.99.1 (`rust-toolchain.toml`). The root `Cargo.toml` declares every
dependency with the versions, git pins and patches Studio uses, so the
fleet builds one of each: change a pin here and in Studio together.

## Working on this and Studio together

In Studio's root `Cargo.toml`, point the git source at a checkout of this
repo, and never commit it:

```toml
[patch."https://github.com/FastTrackStudios/daw"]
daw-proto = { path = "../daw/crates/daw/proto" }
```

Studio's `AGENTS.md` has the full table: replace every line of Studio's
daw `[patch]` table, not just the crate you change, or the engine's path
dependencies and Studio's pins become two copies.

## Licence

GPL-3.0-or-later.
