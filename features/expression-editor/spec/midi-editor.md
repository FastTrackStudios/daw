# Full MIDI editor — sources and plan

The expression editor has to become a complete MIDI editor, serving six
products off one surface. This records where the behaviour specs come
from, so nothing is invented where a reference exists.

## The six products, and what actually differs

| product | row means | note label | note extras |
|---|---|---|---|
| Melodyne-style audio | MIDI pitch | note name | drift / vibrato blend |
| MPE MIDI | MIDI pitch | note name | channel, 3 expression lanes |
| plain MIDI | MIDI pitch | note name | velocity |
| drum MIDI | named drum lane | drum name | diamonds, no length |
| guitar / bass | **string** | **fret** | articulation, legato, bend |
| vocal lyrics | MIDI pitch | **syllable** | — |

Almost the only structural difference is *what a row means*, so that is
abstracted as [`RowSpace`](../expression-editor-core/src/rows.rs)
(`Pitch` / `Drums(DrumMap)` / `Strings(StringTuning)`). Everything else
— gestures, zones, curves, camera, undo — already works in row space and
needed no change.

## Source 1 — REAPER's MIDI mouse modifiers (in-tree)

`features/reaper/reaper-input/src/input/mouse_modifiers/behaviors/midi/`
already carries REAPER's decoded MIDI-editor contexts and behaviour
names: piano roll, note, note edge, CC lane, CC event, CC segment,
ruler, marker lanes. That is the authoritative list of what a MIDI
editor's mouse must do — 37 piano-roll drag behaviours, 33 note drag
behaviours, 18 note click behaviours.

`mouse.rs` mirrors that shape (context × gesture × modifiers → action)
so a REAPER mouse map can be loaded without translation, and so the
per-product defaults can differ: a drum editor wants paint-on-drag where
a Melodyne editor wants marquee.

Razor-edit behaviours are decoded in-tree too
(`behaviors/razor_edit.rs`) — 17 drag, 9 click, plus edge behaviours.

## Source 2 — Ample Sound Riffer (guitar/bass)

[Ample Guitar manual](https://www.amplesound.net/en/Ample_Guitar_Manual.pdf) §6, and the
[Guitar Tab manual](https://www.amplesound.net/en/Guitar_Tab.pdf).

Riffer's panel is a **string roll**, not a pitch roll: Note Properties
line, Expression line, String Roll, FX Noise line, per-string tuner,
Strum line.

Its key commands (§6.2.1), which the `Riffer (Ample)` mouse preset
matches so nobody coming from it is retrained:

| gesture | action |
|---|---|
| left click | insert note |
| left click a note | select it |
| left click elsewhere | deselect |
| double click a note | delete |
| right click / Alt+click | context menu |
| drag vertically | change pitch |
| drag border horizontally | change length |
| Ctrl + drag vertically | change velocity |
| Ctrl + drag border | change duration |
| Shift + drag | move |

Ten per-note properties (§6.3.1): Pitch, Velocity, Duration,
Articulation, Legato, Vibrato Range, Vibrato Rate, Bend Type, Bend Rate,
Note-Off Velocity — plus a bend editor with draggable points.

Articulations (§6.4.2): Natural Harmonic, Palm Mute, Slap, Pop, Tap,
Staccato, Slide In/Out, Hammer On, Pull Off, Legato Slide, Bender,
Vibrato, Slide Guitar.

Rules worth keeping (§6.4.3): legato is only available between adjacent
notes **on the same string** and is marked on the *first* note; long
legato slide speed comes from the destination note's velocity; natural
harmonics only speak at frets 5, 7, 9, 12.

Two behaviours fall out of the model rather than being special-cased:
moving a note to another string **re-fingers it at the same sounding
pitch** rather than transposing it, and importing MIDI picks the
position nearest the hand rather than the lowest fret.

## Source 3 — juliansader's Multi Tool

`js_Mouse editing - Multi Tool.lua` v6.61, 5581 lines
([forum thread](http://forum.cockos.com/showthread.php?t=176878)),
cloned from ReaTeam/ReaScripts.

The idea: on a modifier press, **colored zones light up over the
selection**, each with a left-drag function and a mousewheel function.
Where the drag *starts* picks the tool. Its zones:

| zone | left drag | mousewheel |
|---|---|---|
| compress from top | compress lane from top | flip values absolute |
| compress from bottom | compress lane from bottom | flip values absolute |
| scale from top | scale values from top | flip values relative |
| scale from bottom | scale values from bottom | flip values relative |
| warp | warp left/right or up/down (whichever the drag commits to) | reset and evenly space |
| stretch left | stretch from left | reverse positions |
| stretch right | stretch from right | reverse positions |
| tilt left | tilt left side | snap to chased values on left |
| tilt right | tilt right side | snap to chased values on right |
| move | move up/down and left/right | flip values absolute |
| undo / redo | — | — |

Tweaks *while the gesture runs* — this is most of why it feels powerful:

- **middle-click** switches curve shape (sine ↔ power for compress/tilt;
  slow-start ↔ slow-end for warp)
- **mousewheel** tweaks curve steepness, with a deliberate pause at the
  centre so neutral is easy to return to
- **right-click** toggles one-sided vs symmetrical
- **Shift** ignores snapping while stretching

Two design points to carry over:

- Float positions and values are **remembered between steps**, so
  repeated edits do not accumulate rounding error from snapping to
  ticks or to 128-step value ranges. Our `Curve` is already `f64`, and
  the drawer's restore-then-reapply preview follows the same principle.
- Mouse position at start selects scope: inside a lane edits that lane's
  values *and* positions; over a lane divider or the ruler edits all
  selected events' positions only.

## Status

Landed (89 core tests):

- `RowSpace` — pitch / drums / strings, with GM drum map, guitar and
  bass tunings, capo, nearest-hand fingering
- `Articulation` — the full Riffer set, with legato and
  valid-fret rules
- note properties — velocity, off-velocity, mute, lyric text,
  articulation, legato, fret
- `MouseMap` — context × gesture × modifiers → action, with fallback to
  the unmodified binding, plus four presets (REAPER-like, Drums, Riffer,
  Lyrics)
- edits — velocity set/nudge, off-velocity, mute/toggle, channel nudge,
  scale length, stretch positions (arpeggiate), copy notes, partial
  quantize, legato, set text, set articulation, set string (re-fingers),
  set fret

Next, in order:

1. **Wire the mouse map into the interaction layer** — it is built and
   tested but the handlers still branch on tool, so nothing uses it yet.
2. **Velocity / CC lane strip** below the roll.
3. **Multi Tool zones** as an overlay mode, per the table above.
4. **Razor edits** — decoded in-tree, not yet modelled here.
5. **Row-space rendering** — drum diamonds, string roll with fret
   numbers, lyric labels, articulation badges.
6. **Note context menu** (Riffer §6.2.2: cut/copy/paste/delete/select
   all/clear/copy measure + note properties).
