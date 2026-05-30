+++
title = "Lyrics"
description = "Writing words under the chords in Keyflow: a [lyrics] line in a section, putting chords on the syllable they fall on with {Chord}, and splitting words across notes with hyphens."
weight = 7
+++

Words go under the music as a **`[lyrics]` line** inside a section — right below
the chords they're sung against:

```
VS 4
C  C  F  C
[lyrics] Twinkle twinkle little star
```

`[lyrics]` is a reserved marker (the words for the section above it), not a
section of its own — unlike the custom `[Name]` headers from
[Sections](/guide/sections/).

## Lining chords up with words

To show exactly where a chord changes mid-line, put it in `{curly braces}` right
before the syllable it lands on:

```
[lyrics] {C}Hello {G}world {Am}how {F}are you
```

The chord sits with the word, so a singer reads the change at the moment it
happens. Words without a brace just carry on under the chord before them:

```
[lyrics] {Gm}Slow down you {A#}crazy child
```

Here `Slow down you` are all under Gm, and `crazy child` under A♯.

## Splitting a word across notes

When one word is sung over several notes or chords, break it with **hyphens** —
each piece is its own syllable:

```
[lyrics] A-ma-zing grace how sweet
```

That's six syllables — `A`, `ma`, `zing`, `grace`, `how`, `sweet` — with the
first word stretched across three. Hyphens and `{chords}` combine, so a chord can
land on any syllable:

```
[lyrics] {Cmaj7}A-{Dm7}ma-{G}zing grace
```

## More than one verse

Stack a `[lyrics]` line for each set of words that shares the same chords:

```
CH 4
F  C  G  Am
[lyrics] first time round we sing this line
[lyrics] second time the words are new
```

## That's the whole format

You now have every layer of a Keyflow chart: the [header](/guide/structure/), the
[sections](/guide/sections/) that organize the song, [chords](/guide/chords/) in
any of the [three systems](/guide/notation-systems/), their
[rhythm](/guide/rhythm/), a [melody](/guide/melody/), and the lyrics underneath.
Open a real `.kf` file and start changing things — it renders the same lead sheet
either way.
