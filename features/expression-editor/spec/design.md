# Expression Editor — design

One editing surface, two products:

- **an MPE editor** — per-note pitch bend, channel pressure, and CC74
  edited as properties *of a note* instead of in raw controller lanes;
- **a Melodyne competitor** — an analyzed audio clip's notes carrying a
  tracked f0 contour, edited by exactly the same gestures.

## Why they are one editor

A note in either domain is the same object:

> an integer pitch row, plus a continuous curve measured in semitones
> **relative to that row**.

For MPE the curve is the per-note bend. For audio it is the tracked f0
minus the rounded center. The note rectangle always stays on its literal
integer row and the curve carries the sounding offset — which is
simultaneously the right answer for microtonal MIDI display and for a
sung note that is 30 cents flat.

Everything else follows. Q zones, drag-to-transpose, alt-drag scaling,
curve reshaping, the drift/vibrato sliders, snapping against a
temperament: all of it is defined once against that shape.

## Crates

| crate | what |
|---|---|
| `expression-editor-core` | portable engine — document, camera, tuning, curve shaping, edits + undo, hit testing, modulation stack. **Zero dependencies.** |
| `expression-editor-ui` | Dioxus surface — canvas, toolbar, status bar. Inline styles only. |

The split is the one thing MPElodyne (the ReaImGui MPE script this
borrows its interaction model from) got structurally right: a pure
engine with a command-line regression suite, and a GUI on top. The
engine's 62 tests run with no GPU, no DAW, and no browser.

`core` is dependency-free on purpose — it has to compile for wasm, for
Blitz-native plugin builds, and eventually for embedded hosts. Domain
adapters (a REAPER MIDI take; `tune_dsp::PitchDoc`) live with their
domains, never here.

## The Melodyne decomposition

```text
pitch(t) = center + drift(t)·drift_amount + modulation(t)·modulation_amount
```

`tune_dsp::model::NoteBlob` stores this as truth for analyzed audio.
`expression_editor_core::blob` **derives it on demand** from whatever
curve is there, so Melodyne's two headline sliders work on a hand-drawn
MPE bend that was never analyzed. Raw points stay the source of truth in
both domains; `decompose → recompose` round-trips exactly, so opening
the controls is never destructive.

Zones scale around their **effective center** — where the curve
actually dwells (mode over semitone bins, refined by median), not the
note's row and not the mean. A scoop that starts a fourth low and
settles on target expands about the target.

## Camera

MPElodyne's own `View Magnets.md` ends with a list of "likely roughness
sources", and every one has the same cause: rules that mutate an
already-produced camera in sequence, so a later rule fights an earlier
one every frame.

This inverts that. A gesture produces one base camera, then declares its
magnets as weighted **influences** — candidate cameras, not mutations.
`camera::blend` resolves them in a single pass and `Camera::constrain`
clamps once at the end. Two magnets pulling opposite ways now average
smoothly instead of alternating.

Scales blend **geometrically**; a linear average of `units_per_px` would
bias every blend toward zoomed-out.

Magnet weights, kept from the shipped feel:

- edge magnet: inert through the inner 35% of the item half-span, full
  at the edge, framing it with 20% whitespace
- reset tail: inert until 80% of the way to Reset View, then smoothstep
  to full — and only while zooming *out*
- pitch focus: 45% toward notes near the pointer, 22% toward the
  pointer's own pitch
- deep-zoom center pull: begins at 72% of the vertical range

## MPE safety

Expression ownership is reconstructed from channel and note lifetime.
When two sounding notes share a channel the expression in the overlap is
genuinely ambiguous — `ExpressionDoc::mark_ambiguity` flags both, the UI
draws them red, and the writer must refuse rather than guess.

Channel assignment treats *touching* notes as conflicting, not only
overlapping ones: reusing a channel the instant a note ends means the
incoming note's setup expression lands while the outgoing release is
still sounding. Channel 1 stays free as the MPE master.

## Status

Landed: core engine (62 tests), Dioxus surface (6 SSR tests), wasm-clean.

Open:

- **domain adapters** — MIDI take ↔ `ExpressionDoc`, and
  `tune_dsp::PitchDoc` ↔ `ExpressionDoc`
- **audition** — nothing sounds yet; needs a host hook
- **modulation drawer** — `core::modulation` is built and tested, but
  has no UI
- **wheel anchoring** — a wheel event carries no pointer position, so
  zoom currently anchors on the canvas center
- **time warp** — the melonix marker model exists in `tune_dsp`; this
  document has no time-warp term yet
