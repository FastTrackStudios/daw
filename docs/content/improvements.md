+++
title = "Improvements & Known Issues"
weight = 3
+++


This page is the static-site mirror of `docs/IMPROVEMENTS.md`. It tracks known limitations, planned features, and technical debt across the Keyflow codebase.

---

## Rendering / Layout (`engraver-proto`)

### Current Integration Gaps

**Beam grouping mode is only partially wired**
`ChartLayoutConfig.beam_grouping` exposes `Standard`, `JazzHalfBar`, and `FullBar`, and the responsive/iReal-style preset selects `JazzHalfBar`. The rhythm/beam-building pass still uses the standard grouping behavior, so the non-standard modes are API intent rather than rendered behavior.
*File: `crates/engraver-proto/src/engraver/layout/chart/mod.rs`*

**Slash long-note style is not chart-configurable**
`SlashLongStyle` and `NoteHeadType::glyph_with_slash_style` exist, but layout params and chart config still call the default slash glyph path. Users cannot yet choose diamond vs white-slash long noteheads from the chart renderer config.
*File: `crates/engraver-proto/src/engraver/layout/tlayout/note.rs`*

**Programmatic UI chart layouts can reuse stale cache entries**
`ChartRenderer::layout_chart_with_preview_mode` invalidates layout from the source string and preview settings, not the provided `Chart` value. Parse-from-source flows are fine, but programmatic chart edits or DAW-generated charts can be skipped if the caller reuses the same source string.
*File: `crates/keyflow-ui/src/chart_renderer.rs`*

**Low-level harmony layout panics when font metrics are absent**
The main chart engine supplies metrics, but direct callers of `layout_harmony` can still hit an `expect` if they construct `HarmonyStyle` without text font metrics. This should become a typed error or a safe fallback.
*File: `crates/engraver-proto/src/engraver/layout/tlayout/harmony.rs`*

### Recently Fixed

~~**Multi-note chord accidental column stacking**~~ — Fixed. `NoteParams` gained `accidental_column_width`; chord layout aligns all noteheads at a common X.

~~**Slash notehead beam Y anchor mismatch**~~ — Fixed. Beam positioning now accounts for non-standard notehead anchor offsets.

~~**Beat positions not collected in continuous layout mode**~~ — Fixed. Continuous layout emits `BeatPosition` entries for DAW sync and cursor highlighting.

~~**Score renderer is an unused stub**~~ — Deleted. Chart rendering now goes through `layout_chart` -> `SceneNode` -> renderer.

~~**Page backgrounds missing in paginated adapter**~~ — Fixed. The paginated adapter paints page backgrounds on first page and page breaks.

~~**Percussion noteheads use normal noteheads**~~ — Fixed. `NotationMode::Percussion` returns `NoteHeadType::X`.

~~**Hit testing not implemented**~~ — Fixed. `SceneNode::hit_test(point)` returns the deepest visible node whose bounds contain the point.

~~**SemanticId migration incomplete**~~ — Fixed. Legacy scene graph selection IDs were removed and `Selection` now uses `SemanticId`.

~~**Deprecated rhythm API functions**~~ — Removed.

~~**Deprecated `LayoutContext::minimal()`**~~ — Removed. Use `LayoutContextOwned::new_minimal()`.

~~**Push chord detection for count-in**~~ — Fixed. Count-in header rendering detects when the first real chord is pushed.

---

## Parsing (`keyflow-text`)

~~**Key change position tracking incorrect**~~ — Partially fixed. Section index and line-relative measure/beat offsets are tracked; cross-line carryover within a single section remains a limitation.

~~**Source spans not computed for chord tokens**~~ — Fixed across non-parallel and parallel chord-line paths.

~~**Template recall not implemented**~~ — Fixed; the stale TODO was replaced and regression tests cover section-template and `$riff` recall.

~~**`x^` auto-repeat requires explicit measure count**~~ — Fixed. Section length is inferred from headers and prior section memory when possible.

---

## Data Model (`keyflow-proto`)

~~**Scale degree accidentals dropped**~~ — Fixed. Scale-degree and Roman-numeral roots carry accidentals through parse and resolve.

~~**`section.length_measures()` hardcodes 4/4**~~ — Fixed. It now uses the provided `TimeSignature`.

---

## MIDI Import

### Current Issue

**Final HITS chord detection broken** *(needs fixture)* — The carryover fallback handles fully silent sections. The remaining failure mode appears to be a pushed `C#/G` that starts before the HITS section and ends just after `section_start_tick`, falling into pickup-window logic but failing tolerance.
*File: `crates/keyflow/tests/021_midi_import_thriller.rs`*

### Recently Fixed

~~**Carryover chord detection**~~ — Fixed for empty-section carryover via bounded look-back.

~~**Melody note duration accuracy**~~ — Improved with `MelodyGrid { Auto, Eighth, Sixteenth, Triplet }`.

~~**Key detection not automatic**~~ — Fixed with marker/meta-key parsing and pitch-class fallback.

~~**MIDI corpus not promoted to fixture tests**~~ — Fixed with ignored snapshot tests in `keyflow-midi/tests/snapshot_harness.rs`.

---

## Melody Pitch Rendering

### Current Limitations

**Beam grouping modes not applied yet** — `ChartLayoutConfig.beam_grouping` exists, but `JazzHalfBar` and `FullBar` still need to be threaded into the actual rhythm/beam-building pass.

**MIDI IOI duration detection can still produce odd values** — Grid selection is configurable, but real-world MIDI can still need tighter quantization or phrase-aware duration selection.

### Recently Fixed

~~**Octave centering may be aggressive**~~ — Improved with range-aware centering.

~~**No per-measure chord symbol positioning for melody notes**~~ — Fixed. Chord Y is computed from each measure's local melody extent.

~~**Tie rendering at barline crossings**~~ — Configurable via `ChartLayoutConfig.draw_melody_barline_ties`.

---

## UI (`keyflow-ui`)

### Planned

- Interactive editing: click chord to hear, drag to reharmonize, pinch to zoom
- Real-time collaboration: bandleader updates chart; connected screens update via Roam RPC
- Session/DAW transport sync: chart follows playback position in real time

---

## Roadmap Features

### Near-term

- Chord-syllable alignment UI binding
- ChordPro export
- Voicing blocks `<c e g>` syntax

### Medium-term

- MIDI export from voicings
- Interactive chord-syllable UI
- Karaoke format export
- Session integration

### Long-term

- Arrangement intelligence
- Multi-instrument parts
- Education tools
- MusicXML export
- Ultimate Guitar import
- Planning Center / ProPresenter sync
- Backing track generation
