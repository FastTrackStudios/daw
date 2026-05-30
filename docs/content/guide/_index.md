+++
title = "Guide"
description = "Learn Keyflow one concept at a time, from a song's header to chords, rhythm, lyrics, and melody."
weight = 0
sort_by = "weight"
+++

A hands-on tour of the `.kf` format. Each page introduces one concept and builds
on the last, so by the end you can read and write a complete chart.

Keyflow is **plain text**. You can type a chart in any editor, paste it into a
chat, commit it to git, or generate it from MIDI — and it renders to the same
lead sheet either way. The format is designed to be playable *as-is*, without
tooling.

A taste of what a finished chart looks like:

```
Vienna (Live) - Billy Joel
4/4 140bpm #Gm

VS
Gm Bb F Ab Eb Bb C D11

CH
Bb F Ab Eb
```

That's two ideas: a **header** describing the song, then **sections** of music.
Notice there are no bar lines — `Gm Bb F Ab` is simply four bars, one chord each.
Keyflow uses rhythm modifiers (not `|`) to say how long a chord lasts; a bare
chord just fills its bar.

## The guide

1. [Structure](/guide/structure/) — the document: title, artist, time signature, tempo, key.
2. [Sections](/guide/sections/) — organizing the song: section names, lengths in bars, repeating a part, labels, and custom sections.
3. [Chords](/guide/chords/) — writing a single chord: root, quality, seventh family, extensions, alterations, slash bass.
4. [Notation Systems](/guide/notation-systems/) — the three interchangeable ways to name roots: letter names, Nashville numbers, Roman numerals.
5. [Rhythm](/guide/rhythm/) — how long each chord lasts: the one-chord-per-bar default, slashes, `()` groups, and note-value durations.
6. [Melody](/guide/melody/) — writing the tune line: notes as letters or numbers, octaves, durations, stacked notes, and pairing it with the chords.
7. [Lyrics](/guide/lyrics/) — words under the chords: a `[lyrics]` line, `{Chord}` markers on syllables, and hyphen splits for melisma.
8. [Key & Meter Changes](/guide/key-meter-changes/) — moving to a new key or time signature mid-song, and the `!T` one-bar meter change.
9. [Annotations & Expression](/guide/annotations/) — staff text, instrument cues, dynamics, and crescendo/decrescendo hairpins.

## Two things to know up front

- **One chord per bar by default.** Space-separated chords each take a whole
  measure. You only reach for rhythm modifiers (slashes, durations) when a bar
  holds more than one chord or an off-beat feel — that's a later page.
- **Three ways to name a chord, everywhere.** Letter names (`G`, `Cmaj7`),
  Nashville numbers (`1`, `4`), and Roman numerals (`I`, `IV`) are all
  first-class, for both chords and melody. Pick the one that fits the chart.
