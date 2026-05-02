# Roadmap

Phased plan from scaffold (today) to production-grade VRT + fuzzing.

## Phase 0 — scaffold (DONE)

- Workspace skeleton with six crates (`core`, `macros`, `runtime`, `shell`, `snapshots`, `fuzz`).
- `fts-vt-core` defines `Story`, `KnobSpec`, `KnobValue`, `KnobKind`,
  `Interaction`, `Selector`, `WaitCondition`, `InteractionScript`, and
  the `STORIES` / `INTERACTION_SCRIPTS` `linkme` slices.
- `fts-vt-runtime` defines `RenderFn`, `KnobSource`, `render_fn` cast.
- `fts-vt-snapshots` enforces release builds via `compile_error!`.
- Stubs everywhere else.

## Phase 1 — hand-rolled stories prove the registry (1 day)

- Hand-write 1-2 `Story` values for an `fts-ui` component (no macro yet).
- Wire `fts-vt-shell::Lookbook` to read from `STORIES`, render the
  selected story by casting `render` back to `RenderFn`.
- Implement `KnobSource` against a `Signal<HashMap<&'static str, KnobValue>>`.
- Render shell in fts-ui's existing native app (replaces the showcase grid
  in dev only) — proves the registry survives dynamic linking on Blitz.

## Phase 2 — `#[story]` macro (2-3 days)

- Parse fn signature + per-arg `#[knob(default=…)]` + rustdoc.
- Infer `KnobKind` (start with `bool`, `i64`, `f64`, `&str`/`String`).
- Emit:
  - the user fn untouched
  - `static <UPPER>_STORY: Story = Story { … }`
  - `static <UPPER>_RENDER: RenderFn = |src| { user_fn(read_knob…(src), …) };`
  - `#[distributed_slice(STORIES)] static <UPPER>_REG: &Story = &<UPPER>_STORY;`
- Add a `derive(Knob)` companion to support enum knobs cleanly.
- Manual `#[states(...)]` attribute for explicit matrices on enums and
  String-valued cases.

## Phase 3 — `fts-vt-snapshots` MVP (3 days)

Decision: **license-resolve `blitz-vrt`** (file an issue, ask Tony for
MIT/Apache-2.0 dual) **and vendor** the parts we need, with attribution
in `NOTICE`. If unresolved within a week, write our own.

Pieces:

- `render::render_story(story, &dyn KnobSource, viewport) -> Vec<u8>`
  - build `DioxusDocument` (via patched `FastTrackStudios/blitz`)
  - settling loop: resolve → drain net queue → resolve, capped at 100 iters
  - paint via `anyrender_vello_cpu`
  - PNG-encode
- `diff::compare(baseline, candidate, threshold)` with DSSIM
- `harness::generate_tests!()` macro that emits one `#[test]` per
  story × auto-state combo
- `Dockerfile` based on ubuntu:24.04 + pinned fontconfig/freetype so font
  rendering is byte-stable across CI machines
- GitHub Actions workflow that builds the Docker image, runs `cargo test
  --release`, uploads `vrt-diffs/` as an artifact on failure, and
  refreshes `vrt-baselines/` via `workflow_dispatch` input

## Phase 4 — interaction scripts (2 days)

- `Interaction` executor against `DioxusDocument`:
  - `Click(Selector)` → resolve selector → synthesize `UiEvent::PointerDown`
    + `PointerUp` → poll until idle
  - `Type(s)` → focus check → `UiEvent::KeyDown`/`KeyUp` for each char
  - `Key(KeyAction)` → direct keyboard event
  - `Scroll` → `UiEvent::Wheel` on target
  - `Wait(Idle | Mounted | Millis)` → poll-loop / time-bounded
  - `Snapshot(name)` → render + write PNG (or diff if baseline exists)
  - `Assert` → invoke trait-object predicate
- `Selector::Role` resolution via Blitz's accessibility tree (the patched
  fork already enables `accessibility` by default on `dioxus-native-dom`).
- `#[story_test]` macro emits an `InteractionScript` and registers it
  via `INTERACTION_SCRIPTS`.

## Phase 5 — fuzzing (2 days)

- For each story:
  - enumerate focusable nodes from the rendered tree
  - generate `proptest` strategies for `Interaction` sequences (length
    1-N, weighted toward Click/Hover/Type)
  - run sequence; assert no panic, no `tracing::error!`, no NaN/inf in
    layout, all dialogs closeable
  - on failure, persist the minimised counterexample as JSON and a
    final-state PNG under `fuzz-failures/`

## Phase 6 — cross-renderer parity (later)

- `fts-vt-snapshots` already covers Blitz.
- Add `fts-vt-snapshots-wry` (uses `webview.screenshot()`).
- Add `fts-vt-snapshots-web` (Playwright shell that loads the Dioxus web
  build and screenshots).
- Add `fts-vt-snapshots-mobile` (xcrun simctl io / adb screencap).
- All four write into the same `snapshots/` layout. Each renderer can
  pin its own baseline (`snapshots/<story>__<state>__<renderer>.png`)
  and a meta-test asserts cross-renderer DSSIM is below a generous
  threshold to flag drift.

## Multi-project support

This crate is designed to back **all** FastTrack Studios UI projects, not
just `fts-ui`. Concretely:

- `fts-ui` (dioxus 0.7.6) — primary driver, will host the first stories.
- `frame-ui` (dioxus 0.7.2) — secondary consumer; once `frame-ui` adds
  meaningful components the same `#[story]` attribute applies. Pin both
  to the same `fts-visual-testing` git rev.
- Future UI crates — same pattern.

To make sure new features don't accidentally lock to `fts-ui`:

- **Never import `fts-ui`** from any crate in this repo. Trait bounds
  stay on `dioxus_core::Element`.
- The example app under `examples/` (when added) should be a tiny,
  hand-rolled component, not an `fts-ui` re-export.
- CI matrix should run `cargo check` against a fixture that imports
  *only* `dioxus` plus this crate, to catch accidental coupling.

## Open questions

- **`linkme` on wasm**: works but with caveats; may need `inventory` as
  fallback. Decide before phase 2 lands.
- **Auto state-matrix explosion**: cartesian over Bool + small Enum is
  fine; cap at e.g. 32 combinations per story. Beyond that the macro
  fails compilation and the user must write `#[states(...)]`.
- **Snapshot storage**: in-tree under `snapshots/` keeps the diff
  visible in PR review but bloats the repo. Investigate Git LFS or a
  sidecar repo if tests proliferate.
- **License of vendored `blitz-vrt` pieces**: file the issue **before**
  writing any vendoring code so attribution can be set up correctly.
