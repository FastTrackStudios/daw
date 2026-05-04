+++
title = "Keyflow"
description = "Open chart notation language and GPU-accelerated rendering engine"
+++

Keyflow is a plain-text notation format for lead sheets, chord charts, and rhythm charts.

It parses human-readable chart syntax into a structured document model, then lays out and renders publication-quality output with GPU-accelerated vector graphics via Vello.

## Getting Started

- [Architecture](/architecture/) — How Keyflow is structured internally
- [Melody Pipeline](/melody-pipeline/) — The melody notation and rendering pipeline

## Features

- **Plain-text format** — version-controllable, diffable, portable
- **Smart chord memory** — chords carry forward automatically
- **Section numbering** — repeat/coda navigation built in
- **GPU-rendered** — sub-millisecond layout passes via Vello
- **Multiple exports** — SVG, PNG, and PDF output
- **Syntax highlighting** — editor integration support
