---
"daw-standalone" = "minor"
---

A CLAP hosted from its bundle keeps its state: Effects::state_chunk / set_state_chunk read and load the plugin's own state, so a song saved through its project file and opened again has it; the .session writer puts a CLAP's state in its block
