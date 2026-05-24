# Melody Pitch Rendering Pipeline

This document describes the pitched melody note rendering system added in the `feat: pitched melody note rendering` commit series.

## Overview

Melody notes in keyflow charts now render at correct staff positions with accidentals, ledger lines, and stems following MuseScore conventions. Previously, melody notes rendered as pitchless rhythm symbols on the staff midline.

## Data Flow

```
MelodyNote (keyflow-proto)         ← parse: pitch, octave, octave_modifier, duration
    ↓
MelodyNoteSegment (types.rs)       ← expand: resolved octave, accidental, barline splits
    ↓
RhythmBuildResult (rhythm_builder) ← note_pitches: Vec<Option<(staff_line, Accidental)>>
    ↓
MeasureBuilder (builder.rs)        ← per-note line/accidental → layout_chord / layout_note
    ↓
SceneNode                          ← noteheads at correct Y, stems with SMuFL anchors
```

## Step-by-Step

### 1. Octave Resolution (types.rs)

`expand_melodies_across_measures()` resolves each note's octave:

- **Explicit octave** (`C4`): used directly
- **Relative mode** (`C D E`): `resolve_relative_octave()` finds the octave closest to the previous note (within a 4th), then applies modifier (`'` = +1 octave, `,` = -1)
- **Starting reference**: C4 (middle C)
- **Accidental parsing**: `parse_melody_pitch()` handles `#`, `b`, `n` (natural), `##`, `bb`

### 2. Octave Centering (types.rs)

After resolving all octaves, a post-processing pass centers the melody on the staff:

1. Collect staff positions for all non-rest notes
2. Sort and find the median position
3. Target: median near line 0 (B4, treble clef middle line)
4. Compute shift: `round((0 - median) / 7)` octaves
5. Apply shift to all resolved octaves

Only active when no notes have explicit octave annotations (all relative mode).

### 3. Pitch-to-Staff-Line Conversion (types.rs)

`melody_pitch_to_line(pitch, octave)`:
- Constructs a `Pitch` from the parsed data
- Calls `Pitch::staff_position()` which returns position relative to C4 (middle C = 0)
- Converts to treble clef: `line = staff_position - 6` (B4 at position 6 becomes line 0)

Staff line coordinate system:
- Line 0 = middle line (B4 in treble clef)
- Positive = up, negative = down
- Staff lines at: -4, -2, 0, +2, +4
- Formula: `Y = -line * spatium / 2.0`

### 4. Threading Through Build Pipeline

`RhythmBuildResult.note_pitches` carries `Vec<Option<(i32, Accidental)>>` parallel to entries.

In `MeasureBuilder.build()`, `get_note_pitch(rhythm_idx)` returns per-note `(line, accidental)` which overrides the default `note_line` for:
- Standalone chords (`layout_chord` with `ChordNote { line, accidental }`)
- Beam groups (`BeamNote { line }` + `layout_note` with accidental)
- Multi-note non-flagged groups

### 5. Stem Alignment with Accidentals

When a note has an accidental (flat, sharp, natural), `layout_note` renders the accidental to the LEFT of the notehead, advancing the notehead's X position. The stem must account for this offset.

In `layout_chord`, `accidental_x_offset` is computed as the widest accidental width, and passed to `draw_stem()` and `draw_flags()` which add it to the SMuFL anchor X position.

For beam groups, `BeamNote.x` includes the accidental offset so the beam renderer positions stems correctly.

### 6. Dynamic System Spacing

`melody_note_extent()` computes extra space needed above/below the staff for notes with ledger lines:
- Notes above line +5: `extra_above = (line - 4) * half_sp + 0.5 * spatium`
- Notes below line -5: `extra_below = (-4 - line) * half_sp + 0.5 * spatium`

System height grows: `staff_height + 30.0 + extra_above + extra_below`
Staff Y shifts down by `extra_above` to make room for high notes.
Chord symbols shift up by `extra_above` to stay above the highest notes.

### 7. Natural Signs and Key Signature Awareness

In the MIDI melody extraction pipeline (`midi_chart_builder.rs`):
- Accidental state is initialized from the key signature each measure (e.g., Bb major starts B and E as flat)
- When a natural pitch appears where the key says it should be altered, a natural sign (`n`) is emitted
- `parse_melody_pitch()` recognizes `n` → `Accidental::Natural`

### 8. MIDI Melody Extraction

For RPP files with a LINES track:
1. Extract notes from all MIDI items in the LINES track
2. Dequantize swing (wider tolerance than chord detection)
3. Snap onsets to eighth-note grid
4. Compute IOI-based durations
5. Spell pitches using key context with flat/sharp preference
6. Track accidentals per-measure with key signature initialization
7. Generate single `m{...}` block for entire section
8. `expand_melodies_across_measures` splits at barlines automatically

## Known Limitations

- `ChartLayoutConfig.beam_grouping` exposes jazz/full-bar intent, but non-standard modes still need to be threaded into the rhythm/beam-building pass.
- MIDI IOI duration detection can still produce odd values on uneven real-world performances; use `MelodyGrid` to constrain quantization when needed.
- Ties at barline crossings are correct and now configurable via `ChartLayoutConfig.draw_melody_barline_ties`.
