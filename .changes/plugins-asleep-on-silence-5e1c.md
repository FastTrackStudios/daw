---
"daw-standalone" = "minor"
---

Stop calling a plugin while it has nothing to play: a CLAP that says CLAP_PROCESS_SLEEP, or CLAP_PROCESS_TAIL with its tail run out, or an honest CONTINUE_IF_NOT_QUIET (com.fasttrackstudio.sleep/1), is skipped while its input stays silent and woken by audio, a note, a moved parameter or automation; a silent track's bus stays clean up the graph; render.sleep_skipped / render.sleep_saved_ms, and each track's slept blocks in its costs; FTS_PLUGIN_SLEEP=0 turns it off (session)
