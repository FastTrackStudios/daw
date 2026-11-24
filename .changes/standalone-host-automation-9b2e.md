---
"daw-standalone" = "minor"
---

The CLAP host offers `com.fasttrackstudio.host-automation/1` when its embedder gives it automation (`ClapHost::with_automation`, `LoadOptions::automation`): each call names the calling plugin (the instance's own `clap_plugin`), and one made before the plugin finished `init` is refused.
