+++
title = "Improvements & Known Issues"
weight = 3
+++


This document tracks known limitations, planned features, and technical debt across the Keyflow codebase. Items are grouped by subsystem and ordered by severity within each group.

---

## Rendering / Layout (engraver-proto)

### High Priority

**Multi-note chord accidental column stacking**
When a chord has notes with different accidentals, notes without accidentals appear detached from the stem. MuseScore solves this with accidental column stacking where all noteheads in a chord align at a common X. Current workaround: `accidental_x_offset` uses max width, which works for single-note chords (melody) but not multi-note chords.
*File: `engraver-proto/layout/tlayout/chord.rs:166`*

**Slash notehead beam Y anchor mismatch**
`calculate_beam_position` uses `y_center` (staff-line center) to compute stem lengths, but `stem_y_offset` for slash noteheads returns `-1.0 * spatium`. Effective stem lengths for slash beams are 1sp too short. The beam renders too close to the staff.
*File: `engraver-proto/layout/tlayout/beam_layout.rs:84`*

**Beat positions not collected in continuous layout mode**
`ChartLayoutResult.beat_positions` is empty for continuous (non-paginated) layout. Required for DAW transport sync and interactive cursor highlighting.
*File: `engraver-proto/layout/chart/mod.rs:1703`*

**Score renderer is a stub**
The traditional (non-chart) score renderer path (`renderer/mod.rs`) only draws a background and test staff line. The chart renderer works; this path does not.
*File: `engraver-proto/renderer/mod.rs:120`*

### Medium Priority

**Page backgrounds missing in paginated adapter**
The new paginated adapter (`adapters/paginated.rs`) has stubs for page backgrounds. Multi-page PDFs have no page background fill.
*File: `engraver-proto/layout/chart/adapters/paginated.rs:108,135`*

**Percussion noteheads use normal noteheads**
`NotationMode::Percussion` returns `NoteHeadType::Normal` instead of `NoteHeadType::X`.
*File: `engraver-proto/notation/mode.rs:27`*

**Slash notehead style not configurable**
Half/whole-duration slash noteheads are hardcoded to diamond white. Should be configurable between diamond and white-slash glyphs.
*File: `engraver-proto/layout/tlayout/note.rs:95`*

**Hit testing not implemented**
`Scene.hit_test()` returns `None` unconditionally. Required for interactive click-to-select in the UI.
*File: `engraver-proto/scene/mod.rs:128`*

**SemanticId migration incomplete**
Legacy `GraphicalObjectId` / `PositionAndShape` types still present. Interaction module not migrated.
*File: `engraver-proto/scene/mod.rs:67`*

### Low Priority / Technical Debt

**Deprecated rhythm API functions** — Three functions in `rhythm_builder.rs` are `#[deprecated]`: `estimate_expanded_rhythm_counts`, `build_rhythm_from_chord_rhythms`, `build_rhythm_with_triplets`. Should be removed.
*File: `engraver-proto/layout/chart/rhythm_builder.rs:1227,1293,1311`*

**Deprecated `LayoutContext::minimal()`** — Leaks memory. Migration target: `LayoutContextOwned::new_minimal()`.
*File: `engraver-proto/layout/context.rs:403`*

**Push chord detection for count-in** — `CountInHeaderConfig.has_pushed_chord` is always `false`. Charts with a pushed first chord get wrong count-in pattern.
*File: `engraver-proto/layout/chart/mod.rs:2007`*

---

## Parsing (keyflow-text)

### Medium Priority

**Key change position tracking incorrect**
When a mid-chart key change is detected, `AbsolutePosition` is hardcoded to `at_beginning()` and `section_index` to `0`. Key-change events land at wrong positions.
*File: `keyflow-text/chart/parser/chords.rs:1483`*

**Source spans not computed for chord tokens**
`parse_chord_token` always receives `None` for source span. Diagnostic error locations point nowhere.
*File: `keyflow-text/chart/parser/chords.rs:1676`*

**Template recall not implemented**
Post-processing has a stub for `$riff`-style variable recall across sections. Does nothing.
*File: `keyflow-text/chart/parser/post_process.rs:53`*

### Low Priority

**`x^` auto-repeat requires explicit measure count**
Cannot infer section length from context. Returns hard error if not declared.
*File: `keyflow-text/chart/parser/chords.rs:1814`*

---

## Data Model (keyflow-proto)

**Scale degree accidentals dropped**
`RootNotation::from_scale_degree` silently drops the accidental parameter. `b3` loses its flat.
*File: `keyflow-proto/primitives/root_notation.rs:65`*

**`section.length_measures()` hardcodes 4/4**
`beats_per_measure` is unconditionally `4.0`. All sections report incorrect lengths in non-4/4 meters.
*File: `keyflow-proto/sections/section.rs:435`*

---

## MIDI Import

### High Priority

**Carryover chord detection**
When a chord starts in one section and sustains into the next (e.g., `Cm` carrying from CH to Interlude), the importer sees silence. Needs: look for chords with start tick before section boundary but end tick inside it.
*File: `keyflow/tests/021_midi_import_thriller.rs:2996`*

**Melody note duration accuracy**
IOI-based duration from grid quantization works for mostly-eighth-note lines but produces occasional wrong durations (dotted quarters, quarters) when notes have uneven spacing. Needs: configurable quantization grid, or MuseScore-style note-value detection from onset patterns.

**Key detection not automatic**
Key is hardcoded per test. Should detect from MIDI text markers or pitch analysis.
*File: `keyflow/tests/021_midi_import_thriller.rs:1772`*

### Medium Priority

**Final HITS chord detection broken**
Last HITS section chords sometimes detected as silence.
*File: `keyflow/tests/021_midi_import_thriller.rs:3135`*

**MIDI corpus not promoted to fixture tests**
Three corpus files (Bennie And The Jets, Broadview, For Cryin' Out Loud) are discovery material, not deterministic tests.
*File: `keyflow/tests/midi/readme.md`*

---

## Melody Pitch Rendering (new system)

### Known Limitations

**Octave centering may be aggressive**
The smart octave centering post-processes all non-explicit-octave melodies by shifting the median to the middle staff line. For melodies that intentionally span a wide range, this may shift too much.

**No per-measure chord symbol positioning for melody notes**
Chord symbols are offset per-system based on the highest note in any measure of that system. A measure with high notes pushes chord symbols up for the entire system, even if adjacent measures have low notes.

**Tie rendering at barline crossings**
When `expand_melodies_across_measures` splits a note at a barline, a tie arc is drawn. The tie Y position uses the note's pitch, which is correct, but the visual can be unexpected for users who didn't expect the note to be split.

**Beam groups limited to within-beat**
Beaming follows standard rules (break at beat boundaries in 4/4). Jazz-style beaming across beat boundaries (e.g., beaming all 4 eighths in a beat pair) is not implemented.

---

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
