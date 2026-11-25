---
"daw-standalone" = "minor"
"daw-clap-ext" = "minor"
---

A loaded plugin says what changed without its lock (PluginNotices: a CLAP's request_callback, rescans and mark_dirty), each cell keeps its instance id, a touch count, its editor's state and its signal/1, and a song's fingerprint reads a plugin's state again only when it may have changed; com.fasttrackstudio.signal-revisions/1 gives Signal's revisions from any thread; the media read-ahead scans nothing while the playhead and song hold still (session)
