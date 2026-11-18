---
"daw-standalone" = "minor"
---

A project has one bridge in a process (a second is refused, keyed by engine and guid), and a live set's doc of a song is kept in step with the song's own doc as a CRDT (sync::relay) instead of bridged to the engine: two bridges on one song re-added its FX chains every round (#316) (session)
