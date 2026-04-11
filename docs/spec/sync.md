# Sync Spec

## Vault Location

### t[sync.vault-root]
On first launch the user is prompted to select a folder that will serve as the vault root. The chosen path is persisted in app settings and used for all subsequent reads and writes. The default suggestion is `~/Documents` on macOS/iOS; if an Obsidian vault is detected at a common location it is offered as an alternative. The setting can be changed later in preferences, which triggers a full reload of the in-memory snapshot from the new location.

---

## File Watching

### t[sync.file-watch]
vault-core registers a file-system watcher using FSEvents on macOS/iOS and inotify on Linux. When a `.md` file inside the vault root is created, modified, or deleted by an external process, the watcher fires an event. The affected task or project file is re-read and the in-memory snapshot is updated within 500 ms of the event. Batch events within a 100 ms debounce window are coalesced into a single reload pass.

---

## Conflict Resolution

### t[sync.conflict]
vault-core uses a last-write-wins strategy for concurrent edits to the same file. When two versions of a file are compared, the one with the later `dateModified` value is kept. If `dateModified` values are identical (for example, because two clients wrote within the same second), the longer file by byte count is preferred. There is no three-way merge; the losing version is discarded. Users who need richer conflict resolution should rely on a version-controlled sync backend (e.g., git) external to vault-core.

---

## iCloud Drive

### t[sync.icloud]
iCloud Drive is supported as a vault location without any special handling by vault-core. On macOS and iOS the vault root may be set to a path inside `~/Library/Mobile Documents` (the iCloud Drive container). vault-core reads and writes files there identically to any local path. iCloud's own eviction and download machinery may cause files to be offline; vault-core surfaces `IoError` in that case rather than silently returning stale data. Users are responsible for ensuring files are downloaded before offline use.

---

## Offline Queue

### t[sync.offline-queue]
On iOS, mutations made while the vault root is inaccessible (e.g., iCloud files not downloaded, vault on an unmounted volume) are serialized as JSON objects and stored in the App Group shared container under `pending/`. Each pending entry records the operation type (`create`, `update`, `complete`), the task fields, and the timestamp. When the vault becomes accessible again, the queue is replayed in order, applying each mutation atomically. Conflicts between queued operations and external changes are resolved by the standard last-write-wins rule.

---

## Atomic Writes

### t[sync.atomic-write]
All file writes by vault-core are performed atomically. The new content is written to a temporary file with the suffix `.md.tmp` in the same directory as the target. Once the write is flushed and synced, the temporary file is renamed over the target `.md` file. This guarantees that readers never observe a partially written file, even if the process is interrupted mid-write.

---

## Date Modified

### t[sync.date-modified]
The `dateModified` field in task frontmatter is updated to the current UTC datetime on every save, whether the save originates from a user action, an API call, or an offline-queue replay. This timestamp is the authoritative signal used by `t[sync.conflict]` for last-write-wins arbitration and must not be set to a past value by any write path.
