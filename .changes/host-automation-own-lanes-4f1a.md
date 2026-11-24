---
"daw-clap-ext" = "minor"
---

`com.fasttrackstudio.host-automation/1`: a plugin reads and writes the host's automation lanes of its own parameters — `lanes`, `read`, `write` (replace `[from, to)`, with `CREATE_LANE`, `SHOW_LANE`, `KEEP_USER_EDITS`), `clear`, and `begin_edit`/`end_edit` for one undo step. Points are `{t (project seconds), value (normalized), shape, tension?, by: plugin|user}`; a user's edit arrives on the `automation` topic. The C header, both sides in Rust (`automation::{HostAutomation, HostAutomationVtable, HostAutomationClient}`), `merge_points` (the one rule for which user points survive a write) and `EventSink::plugin_key`.
