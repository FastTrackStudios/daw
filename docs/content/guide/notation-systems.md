+++
title = "Notation Systems"
description = "Keyflow's three interchangeable ways to name chords and melody: letter names, Nashville numbers, and Roman numerals — including how flats and the ambiguous b7 are resolved."
weight = 3
+++

A core idea in Keyflow: the same music can be written three ways, and they're
**fully interchangeable**. Everything after the root — quality, sevenths,
extensions, alterations, slash bass (see [Chords](/guide/chords/)) — works
identically in all three. You only choose how to spell the *root*.

| System | Same four bars | Reads as |
| ------ | -------------- | -------- |
| **Letter names** | `C  F  Am  G` | absolute pitches |
| **Nashville numbers** | `1  4  6m  5` | scale degrees in the key |
| **Roman numerals** | `I  IV  vi  V` | scale degrees in the key |

Use whichever fits the chart — letter names for a fixed-key lead sheet, numbers
for a transposable worship/Nashville chart, Roman numerals for analysis or
classical feel.

## Letter names

Absolute pitch. The root is a note `A`–`G` with an optional accidental, and the
quality is written out explicitly:

```
C    F#    Bb    Am    Cmaj7    F#m7b5
```

Letter names don't depend on the key — `C` is always C.

## Nashville numbers

The root is a **scale degree** `1`–`7`, relative to the song's key. A bare
number is major; add `m` for minor:

```
1   2   4   5      major
1m  2m  6m         minor
```

So in the key of C, `1 4 5` is C–F–G; in the key of G, the *same* `1 4 5` is
G–C–D. That's the point of the number system — the chart is the progression,
independent of key.

## Roman numerals

Also a scale degree relative to the key, but **case carries the quality** —
**uppercase is major, lowercase is minor**:

```
I  ii  iii  IV  V  vi  vii      →  I, iim, iiim, IV, V, vim, viim
```

You can still add explicit descriptors on top: `Imaj7`, `V7`, `iim7`.

## Readability: the `:` separator

When a number or numeral is followed by a quality that *starts with a digit*,
the two runs of digits can be hard to read — is `17` "degree 1, seventh" or the
number seventeen? Optionally put a colon between the root and the quality:

```
1:7      4:maj9      2:m7      5:9
```

It's purely for readability and carries no meaning — `1:7` and `17` parse
identically, as do `4:maj9` and `4maj9`. The colon works on **all three
systems** — numbers (`4:maj9`), Roman numerals (`V:7`, `i:m7`), and letter
names (`C:7`) — though it matters most for numbers.

**Good practice: write the `:`.** `1:7` reads cleanly; `17` is correct but
easy to misread. (A future editor will insert the `:` for you automatically.)

## Relative to the key

Both number-based systems resolve against the key set in the
[header](/guide/structure/) (`#C`, `#Gm`, …). Because the chart stores *degrees*
rather than fixed pitches, transposing a Nashville or Roman chart is just
changing the key in the header — every chord follows.

Letter names are the opposite: fixed pitches that ignore the key.

## Accidentals and borrowed chords

Put `#` (sharp) or `b` (flat) before a degree or numeral to raise or lower it —
exactly how you write a borrowed or chromatic chord:

```
1  b3  4  b7        (numbers)   ♭3 and ♭7 borrowed
I  bIII  IV  bVII   (Roman)     ♭III and ♭VII borrowed
1  #4  5            sharpened 4th
```

Since almost no one can type a real ♭ glyph, Keyflow treats the plain letter
**`b` as a flat** in these positions.

### The `b7` question

That creates one genuine ambiguity: **`b7`** could mean the *note* B with a
7th (`B7`), or the *flat-7 degree* (`♭7`). Keyflow resolves it from the
surrounding notation system:

| Context | `b7` means |
| ------- | ---------- |
| Letter-name chart (`C  F  b7  G`) | the chord **B7** |
| Number chart (`1  4  b7  5`) | the **♭7** degree |
| Roman chart (`I  IV  b7  V`) | the **♭7** degree |
| No surrounding context | the chord **B7** |

The current line decides first; if it can't tell, the rest of the chart does;
failing that it's read as the note B. Only `b5`/`b6`/`b7` are ever ambiguous —
`b9` and up can only be the note B (degrees stop at 7), and any `#`-prefixed
number is always a degree.

## Mixing systems

You can borrow a Roman or number chord inside a letter chart — `bVII` and `#IV`
are unambiguous, so `Bb  bVII  Eb` reads the `bVII` as the flat-7 degree while
the rest stay letter names.

## The same for melody

These three systems aren't just for chords — melody notes use the same letter /
number / numeral choice, covered in the Melody page.

## What's next

- **Rhythm** — how long each chord lasts, and why a bare chord fills a whole bar.
