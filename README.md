# Keyflow

A Rust workspace for parsing, modeling, and rendering musical charts. Keyflow provides a text-based chart format (`.kf`), a rich data model for chord progressions, lyrics, melodies, and time—and a GPU-accelerated engraving pipeline that outputs PDF, SVG, or interactive previews.

Part of the [FastTrack Studio](https://github.com/nicholasgasior/fasttrack-studio) ecosystem.

## The `.kf` Format

Keyflow charts are plain-text files designed to be human-readable, version-control friendly, and expressive enough for real-world lead sheets.

```
Vienna (Live) - Billy Joel
120bpm 4/4 #Gm

vs verse 1
Gm//// A#//// F//// Gm////
[lyrics] {Gm}Slow down you {A#}crazy child

ch chorus
D#//// A#//// F//// Gm////
```

**Key features of the format:**
- Metadata on the first lines (title, artist, tempo, time signature, key)
- Named sections (`VS`, `CH`, `BR`, `INTRO`, `OUTRO`, `PRE`, `INST`, `SOLO`, etc.)
- Rhythm notation via slashes (`Cm////`), Lilypond-style durations (`C4. D8`), push/pull (`C+8`), and triplets (`C4t`)
- Lyrics with inline chord attachment: `{Gm}Slow {A#}down`
- Melody variables and inline melodies: `riff = m{ C_8 D_8 E_4 }` then `$riff`
- Key changes, tempo changes, and time signature changes mid-chart
- Multi-block documents with `--- blockname ---` delimiters for combining keyflow, chordpro, and voicing blocks

## Workspace Crates

```
keyflow/
├── keyflow-proto     Core data model — Chart, Chord, Measure, Melody, Lyrics, Time.
│                     Facet-derived types for RPC compatibility.
├── keyflow-text      Text parser — .kf format → Chart structs.
│                     Syntax highlighting support.
├── keyflow-midi      MIDI import — MIDI files → Chart, with automatic chord
│                     detection and timing analysis.
├── engraver-proto    Layout & rendering pipeline — Chart → paginated score →
│                     PDF/SVG/Vello scene. Fonts: Bravura (SMuFL), MuseJazzText.
├── keyflow-ui        Dioxus components for chart editing with live WGPU preview.
│                     Web, native, and desktop-panel targets.
├── keyflow-cli       CLI tool (`kf`) — parse, render, export.
└── keyflow           Facade crate — re-exports everything, provides `IntoChart`
                      trait and generic `parse()` entry point.
```

## Quick Start

### As a library

```rust
// Parse from text
let chart = keyflow::parse("My Song\n120bpm 4/4 #C\n\nVS\nCmaj7/// Dm7///")?;

// Parse from MIDI bytes
let chart = keyflow::parse(midi_bytes)?;

// Access the chart
for section in &chart.sections {
    for measure in &section.measures {
        for chord in &measure.chords {
            println!("{}", chord.symbol);
        }
    }
}
```

### CLI

```bash
# Parse and inspect a chart
kf parse song.kf

# Render to PDF (A4)
kf pdf song.kf -o song.pdf

# Render to SVG
kf svg song.kf -o song.svg

# Import MIDI → PDF
kf midi-pdf recording.mid -o sheet.pdf --chart-output generated.kf
```

## Architecture

```
                    ┌─────────────┐
                    │  .kf text   │    MIDI file
                    └──────┬──────┘        │
                           │               │
                    keyflow-text      keyflow-midi
                           │               │
                           ▼               ▼
                    ┌─────────────────────────┐
                    │      keyflow-proto       │
                    │  (Chart data model)      │
                    └────────────┬────────────┘
                                 │
                    ┌────────────┼────────────┐
                    │            │             │
              engraver-proto  keyflow-ui    services
              (layout+render) (Dioxus UI)  (Roam RPC)
                    │            │
               ┌────┴────┐      │
               │  PDF/SVG │   WGPU
               └──────────┘  preview
```

**Data flows one direction**: source → parse → model → layout → render. The `keyflow-proto` data model sits at the center, RPC-ready via Facet, so the same chart can be used across web, desktop, DAW plugins, and server contexts.

## Key Dependencies

| Crate | Role |
|-------|------|
| [Facet](https://github.com/nicholasgasior/facet) | Reflection, serialization, RPC shapes |
| [Roam](https://github.com/nicholasgasior/roam) | RPC framework for services |
| [Vello](https://github.com/linebender/vello) | GPU-accelerated 2D rendering |
| [Dioxus](https://dioxuslabs.com) | Reactive UI framework |
| [Peniko](https://github.com/linebender/peniko) / [Kurbo](https://github.com/linebender/kurbo) | Painting primitives and geometry |

## Building & Testing

```bash
cargo build              # Build all crates
cargo test               # Run all tests
cargo test -p keyflow    # Run integration tests only
```

The test suite covers parsing (basic structure, chords, keys, durations, melodies, tracks, rhythm notation), MIDI import, layout consistency, and spacing diagnostics.

## License

See [LICENSE](LICENSE) for details.
