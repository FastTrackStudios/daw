---
"daw-standalone" = "minor"
---

An offline render of part of a song: `Offline::only` renders the tracks named and leaves every other one empty (no media, no FX), `Offline::probes` reads any track's signal at any point along it each block (its own media, before its FX, into or out of any FX, before or after its fader; `with_probes`, each one's lateness by `probe_latencies`), and `Offline::fed` plays a track's output recorded by an earlier render of the same frames in its place, so what feeds only it is not rendered again. `track_prints` fingerprints each track as a render reads it (the listening tools' cache key)
