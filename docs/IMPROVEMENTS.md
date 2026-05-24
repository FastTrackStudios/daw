# Planned Improvements & Known Issues

This document tracks known limitations, planned features, and technical debt across the Keyflow codebase. Items are grouped by subsystem and ordered by severity within each group.

---

## Rendering / Layout (engraver-proto)

### High Priority

**Beam grouping mode is only partially wired** — `ChartLayoutConfig.beam_grouping` exposes `Standard`, `JazzHalfBar`, and `FullBar`, and the responsive/iReal-style preset selects `JazzHalfBar`. The rhythm/beam-building pass still uses standard grouping behavior, so non-standard modes are API intent rather than rendered behavior.
*File: `engraver-proto/layout/chart/mod.rs`*

~~**Multi-note chord accidental column stacking**~~ — Fixed. `NoteParams` gained `accidental_column_width`; in chord layout each note uses the chord's max accidental width as a shared column, with each note's accidental right-aligned inside it so all noteheads land at a common X. Single-note layout unchanged.

~~**Slash notehead beam Y anchor mismatch**~~ — Fixed. Introduced `stem_anchor_y(note, stem_dir, spatium)` and use it in `calculate_beam_position` + the collision-avoidance loop, so beams account for non-standard notehead anchor offsets (slash, X) instead of measuring stem length from the notehead center.

~~**Beat positions not collected in continuous layout mode**~~ — Fixed. `layout_continuous` now mirrors the paginated path's tempo/tick bookkeeping (480 ticks/quarter), walks each measure's `CHORD_REST` segments, and emits `BeatPosition` entries with absolute_tick / time_start / time_end / x / staff_y. Continuous mode has no count-in synthesis, so the timeline starts at t=0.

~~**Score renderer is an unused stub**~~ — Deleted. `EngraverRenderer` had no callers and was only drawing a placeholder staff line. Use `scene_renderer::SceneRenderBuilder` / `VelloSceneRenderer` directly via the chart pipeline (`layout_chart` → `SceneNode` → renderer). `RenderConfig` is kept for downstream consumers.

### Medium Priority

**Programmatic UI chart layouts can reuse stale cache entries** — `ChartRenderer::layout_chart_with_preview_mode` invalidates layout from the source string and preview settings, not the provided `Chart` value. Parse-from-source flows are fine, but programmatic chart edits or DAW-generated charts can be skipped if the caller reuses the same source string.
*File: `keyflow-ui/src/chart_renderer.rs`*

**Low-level harmony layout panics when font metrics are absent** — The main chart engine supplies metrics, but direct callers of `layout_harmony` can still hit an `expect` if they construct `HarmonyStyle` without text font metrics. This should become a typed error or a safe fallback.
*File: `engraver-proto/layout/tlayout/harmony.rs`*

~~**Page backgrounds missing in paginated adapter**~~ — Fixed. `PaginatedAdapter::add_background` paints the first page; `handle_boundary` paints each new page on break. Both delegate to `page_rendering::add_page_background`.

~~**Percussion noteheads use normal noteheads**~~ — Fixed. `NotationMode::Percussion` now returns `NoteHeadType::X`.

**Slash notehead style not chart-configurable** — Fixed at the glyph helper level. Added `SlashLongStyle { Diamond, WhiteSlash }` and `NoteHeadType::glyph_with_slash_style(duration, style)`; default `glyph()` keeps the legacy diamond. Threading the style through chord/note params and chart config remains.
*File: `engraver-proto/layout/tlayout/note.rs`*

~~**Hit testing not implemented**~~ — Implemented `SceneNode::hit_test(point)` that walks the scene tree depth-first and returns the deepest visible node whose world-space bounds contain the point (skipping invisible subtrees and zero-area nodes). The legacy `SceneGraph::hit_test` stub on the deprecated `SceneGraph` was removed.

~~**SemanticId migration incomplete**~~ — Complete. Migrated `Selection` (interaction module) to `SemanticId`; deleted the legacy `GraphicalObjectId`, `PositionAndShape`, and `SceneGraph` types from `scene/mod.rs`. Added round-trip tests for `Selection::{select, add, toggle, contains}`. `Selection::len` exposed for callers.

### Low Priority / Technical Debt

~~**Deprecated rhythm API functions**~~ — Removed `estimate_expanded_rhythm_counts`, `build_rhythm_from_chord_rhythms`, `build_rhythm_with_triplets` from `rhythm_builder.rs`. No callers remained.

~~**Deprecated `LayoutContext::minimal()`**~~ — Removed (memory leak). Use `LayoutContextOwned::new_minimal()`.

~~**Push chord detection for count-in**~~ — Fixed. New helper `chart_first_chord_is_pushed` walks the first non-compact / non-End section, finds the first measure with chords, and checks the lead chord's `push_pull.is_push`. Threaded into `add_title_header` → `CountInHeaderConfig.has_pushed_chord`.

---

## Parsing (keyflow-text)

### Medium Priority

~~**Key change position tracking incorrect**~~ — Partially fixed. `section_index` now derived from `self.chart.sections.len()`; `AbsolutePosition` now reflects line-relative measure + beat offset (via `MusicalDuration::from_beats`). Cross-line carryover within a single section is still not tracked — multi-line sections will still anchor key changes at line-start.

~~**Source spans not computed for chord tokens**~~ — Fixed across both paths. Non-parallel chord-line path uses `tokenize_with_spans`. Parallel-container path now threads a `line_byte_offset` through `parse_chord_line_with_offset` → `parse_parallel_chord_line` → `parse_parallel_measure`; new `_spanned` variants of the two splitters preserve byte offsets so each branch's tokens land in the original-line coordinate system. Tests verify spans for `<< C7 ; F7 >>` and mixed `Am | << Dm7 ; G7 >> | Cmaj7` resolve back to the right substrings.

~~**Template recall not implemented**~~ — The post-processing comment was misleading: recall already happens at parse time (section-template recall via `templates.recall_transposed` for empty section content; `$name` melody-variable recall in `parse_chord_line`). Stale TODO replaced with a doc comment explaining the dispatch. Two regression tests added: section-template replay across two `CH 4:` headers, and `$mainRiff` melody recall. Found and fixed two real bugs uncovered by the tests: (1) `apply_auto_durations_between_separators` was mangling `$name` tokens into `$name_N`, breaking recall; (2) `$name` after a chord that auto-completed the measure attached to an empty unpushed measure and was silently dropped — now attaches to the most-recent pushed measure (mirrors the inline `m{...}` branch).

### Low Priority

~~**`x^` auto-repeat requires explicit measure count**~~ — Fixed. Now infers section length from (1) the section header count, (2) the most recent prior section of the same type, (3) `chart.section_measure_memory`, and only errors if none of those resolve.

---

## Data Model (keyflow-proto)

~~**Scale degree accidentals dropped**~~ — Fixed. `RootFormat::ScaleDegree` and `RootFormat::RomanNumeral` now carry an optional `Accidental`, parsed from leading `b`/`#`/`bb`/`##` and applied during `resolve`.

~~**`section.length_measures()` hardcodes 4/4**~~ — Fixed. Method now takes `&TimeSignature` and uses `beats_per_measure()` from it.

---

## MIDI Import

### High Priority

~~**Carryover chord detection**~~ — Fixed for the empty-section case. Existing logic already handled chords that overlap the boundary; the gap was when a section ended up with no chords at all. `build_section_chord_or_rest` now does a bounded look-back (`max(section_length, 8 measures)`) for the most recent prior chord and uses it as the section's sustained chord (with accent flag dropped — original attack lives in the prior section).

~~**Melody note duration accuracy**~~ — Added `MelodyGrid { Auto, Eighth, Sixteenth, Triplet }` config knob on `MidiChartConfig`. `Auto` inspects min IOI per section and picks Triplet (when an IOI ≈ PPQ/3), Sixteenth (when min IOI is materially shorter than an eighth), or Eighth otherwise. New `grid_ticks_to_duration_with_grid` emits the right dotted forms (`.4`, `.8`, `.2`, `8t`, `4t`) per grid. Six unit tests cover auto-detection + duration tables.

~~**Key detection not automatic**~~ — Added `detect_key(midi)` (text markers → MIDI meta key sig → Krumhansl-Schmuckler PC histogram fallback) and `detect_key_by_pitch_class(notes)`. `parse_key_signature_marker` now also accepts `KEY:` / `Key:` / `key:` prefixes and explicit mode-suffix forms (`Bb major`, `F# minor`); bare strings still gated to keyflow's `#`/`b` prefix to avoid section markers like `CH 1` parsing as keys.

### Medium Priority

**Final HITS chord detection broken** *(needs fixture)* — Round 7's `build_section_chord_or_rest` carryover fallback handles the case where the section is *fully* silent (no chord overlap). The remaining failure mode is a pushed `C#/G` that starts before the HITS section but ends a few ticks past `section_start_tick`, which falls inside the existing pickup-window logic but apparently fails its tolerance check (`pickup_tolerance = ppq / 12`). Cannot reproduce without the Thriller MIDI loaded as a fixture (see "MIDI corpus not promoted to fixture tests").
*File: `keyflow/tests/021_midi_import_thriller.rs:3135`*

~~**MIDI corpus not promoted to fixture tests**~~ — Promoted via a new
snapshot harness in `keyflow-midi/tests/snapshot_harness.rs`. Each
corpus `.mid` now has a deterministic `.kf` snapshot written next to it
holding the full output of `generate_chart_text`. Four ignored-by-default
tests (`snapshot_bennie_and_the_jets`, `snapshot_broadview`,
`snapshot_cryin_mateus_asato`, `snapshot_for_cryin_out_loud`) verify
output stability:

```
cargo test -p keyflow-midi -- --ignored snapshot_      # verify
KEYFLOW_UPDATE_SNAPSHOTS=1 cargo test ... -- --ignored # regenerate
```

A built-in line-diff panic message highlights the first 30 differing
lines so accidental regressions are obvious in PR output.

---

## Melody Pitch Rendering (new system)

### Known Limitations

~~**Octave centering may be aggressive**~~ — Range-aware now. Centering is skipped when the melody spans more than ~2 octaves (14 staff positions, deliberately wide) or when the median is already within ~half an octave of the staff center.

~~**No per-measure chord symbol positioning for melody notes**~~ — Fixed. Per-measure `chord_y` computed from each measure's local melody extent inside both the paginated and continuous measure loops, replacing the old system-wide max.

~~**Tie rendering at barline crossings**~~ — Now configurable via `ChartLayoutConfig.draw_melody_barline_ties` (default `true`, matching prior behavior). Set to `false` for lead-sheet styles where the second piece renders as a fresh attack.

**Beam groups limited to within-beat** — Config knob exposed as `ChartLayoutConfig.beam_grouping: BeamGroupingMode { Standard, JazzHalfBar, FullBar }`. Default `Standard` preserves existing behavior. Threading `JazzHalfBar` / `FullBar` through the actual beam-building pass (`build_rhythm` / beam group detector) is still a follow-up — the API now lets callers express intent.

---

## Tree-sitter grammar (`tree-sitter-keyflow` crate)

New crate covering keyflow + embedded ChordPro for the editor-side
highlighting path. Files:

- `grammar.js` — covers `--- block ---` separators, `{directive: value}`
  with conditional `-selector`, `[chord]` / `[*annotation]` markers,
  keyflow chord/rhythm lines (push `'`, accent `>`, `_8t` durations,
  `|` bars, `/` slash runs, `.` dot repeats), section headers
  (`VS 1: "Down":`), metadata header (`120bpm 4/4 #C`), `/cmd = value`
  config directives, `;` line comments.
- `queries/highlights.scm` — captures aligned to standard tree-sitter
  highlight names (`@function`, `@keyword.control.section`, …) so they
  map cleanly onto the LSP `SemanticTokenType` legend the engine already
  emits. Editors using either path see the same coloring.
- `queries/injections.scm` and `queries/locals.scm` — placeholders for
  ChordPro re-injection and `$riff` melody-variable scope tracking.
- Rust binding crate at `bindings/rust/` exposing
  `tree_sitter_keyflow::LANGUAGE` plus the bundled query strings.
- Build script falls back to a NULL-language stub when `src/parser.c`
  isn't present (i.e. before someone runs `tree-sitter generate`), so
  the workspace builds cleanly without forcing the `tree-sitter-cli`
  Node toolchain on every contributor.

The grammar is the **structural / highlighting** path; the IDE engine in
`keyflow-text::ide` and the LSP server in `keyflow-lsp` remain the source
of truth for diagnostics, completion, and hover. Both surfaces consume
the same input and agree on token kinds.

## ChordPro 6.07 (`keyflow-chordpro` crate)

New standalone crate covering the **full ChordPro 6.07 cheat sheet**:

- Typed `DirectiveKind` with structured variants for every documented
  directive (Title, Subtitle, Comment / CommentBox / CommentItalic,
  Highlight, Meta, StartOfEnvironment / EndOfEnvironment / ChorusRecall
  for Verse / Chorus / Bridge / Tab / Grid / Section, NewPage / NewPhysicalPage / NewSong
  / ColumnBreak / Columns / PageType, Style — folds all `*font` / `*size`
  / `*colour`, TitlesFlush, Define / `chord`, Diagrams, Transpose, Image,
  and `Custom` for `x_*` / unknown).
- `[*annotation]` markers as first-class chunks alongside `[chord]`.
- `{define name base-fret 1 frets … fingers … keys …}` parsed into a
  `ChordDefinition` struct with `extra` capture for forward-compat.
- Conditional directive selectors (`{title-en: …}` → `condition: Some("en")`).
- Pre-pass: trailing-`\` line continuation (drops leading whitespace on
  the joined line) + `\uXXXX` Unicode escape expansion. Both keep a byte
  map back into the original source so spans are accurate.
- Quoted directive arguments (`"…"` / `'…'`) are unquoted at parse time.
- Aliases canonicalize at parse (`t` → `title`, `eoc` → `end_of_chorus`,
  `g` → `grid`, `np` → `new_page`, …).
- Errors: `ParseError { kind, message, span }` with `UnclosedBrace`,
  `UnclosedBracket`, `InvalidDirective`, `InvalidEscape` kinds.

### Why a new crate vs a third-party one

No production-grade Rust ChordPro parser exists at the time of writing.
The reference implementation is the Perl `chordpro` CLI; existing
crates.io entries (`chordpro_parser` and friends) cover only `[Chord]Lyric`
and a small subset of directives, with no published activity in 2+ years.

### Document integration

`parse_document(text) -> (Chart, KfDocument)` now also routes
`--- chordpro ---` blocks through the new engine and merges them into the
`Chart`:

- **Metadata**: top-level `{title}` / `{artist}` / `{key}` / `{tempo}` /
  `{time}` / `{composer}` / `{copyright}` / `{year}` flow into
  `Chart::metadata` / `Chart::current_key` / `Chart::tempo` /
  `Chart::time_signature` only when those fields aren't already populated
  by the keyflow block. The keyflow block is the source of truth for
  rhythm + sections; ChordPro fills gaps but never overrides.
- **Lyric attachment**: each `{start_of_verse}` / `{soc}` / `{sob}` block
  becomes a single `LyricLine` (chunks → space-/hyphen-separated
  `LyricSyllable`s with the chord attached to the first syllable of each
  chunk). The line is added as `Track::lyrics(LyricLine)` to the next
  matching keyflow `ChartSection` of the same `SectionType` that doesn't
  already have lyrics. Multiple `--- chordpro ---` blocks are supported
  (e.g. translations); each block is processed in source order.
- **Tab/Grid/Section** ChordPro environments are not lyric-bearing and
  are skipped by the integrator.

End-to-end test (`end_to_end_parse_document_routes_chordpro_block`)
asserts a hybrid `--- keyflow --- / --- chordpro ---` document yields a
chart with rhythm sections AND lyric tracks, with chords attached to the
right syllables (`Twinkle,` → `C`, `little` → `F`).

### Migration path

`keyflow-proto::chord::chordpro` keeps its existing AST as a **legacy
compact view**, and `keyflow_text::chart::parser::parse_chordpro` now
delegates to `keyflow_chordpro::parse` and projects the result back into
the legacy types. All 70+ existing call sites keep working unchanged. New
code should depend on `keyflow-chordpro` directly.

Tests: 13 unit tests + 1 doc test in `keyflow-chordpro`; 3 bridge tests
in `keyflow-text/.../chart/parser/chordpro.rs::tests`. All 57
keyflow-text + 667 keyflow-proto tests still green.

## IDE Engine

Live linter / completion / hover stack, shared between the in-process
Dioxus editor and the LSP server. New module **`keyflow_text::ide`**:

- `analyze(text) -> Analysis { chart, diagnostics, highlights }` — full-document
  pass with line-level error recovery (split on blank lines, parse each chunk
  independently, surface failures as `Diagnostic`s, merge healthy chunks).
- `complete(text, offset, &chart) -> Vec<Completion>` — context-aware
  completions: chord roots / qualities, section headers, slash commands,
  `$riff` melody-variable recall (filtered by `chart.melody_variables`).
- `hover(text, offset, &chart) -> Option<HoverInfo>` — markdown tooltip for
  the token under the cursor (`$name`, scale-degree → key resolution,
  `/cmd`, chord token).
- `Diagnostic { range, severity, code, message, fixes }` mirrors LSP's
  shape so the LSP layer is a numeric cast.
- `offset_to_line_col` / `line_col_to_offset` for editor protocol glue.

Tests: 57 keyflow-text passing (up from 41, +16 in `ide::*`).

## LSP server (`keyflow-lsp`)

New crate. `tower-lsp`-based server; ~310 lines including the
semantic-tokens encoder. Surfaces `publishDiagnostics`, `completion`,
`hover`, and `semanticTokens/full` by delegating to `keyflow_text::ide`.
Trigger characters: `$ / | <space>`. Editor scaffolds shipped:

- `editors/vscode-keyflow/` — `package.json`, `extension.js`, language config;
  `npm run package` produces a `.vsix`.
- `editors/zed-keyflow/` — placeholder for Zed.
- README documents Helix / Neovim setup (just point at `keyflow-lsp` on PATH).

The same engine drives the embedded editor in `keyflow-ui`, so any IDE
feature lands in both surfaces without duplication.

## Embedded editor (live diagnostics)

`keyflow-ui::components::HighlightedEditor` now stacks **three** layers:
highlighted text (existing), a transparent squiggle overlay rendered from
`ide::analyze().diagnostics` (per-severity wavy underline color), and the
input textarea on top. Scroll is mirrored across all three. A status footer
shows the error / warning counts and the first message inline. Diagnostics
re-run on every local-text edit (the analysis call is sub-millisecond, so
no debouncing is needed for the lint pass; the existing 150 ms debounce
still gates the parent `on_change` callback).

Helper `render_squiggle_overlay(source, &[Diagnostic])` is HTML-escape-safe
and merges overlapping ranges. Three unit tests in
`components/highlighted_editor.rs::tests`.

## UI: fts-ui adoption

`keyflow-ui` now depends on the FastTrack Studio shared design system
(`../fts-ui/crates/fts-ui`). The chart **renderer** (Vello scene mounts
in `chart_graphics.rs` / `chart_renderer.rs`) stays raw; every other
piece of chrome should compose `fts-ui` primitives. Wiring landed:

- New workspace dep `fts-ui` in `keyflow/Cargo.toml`.
- `keyflow-ui` adds `fts-ui = { workspace = true }` and re-exports
  `fts_ui::prelude::*` + the `cn!` macro through `keyflow_ui::prelude`,
  so existing `use crate::prelude::*` callers pick up `Button`, `Card`,
  `Tabs`, `Text`, `Toast`, theme tokens, etc. with no churn.
- `[patch."https://github.com/FastTrackStudios/fts-ui.git"]` redirects
  the transitive fts-ui pulled in by `session-ui` to the same local
  checkout so the workspace ships a single fts-ui version.
- Migrated:
  - `components/highlighted_editor.rs` — status footer → `Text` + `TextVariant`.
  - `panels/preview_panel.rs` — "Reset View" overlay → `Button` + `lucide_dioxus` icon.
  - `layouts/chart_editor.rs` — 13 raw buttons + 2 segmented-control
    pill clusters → `Button { variant, size }` (Secondary / Ghost) and
    `SegmentedControl { value, options, on_change }`. Inline SVG path
    chevrons replaced by `lucide_dioxus::{ChevronDown, ChevronLeft, ChevronRight}`.
- `crates/keyflow-ui/MIGRATION_FTS_UI.md` documents what's done plus the
  remaining surface areas (modals → `Dialog`, toasts → `use_toast`,
  metadata inputs → `Input`/`Field`, hard-coded `rgb()` colors → theme
  classes like `text-destructive` / `text-warning`).
- Build state: `cargo build -p keyflow-ui` reports the **same 7 errors**
  as `main` (vello 0.7 vs 0.8 + `dioxus::desktop` module). Verified by
  stashing my changes and recompiling — no regressions from this
  migration. Those pre-existing dep-graph fixes are independent work.

### Round 2 follow-up — dropdowns, theme tokens, providers

- **Examples dropdown** in `layouts/chart_editor.rs` switched from an
  ad-hoc `relative div` + manual open-state to fts-ui's
  `Dropdown` / `DropdownTrigger` / `DropdownContent` / `DropdownItem`
  (carries its own keyboard nav, focus management, and click-outside).
- **Color classes → theme tokens.** `text-green-400` / `text-yellow-400`
  / `text-red-400` / `text-blue-400` replaced with `text-success` /
  `text-warning` / `text-destructive` / `text-info` so light/dark and
  custom theme presets recolor automatically.
- **Squiggle overlay colors** in `components/highlighted_editor.rs`
  switched from inline `rgb(...)` to `var(--destructive)` / `var(--warning)`
  / `var(--info)` / `var(--muted-foreground)` CSS custom properties.
- **`ThemeProvider` + `ToastProvider` wrap** — `ChartEditorLayout` is
  now a thin shell over `ChartEditorLayoutInner` wrapped in
  `ThemeProvider { state: default_theme_state }` and
  `toast::ToastProvider`. Descendants get `use_toast()` and theme-token
  CSS variables for free without any manual plumbing.

After Round 2: zero hard-coded `rgb(...)` UI colors and zero raw
tailwind palette classes (`-400`/`-500` etc.) in `keyflow-ui`. The 7
pre-existing build errors are unchanged.

## UI (keyflow-ui)

### Planned (not started)

- **Interactive editing** — click chord to hear, drag to reharmonize, pinch to zoom
- **Real-time collaboration** — bandleader updates chart; all connected screens update via Roam RPC
- **Session/DAW transport sync** — chart follows playback position in real time

---

## Roadmap Features (from VISION.md)

### Near-term
- Chord-syllable alignment (data model exists, UI binding not wired)
- ChordPro interop (import exists, export not implemented)
- Voicing blocks `<c e g>` syntax (Phase 3, not started)
- Multi-block documents (implemented)

### Medium-term
- MIDI export from voicings
- Interactive chord-syllable UI
- Karaoke format export
- Session integration (DAW transport)

### Long-term
- Arrangement intelligence (progression analysis, substitution suggestions)
- Multi-instrument parts (bass lines, drum patterns, horn hits)
- Education tools (scale degree overlays, Roman numeral analysis)
- MusicXML export
- Ultimate Guitar import
- Planning Center/ProPresenter sync
- Backing track generation
