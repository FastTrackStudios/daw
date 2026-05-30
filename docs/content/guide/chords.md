+++
title = "Chords"
description = "How to write a single chord in Keyflow: the root (letter, number, or numeral), then quality, seventh family, extensions, alterations, and slash bass."
weight = 3
+++

A chord is a **root** followed by an optional **descriptor** that says what's
built on top of it:

```
C        F#m7       Bbmaj9       G7b9       Dm7b5/F
└root    │ │        │   │        │  │       │    └ bass
         │ └family  │   └ext     │  └alt    └ everything else
         └quality   └root        └family
```

This page is about writing **one chord, on its own**. (How chords carry their
quality forward from bar to bar — "chord memory" — comes later.) Everything
after the root reads left to right: quality → seventh family → extensions →
alterations → additions/omissions → slash bass.

## The root

The root can be written three ways, and they're interchangeable — pick whatever
fits the chart:

| System | Example | Notes |
| ------ | ------- | ----- |
| **Letter name** | `C`, `F#`, `Bb` | Absolute pitch. Accidentals: `#` sharp, `b` flat. |
| **Nashville number** | `1`, `4`, `5` | Scale degree, relative to the song's key. |
| **Roman numeral** | `I`, `IV`, `V` | Scale degree, relative to the song's key. |

Numbers and numerals are **relative to the key** set in the header (see
[Structure](/guide/structure/)) — `1` in `#C` is C, `1` in `#G` is G. That's
what makes a Nashville or Roman chart transposable.

The same descriptor works on any root, so every chord below could equally be
written `Cmaj7`, `1maj7`, or `Imaj7`.

## Quality — the triad

Quality is the basic three-note shape. **Major is the default** — a bare root is
a major triad.

| Quality | Write | Example |
| ------- | ----- | ------- |
| Major (default) | *(nothing)* | `C` |
| Minor | `m` | `Cm` |
| Diminished | `dim` | `Cdim` |
| Augmented | `aug` | `Caug` |
| Suspended 2nd | `sus2` | `Csus2` |
| Suspended 4th | `sus4` | `Csus4` |
| Power chord (no 3rd) | `5` | `C5` |

### Quality on numbers and numerals

For **Roman numerals**, case carries the quality — **uppercase is major,
lowercase is minor**:

```
I  ii  iii  IV  V  vi  vii      → I, iim, iiim, IV, V, vim, viim
```

For **Nashville numbers**, a bare number is major; add `m` for minor:

```
1  2  6        → major
1m  2m  6m     → minor
```

## The seventh — chord family

Adding a seventh puts the chord in a *family*. A chord with no seventh is just a
triad.

| Family | Write | Example | Meaning |
| ------ | ----- | ------- | ------- |
| Major 7th | `maj7` | `Cmaj7` | major triad + major 7th |
| Dominant 7th | `7` | `C7` | major triad + flat 7th |
| Minor 7th | `m7` | `Cm7` | minor triad + flat 7th |
| Minor-major 7th | `mM7` | `CmM7` | minor triad + major 7th |
| Half-diminished 7th | `m7b5` | `Cm7b5` | diminished triad + flat 7th |

## Extensions — 9th, 11th, 13th

Extensions stack thirds above the seventh. Writing `9`, `11`, or `13` on a plain
root **implies a dominant 7th** underneath (so `C9` is `C7` + a 9th); combine
with `maj`/`m` to keep a major- or minor-7th underneath.

| Write | Example | Is |
| ----- | ------- | -- |
| `6` | `C6` | major triad + added 6th |
| `9` | `C9` | dominant 7th + 9th |
| `11` | `C11` | dominant 7th + 11th |
| `13` | `C13` | dominant 7th + 13th |
| `maj9` | `Cmaj9` | major 7th + 9th |
| `m9` | `Cm9` | minor 7th + 9th |

## Alterations

Alterations sharpen or flatten a single tone — most often the 5th, 9th, 11th, or
13th. Write the accidental (`b`/`#`) directly before the degree:

| Write | Example |
| ----- | ------- |
| flat 5th | `C7b5` |
| sharp 5th | `C7#5` |
| flat 9th | `C7b9` |
| sharp 9th | `C7#9` |
| sharp 11th | `C9#11` |
| flat 13th | `C7b13` |

## Additions and omissions

- **Add a tone** without implying the notes below it: `add` — `Cadd9` is a major
  triad plus a 9th, with *no* 7th.
- **Remove a tone**: `no` — `C7no3` drops the 3rd, `Cno5` drops the 5th.

## Slash bass

Put a bass note other than the root after a `/`:

```
C/E        major triad over E
Dm7/G      Dm7 over G
F/A        F over A
```

The bass takes the same systems as the root, so `1/3` (Nashville, the 1 chord
over the 3rd degree) means the same thing as `C/E` in C.

### One exception: `V/V` in Roman numerals

There's a single twist. Between two **Roman numerals**, `/` doesn't mean slash
bass — it's a **secondary (applied) chord**, the way analysts write them:

```
V/V        "five of five" — the dominant of the dominant
V/vi       the dominant of the vi chord
V7/V       …with a seventh
```

`V/V` reads as "the V chord *in the key of* V." In C, the second `V` is G, so
`V/V` is the dominant of G — a **D** chord (`V7/V` is D7). It's a chromatic chord
that pulls toward the V, not a G over a G bass.

This only applies to **Roman `/` Roman**. A Roman numeral over a note or a
number — `I/3`, `V/B` — is still an ordinary slash bass, as are all letter-name
(`C/E`) and number (`1/3`) slashes.

## Putting it together

The pieces stack in order — root, quality, family, extensions, alterations,
bass:

```
Am7         A  + minor + 7th
Cmaj9       C  + major 7th + 9th
G7b9        G  + dominant 7th + flat 9th
F#m7b5      F# + half-diminished 7th
Bbmaj7/D    Bb + major 7th, over D
```

## What's next

- **Notation Systems** — the three interchangeable ways to write the root
  (letter names, Nashville numbers, Roman numerals), how they relate to the
  key, and how flats and the ambiguous `b7` are resolved.
