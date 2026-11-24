---
"daw-standalone" = "minor"
---

The engine offers host-timeline/1 to its CLAPs (LoadOptions::timeline, each instance bound to its track with bind_timeline), builds a track's timeline as the render plays it (Standalone::track_timeline: items merged at their positions, cut at their ends, looped, mutes left out, the tempo map with the renderer's seconds), stops delivering a claimed track's item MIDI to its chain (set_track_midi_claimed; live MIDI still reaches it), and tells a plugin the transport is stopped when it renders while stopped (session)
