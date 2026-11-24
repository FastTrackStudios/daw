---
"daw-standalone" = "minor"
---

Live taps: `Standalone::live_taps().open(project, ProbeAt, frames)` reads a point of a track (its media, before its FX, into or out of an FX, before or after its fader) while the song plays, handed off the audio thread through a preallocated lock-free ring — the audio thread copies each block into it and nothing else (no allocation, no waiting: a busy slot is skipped, a full ring drops the block and counts it) — and `LiveTap::drain` reads it at its own pace
