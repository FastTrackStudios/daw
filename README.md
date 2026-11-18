# daw

The base of FastTrackStudios' audio stack: the crates everything else is
built on and that depend on nothing above them. Studio
(FastTrackStudios/studio), and the repos being split out of it
(studio#510), take them from here by git.

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

The paths are Studio's, so history and docs line up.

## History

Until October 2026 this repo was the whole DAW (the engine, the REAPER
backend, the project-file parsers, the UI). That code was folded into the
Studio monorepo and carried on there; it is in this repo's history up to
`5c2570e7` and under the tags `v0.0.6` to `v0.0.9`, which older consumers
still pin. In November 2026 the eleven crates above came back from Studio
with their history, and the rest of the old tree was removed here. The
engine (`daw`, `daw-standalone`, `session-format`, `daw-actions`) is still
in Studio.

## Build

Builds go through the fleet's devenv shell (`devenv.yaml`, `devenv.nix`):
Rust 1.99.1 at the same nix store path as every FastTrackStudios repo (so
kache shares builds across them), mold, and on Linux the audio (ALSA, JACK,
PipeWire) and desktop libraries the crates link. Enter it with
`devenv shell`, let direnv do it (`direnv allow`; `.envrc` loads it), or run
one command with `devenv shell -- <command>`. `devenv shell -- fleet-doctor`
prints the rustc path, the compiler wrapper (kache) and the linker, and
fails if rustc is off the fleet pin.

```bash
devenv shell -- cargo check --workspace --all-targets
devenv shell -- cargo test --workspace
```

Eight of the crates are in Studio's browser builds; CI also checks them
for `wasm32-unknown-unknown` with the features those builds ask for (the
command is in `.github/workflows/checks.yml`).

Rust 1.99.1: the devenv shell's, and `rust-toolchain.toml` for rustup
outside it (CI's). The root `Cargo.toml` declares every
dependency with the versions, git pins and patches Studio uses, so the
fleet builds one of each: change a pin here and in Studio together.

## Working on this and Studio together

In Studio's root `Cargo.toml`, point the git source at a checkout of this
repo, and never commit it:

```toml
[patch."https://github.com/FastTrackStudios/daw"]
daw-proto = { path = "../daw/crates/daw/proto" }
```

One line per crate you are changing. Studio's `AGENTS.md` has the full
table.

## Licence

GPL-3.0-or-later.
