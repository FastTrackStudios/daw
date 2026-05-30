+++
title = "Melody"
description = "Writing a melody line in Keyflow: notes as letters or scale numbers, relative octaves with nudges and pins, the same durations as chords, stacked notes, and pairing a tune with the chords above it."
weight = 5
+++

A melody is the tune line, written **alongside** the chords rather than instead
of them. It uses the same note names you already know and the same durations as
[Rhythm](/guide/rhythm/) — so once you can write a chord chart, a melody is a
short step away.

Notes live in a melody block, `m{ … }`:

```
m{ C D E F }
```

That's four notes — C, D, E, F.

## Writing notes

A note is a **pitch** and, optionally, a duration. The pitch is written one of
two ways, exactly like a chord root:

- **Letter names** — `C D E F G A B`, with `#` sharp, `b` flat, `n` natural:
  ```
  m{ C F# Bb Dn }
  ```
- **Scale numbers** — `1`–`7`, relative to the key, with the same accidentals:
  ```
  m{ 1 2 3 4 5 }        in C: C D E F G
  ```

Pick whichever suits the chart — letters for a fixed key, numbers for a
transposable one. (Roman numerals name *chords* but not melody notes.)

## Octaves

By default octaves are **relative**: each note lands in whichever octave puts it
nearest the note before it. You write the line and the leaps sort themselves out
— a `C` after a `B` sits just above it, not seven steps down.

When you need to steer the octave, three tools, lightest first:

- **Nudge** with `'` (up) and `,` (down) — push one note into the next octave:
  ```
  m{ C C, C C, }        the 2nd and 4th C drop an octave
  m{ G A B C'' }        C'' jumps two octaves up
  ```
- **Pin** an absolute octave with `(N)`:
  ```
  m{ C(4) D E F }       start on C4; the rest follow relatively
  ```
- **Set a starting octave** for a whole block or section with `/octave`:
  ```
  /octave 4
  m{ C D E F }
  ```

## Durations

Melody durations are the **same** as for chords (see [Rhythm](/guide/rhythm/)):
write the note value after the pitch — `8` eighth, `4` quarter, `2` half, `16`
sixteenth — with `.` for dotted and `t` for a triplet:

```
m{ C8 D8 E8 F8 }        four eighth notes
m{ C4. D8 E4 }          dotted quarter, eighth, quarter
m{ C8t D8t E8t }        an eighth-note triplet
```

A bare number is the **duration**, so `C4` is a quarter-note C (an octave is
`C(4)`). You can also write the value after an underscore — `C_8` is the same as
`C8` — which reads more clearly when a pitch already carries an octave.

Like chords, a duration **sticks** until you change it, so a run of equal notes
needs it only once:

```
m{ C8 D E F G A B C }   one eighth marks them all
```

Three more tokens fill the gaps between notes:

| Write | Means |
| ----- | ----- |
| `r4` | a quarter **rest** |
| `s4` | a **space** — a silent placeholder, no rest drawn |
| `~` | a **tie** — `C4~ ~C4` holds one C across the two |

## Stacked notes

Wrap notes in `< … >` to sound them **together**, read low to high — a melody
note with harmony notes stacked above it:

```
m{ <C E G> }            a C-major shape on one stem
m{ <F# 'C#> <G# 'D#> }   two stacked pairs
```

Inside a group, `'` and `,` adjust an individual note's octave, so `<F# 'C#>`
puts C# an octave above where it would otherwise land.

## Pairing a melody with chords

A melody and the chords beneath it run in **parallel**, separated by `;` inside
`<< … >>`. The chords are one lane, the melody the other:

```
<< C ;  m{ C8 D8 E8 F8 G8 A8 B8 C8 } >>
```

The chords and the melody are the same length — here one bar of C under an
eighth-note run.

For anything longer than a line, the multi-line form reads better:

```
<<
  Am  F ;
  m{ C8 D E F E4 C  E8 F G A G4 E }
>>
```

### Whole songs: lanes

For a full chart, keeping chords and melody on one line gets unwieldy. Split them
into named **lanes** and pair the lanes once. The top section list owns the
section lengths; each lane just repeats the section name:

```
intro 4

let chords = {
  intro
  C  F  G  Am
}

let melody = {
  intro
  /octave 4
  C8 D8 E8 F8 G8 A8 B8 C8
  E4 F4 G4 A4
}

<< <chords> ; <melody> >>
```

Inside a `melody` lane the notes are written bare — no `m{ }` wrapper — because
the whole lane is already melody.

## That's the tour

You can now read and write a complete Keyflow chart: a [header](/guide/structure/),
[chords](/guide/chords/) in any of the [three systems](/guide/notation-systems/),
their [rhythm](/guide/rhythm/), and a melody on top. From here, the best next
step is to open a real chart and start changing things.
