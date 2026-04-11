# vault-core Spec Index

The vault-core spec is split across focused files:

- [task.md](task.md) — Task schema, status, priority, urgency, dates, time tracking, recurrence, dependencies, reminders, external integration
- [project.md](project.md) — Project and area schema, hierarchy, workflow integration
- [query.md](query.md) — Query engine: filters, sorting, grouping, execution model
- [plugin.md](plugin.md) — Obsidian WASM plugin API
- [ios.md](ios.md) — iOS app: lock screen widgets, home screen widgets, deep links
- [api.md](api.md) — VaultService Rust API trait, Vox RPC transport, VaultError types
- [views.md](views.md) — Core views: Today, Inbox, Upcoming; empty states; hidden-by-start rule
- [sync.md](sync.md) — Vault sync strategy: file watching, conflict resolution, iCloud, offline queue, atomic writes
- [capture.md](capture.md) — Task creation UX: quick-add bar, NLP parsing, voice, lock screen queue
- [integrations.md](integrations.md) — Domain integration framework: status sets, project/task templates, TOML schema
- [integration-fts.md](integration-fts.md) — Fast Track Studio integration: recording project statuses and templates
- [integration-fitness.md](integration-fitness.md) — Fitness integration: workout recurrence, rest days, training cycle template
- [integration-music-practice.md](integration-music-practice.md) — Music Practice integration: daily recurrence, piece lifecycle, Piece Study template
- [integration-learning.md](integration-learning.md) — Learning integration: spaced repetition, course template, milestone tasks
