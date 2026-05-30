+++
title = "Repeats & Endings"
description = "Saying it once in Keyflow: repeat a bar with %, a line with xN, a span with repeat barlines |: :|, and first/second endings with [1] and [2]."
weight = 10
+++

Songs loop. Rather than write the same bars again, Keyflow has a few ways to say
"play that again."

## Repeat a bar — `%`

`%` replays the bar before it (from [Rhythm](/guide/rhythm/)):

```
1  4  %  5        bar 3 repeats bar 2 (the 4)
```

## Repeat a line — `xN`

Put `x` and a number at the end of a line to play the whole line that many times:

```
1 4 5 1 x2        these four bars, played twice (eight bars)
```

## Repeat a span — `|: … :|`

Wrap bars in repeat barlines to mark a section that plays twice:

```
|: 1 4 | 5 1 :|
```

The `|: … :|` only marks the repeat — the bars are still written once. Use bar
lines `|` between them when the span is more than one bar (as above), since a
bare `|: 1 4 5 1 :|` would pack everything into a single bar.

## First and second endings

When a repeat ends differently the second time, mark the alternate endings with
`[1]` and `[2]` — placed right after a bar line, on the bars they apply to:

```
|: 1 | [1] 4 :| | [2] 5 |
```

That reads: play the `1` bar, take the **first ending** (`4`) and repeat back;
the second time through, skip to the **second ending** (`5`). Write `[1,2]` for a
bar shared by both endings.

## That's the whole tour

You can now read and write every part of a Keyflow chart — the
[header](/guide/structure/) and [sections](/guide/sections/), the
[chords](/guide/chords/), [rhythm](/guide/rhythm/), [melody](/guide/melody/), and
[lyrics](/guide/lyrics/), the [key and meter changes](/guide/key-meter-changes/),
the [markings](/guide/annotations/), and now the repeats that tie a song's form
together. Open a real `.kf` file and start playing with it — that's the best
teacher from here.
