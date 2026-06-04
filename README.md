# Keyflow

Plain-text chord charts that engrave themselves. A diffable source format
(`.kf`) for lead sheets, rhythm charts, and melodies — rendered to
publication-quality SVG and PDF.

```
Vienna (Live) - Billy Joel
4/4 120bpm #Gm

VS
Gm | A# | F | Gm
[lyrics] {Gm}Slow down you {A#}crazy child
```

**Docs: [keyflow.fasttrackstudio.app](https://keyflow.fasttrackstudio.app)** —
the [format guide](https://keyflow.fasttrackstudio.app/guide/) and
[architecture](https://keyflow.fasttrackstudio.app/architecture/), with every
chart engraved live by this repo's engraver.

## Quick start

```bash
kf pdf song.kf -o song.pdf    # engrave to PDF
kf svg song.kf -o song.svg    # engrave to SVG
```

```rust
let chart = keyflow::parse("My Song\n120bpm 4/4 #C\n\nVS\n1 | 4 | 5 | 1")?;
```

Web editor: `dx serve --package web-editor --platform web`

## License

See [LICENSE.md](./LICENSE.md)
