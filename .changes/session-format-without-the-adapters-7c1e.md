---
"session-format" = "none"
"dawfile-reaper" = "none"
"daw-standalone" = "none"
---

Changed the session document to stop depending on the REAPER and DAWproject adapters: the `.rpp` and DAWproject import and export are the adapters' (`dawfile_reaper::session`, `interchange::dawproject::session`); the site no longer links the engine, and `daw-actions` no longer links `studio` (session)
