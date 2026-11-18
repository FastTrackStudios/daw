---
"session-format" = "patch"
---

A .session file's writes wait for another connection's lock instead of failing, so a sync replica no longer drops a change written while the file is busy (session)
