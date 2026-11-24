---
"daw-standalone" = "minor"
"daw-clap-ext" = "minor"
---

Added: an engine offers its embedder's actions to every plugin it loads (`Standalone::set_plugin_load_options`), a CLAP's host-actions subscriptions reach the plugin (`PluginInstance::event_sink`, `EventSink::same_plugin`), `PluginInstance::wants_main_thread`, and `Standalone::with_plugin_map` with the map's generation, so a host reads a plugin's thread-safe values beside a render (signal)
