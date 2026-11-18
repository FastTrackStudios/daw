---
"session-format" = "minor"
---

Folding a song's history into its snapshot (when it is left alone, and every 64 edits) no longer holds the song: only the snapshot is taken with it held, and packing (zstd level 3, from 9) and writing it happen after — an edit's save on Unbreakable waited up to 13.6 s behind a fold, now about 2 s at most. A fold written after a later one never puts the song back. Each fold is traced (`song.compact`: export, pack, write) (session)
