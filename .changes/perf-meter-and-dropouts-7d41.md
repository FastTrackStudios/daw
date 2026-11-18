---
"audio" = "patch"
"daw-standalone" = "minor"
---

A performance meter, like REAPER's: the engine's CPU, the audio callback's (RT) and the render ahead's, the longest block, xruns and dropouts, disk read and write, RAM and system free, and every track's FX CPU, media CPU, FX count and PDC. Shown in a window panel (`SESSION_PERF_METER`), read with `SESSION_PERF`, and exported as metrics (`audio.track.fx_cpu` / `.media_cpu` / `.pdc` by `track.name`, `audio.rt_cpu`, `process.*`). Dropouts are measured as the time the device went without audio (`audio.gap`, frames delivered against the clock, so a callback that blocked or never ran counts too), with `audio.smooth` (1 per interval with no dropout), every dropped block as an `audio.dropout` span at the time it happened, and the slowest render's stages (`audio.worst_block.duration`) (session)
