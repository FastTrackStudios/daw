---
"daw-standalone" = "minor"
---

`PluginInstance::reset`: a plugin clears what it carries from the audio it processed (buffers, voices, envelopes) and stays activated — a hosted CLAP through CLAP's `reset` — so an offline render reusing a copy starts it as from silence without activating it again; `false` for a plugin that cannot, which its caller deactivates instead. `Probe` and `ProbeAt` hash (a caller keys recordings by them)
