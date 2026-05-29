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
2. [Chords](/guide/chords/) — writing a single chord: root, quality, seventh family, extensions, alterations, slash bass.

*(More pages land here as the guide grows: Rhythm, Sections, Lyrics, and
Melody.)*

## Two things to know up front

- **One chord per bar by default.** Space-separated chords each take a whole
  measure. You only reach for rhythm modifiers (slashes, durations) when a bar
  holds more than one chord or an off-beat feel — that's a later page.
- **Three ways to name a chord, everywhere.** Letter names (`G`, `Cmaj7`),
  Nashville numbers (`1`, `4`), and Roman numerals (`I`, `IV`) are all
  first-class, for both chords and melody. Pick the one that fits the chart.
