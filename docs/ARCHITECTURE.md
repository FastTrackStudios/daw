# Keyflow Architecture

This document describes the internal architecture of the Keyflow workspace — how the crates fit together, the rendering pipeline, key design patterns, and extension points.

## Crate Dependency Graph

```
keyflow-proto  (core data model, no I/O)
    ↑
keyflow-text   (parser: .kf text → Chart)
    ↑
engraver-proto (layout engine + rendering: Chart → PDF/SVG/Vello)
    ↑
keyflow        (facade crate: re-exports all, IntoChart trait)
    ↑
keyflow-cli    (CLI binary: kf parse, kf pdf, kf svg)
keyflow-ui     (Dioxus UI: WGPU preview, editing)
```

External dependencies not in workspace:
- `dock-dioxus` — docking panel system for the UI
- `session-ui` — session/transport integration
- Both live at `/Users/codywright/Documents/Development/FastTrackStudio/`

## Crate Responsibilities

### keyflow-proto

The **data model** crate. Zero I/O, zero rendering. Defines:

- **`Chart`** — the root type. Contains sections, metadata (title, artist, tempo, time signature, key).
- **`Section`** — a musical section (Verse, Chorus, Bridge, Intro, Outro, Instrumental, Solo, etc.).
- **`Measure`** — a bar of music containing chords, rhythm elements, melodies, text cues, dynamics.
- **`ChordInstance`** — a chord with symbol, duration, push/pull timing, commands (staccato, accent, stop).
- **`Melody`** / **`MelodyNote`** — inline melody with pitch, octave, duration, octave modifiers.
- **`Chord`** — parsed chord quality (root, quality, extensions, alterations, bass note).
- **`Pitch`** / **`PitchClass`** / **`Octave`** — engraver-level pitch representation with `staff_position()` and `from_midi()`.
- **`KeySpelling`** — context-aware note spelling (Bb vs A#) based on key signature.
- **Time types** — `TimeSignature`, `MusicalDuration`, `MusicalPosition`, tick arithmetic.

Uses [Facet](https://github.com/bearcove/facet) for reflection-based serialization (RPC-ready).

### keyflow-text

The **parser** crate. Converts `.kf` text format → `Chart` structs.

Key modules:
- `chart::parser::ChartParser` — main entry point. Processes lines, identifies sections, delegates to sub-parsers.
- `chart::parser::chords` — chord line parsing with auto-duration, slash notation, push/pull, inline melodies, dot repeats, tempo/time-sig changes.
- `chart::parser::post_process` — alignment computation, measure numbering, position calculation.
- `chart::display` — round-trip serialization (Chart → .kf text).

Parser pipeline:
1. Split input into metadata + section blocks
2. For each section: parse chord lines via `parse_chord_line()`
3. Auto-duration: `apply_auto_durations_between_separators()` distributes beat counts across chords between `|` separators
4. Inline melodies: `m{...}` blocks parsed by `Melody::parse_block()` and attached to measures
5. Post-processing: position calculation, alignment, repeat expansion

### engraver-proto

The **layout and rendering engine**. Largest crate. Converts `Chart` → visual output.

#### Layout Pipeline

```
Chart
  → ChartLayoutEngine.layout_chart_with_config()
    → expand_melodies_across_measures()     // split melodies at barlines
    → detect_push_spillbacks()              // pushed chords that cross barlines
    → group_measures_into_systems()         // line breaking
    → for each system:
        → for each measure:
            → rhythm_builder::build_rhythm()  // RhythmSource → RhythmBuildResult
            → MeasureBuilder.build()          // entries → segments → SceneElements → SceneNode
        → chord_renderer::render_chord_symbols()
        → add_melody_ties()
        → collect beat_positions
  → ChartLayoutResult { scene, pages, beat_positions }
```

#### Key Layout Modules

- **`layout::chart::mod.rs`** — the main layout orchestrator (2800+ lines). Two code paths: paginated (PDF) and continuous scroll (live preview). Handles system grouping, page breaks, section headers, count-in.
- **`layout::chart::types.rs`** — `MelodyNoteSegment`, `MeasureMelodyData`, `expand_melodies_across_measures()`, melody pitch helpers (`melody_pitch_to_line`, `resolve_relative_octave`, `melody_note_extent`).
- **`layout::chart::rhythm_builder.rs`** — unified rhythm pipeline. `RhythmSource` (ExplicitRhythm, MelodyData, SlashNotation) → `RhythmBuildResult` (entries, tuplet specs, head type overrides, note pitches).
- **`layout::chart::chord_renderer.rs`** — chord symbol layout with collision avoidance, accent/staccato markers, stop signs.
- **`layout::chart::constants.rs`** — spacing constants (chord space, system height, articulation distances).
- **`notation::builder.rs`** — `MeasureBuilder`: converts rhythm entries into segments, computes beam groups, renders chords/rests/beams into `SceneNode` tree.

#### Typesetting Layout (tlayout)

The `tlayout` module contains MuseScore-ported layout functions:

- **`note.rs`** — `layout_note()`: notehead glyph + accidental + ledger lines + dots. Y from staff line.
- **`chord.rs`** — `layout_chord()`: notehead(s) + stem + flags. SMuFL anchor points for stem attachment. Accidental width offset for stem alignment.
- **`beam_layout.rs`** — `layout_beam()`: MuseScore's beam algorithm with slope constraints, collision avoidance, per-note stem tips. `determine_beam_direction()`, `calculate_beam_position()`, `calculate_stem_tips()`.
- **`slur_tie.rs`** — tie arcs for barline-crossing notes.

#### Scene Graph

- **`scene::node::SceneNode`** — tree of graphical elements with transforms, semantic IDs, metadata.
- **`scene::paint::PaintCommand`** — drawing primitives (glyph, line, rect, path, text).

#### Export

- **`export::svg::SvgSerializer`** — SceneNode → SVG string with embedded fonts.
- **`export::pdf::PdfSerializer`** — SVG pages → PDF via `svg2pdf`.

#### Import

- **`import::midi_import.rs`** — `MidiFile` from raw MIDI bytes.
- **`import::midi_chart_builder.rs`** — `MidiFile` → chart text. Chord detection, push/pull analysis, rhythm formatting, melody extraction from LINES track.

### keyflow

The **facade crate**. Re-exports all other crates under a unified namespace. Provides `IntoChart` trait for generic parsing (text or MIDI bytes). Contains integration tests.

### keyflow-cli

The **CLI binary** (`kf`). Subcommands:
- `kf parse <file>` — parse and display chart structure
- `kf pdf <file> -o <output>` — render chart to PDF
- `kf svg <file> -o <output>` — render chart to SVG

### keyflow-ui

**Dioxus components** for interactive chart editing. WGPU-based rendering via Vello scene renderer. Targets: native desktop, web (WASM+WebGL2).

## Layout Pipeline / Adapter Pattern

The layout engine uses a **pipeline + adapter** architecture:

```
LayoutPipeline<A: LayoutAdapter>
    ├── PaginatedAdapter    (PDF: discrete pages, page breaks)
    └── ContinuousAdapter   (live preview: unbounded vertical scroll)
```

- **`LayoutPipeline`** (`layout/chart/pipeline/mod.rs`) — the shared orchestration logic
- **`LayoutAdapter`** trait — mode-specific concerns (page boundaries, Y tracking)
- **`LayoutState`** (`pipeline/state.rs`) — mutable state shared during layout (page number, measure index, beat positions, chord tracking)
- **`SystemState`** — temporary state for a single system (line of music)

The config subsystem is split across:
- **`ChartLayoutConfig`** — the legacy monolithic config (still primary)
- **`config/layout_params.rs`** — `LayoutParams` (newer split: margins, spatium, spacing)
- **`config/render_options.rs`** — `RenderOptions` (harmony style, stems, slashes)
- **`config/behavioral_flags.rs`** — `BehavioralFlags` (push_alters_rhythm, hide_repeated)
- Per-chart directives via `/KEY=VALUE` in `.kf` text → `ChartSettings` → `ChartLayoutConfig::with_chart_settings()`

## Key Design Patterns

### Facade Pattern

The `keyflow` crate re-exports everything:
```rust
pub use keyflow_proto::*;          // all chart/chord/key types at top level
pub use keyflow_text as text;      // feature "text"
pub use keyflow_midi as midi;      // feature "midi"
pub use engraver_proto as engraver; // via engraver facade
```
Consumers depend on `keyflow` only, getting a clean namespace.

### Builder Pattern

`MeasureBuilder` is the central example:
```rust
MeasureBuilder::new()
    .entries(rhythm_entries)
    .note_pitches(pitches)
    .head_type_overrides(overrides)
    .justify_to(width)
    .build(ctx)
```

Also used in: `ChartLayoutConfig`, `SvgExportConfig`, `RhythmBuildConfig`, `ChordInstance::with_source_span()`.

### Source Enum for Unified Pipeline

`RhythmSource` unifies three input types into one pipeline:
```rust
enum RhythmSource<'a> {
    ExplicitRhythm { elements, spillbacks },
    MelodyData(&'a MeasureMelodyData),
    SlashNotation { chords, spillbacks },
}
```

### Trait Dispatch for Parsing

`IntoChart` trait dispatches `keyflow::parse(source)` over `&str`, `String`, `&[u8]` (MIDI bytes), and `&Path`.

### Scene Graph + Paint Commands

Layout produces a `SceneNode` tree (like a DOM). Export traverses it to produce SVG or Vello scene. This separation means layout doesn't depend on any specific rendering backend.

### Type-State Units

`Spatium`, `Points`, `Pixels` newtypes enforce unit safety at compile time. `Spatium::to_points(base_spatium)` prevents accidentally mixing coordinate spaces.

### RPC via Facet/Roam

`Chart` and most data model types derive `#[derive(Facet)]`. Service traits in `keyflow-proto::services` are Roam RPC definitions. Same types travel over the wire without duplication.

## Font System

| Font | Role | Constant |
|------|------|----------|
| **Bravura.otf** | SMuFL music glyphs (noteheads, clefs, accidentals, rests, flags) | `BRAVURA_FONT_BYTES` |
| **bravura_metadata.json** | SMuFL anchor points, bounding boxes, engraving defaults | `BRAVURA_METADATA_BYTES` |
| **MuseJazzText.otf** | Chord symbols, section labels, measure numbers (jazz style) | `MUSEJAZZ_FONT_BYTES` |
| **FreeSans.ttf** | Titles, auxiliary text | `FREESANS_FONT_BYTES` |

All fonts embedded via `include_bytes!` statics.

- **`SMuFLFont`** — wraps `skrifa::FontRef` + `smufl::Metadata`. Methods: `advance_width()`, `bounding_box()`, `anchors()`, `get_glyph_path()`.
- **`ChartFontBundle`** — single source of truth. `create_layout_engine(style)` and `configure_renderer(renderer)` guarantee identical font wiring across CLI, web, and REAPER extension.
- **Glyph rasterization** — two paths: `BezPathPen` (Vello/SVG) and `LyonPen` (GPU tessellation via Lyon). Both use `skrifa`'s `OutlinePen` trait.
- **SMuFL anchor points** — `stemUpSE` (1.18, 0.168) and `stemDownNW` (0.0, -0.168) from Bravura metadata for precise stem attachment.

## Configuration

- **`ChartLayoutConfig`** — master layout parameters: margins, spatium, system spacing, max measures/system, harmony style, stems, auto_rhythm_slashes, push_alters_rhythm, spacing slope/density.
- **`MStyle`** — engraver style values (MuseScore-compatible style IDs).
- **`MidiChartConfig`** — MIDI import parameters: key root, title, swing ratio.

## Coordinate System

- Y increases **downward** (screen coordinates).
- Staff line 0 = middle line (B4 in treble clef).
- Positive staff line = **upward** on staff (negative Y).
- Formula: `Y = -line * spatium / 2.0`
- Staff lines at: -4, -2, 0, +2, +4 (bottom to top).
- Spatium = distance between two adjacent staff lines (default 5.0 points).

## Test Infrastructure

- Integration tests in `crates/keyflow/tests/` (numbered: 001–029).
- PDF output tests write to `crates/keyflow/tests/output/`.
- MIDI fixture files in `crates/keyflow/tests/fixtures/` (Thriller, Vienna RPP).
- MIDI discovery corpus in `crates/keyflow/tests/midi/` (real songs for pressure-testing).
- Engraver unit tests: 719 tests covering style, spacing, scene graph, layout primitives.
