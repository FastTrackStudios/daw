# Claude

This project's agent instructions live in [AGENTS.md](AGENTS.md). Read it first.

Key callouts (all detailed in AGENTS.md):

- **Architecture**: feature trios (`proto` / `crdt` / `db` / `ui` / facade) on Loro CRDT + Dioxus + fts-ui design system.
- **UI rules**: fts-ui primitives only, theme tokens never hex, dark mode default, dumb components.
- **Common gotchas**: lucide names (`CircleCheck` not `CheckCircle2`), `StatusBadgeVariant` only `Success`/`Warning`/`Danger`/`Neutral`, `.peek()` vs `.read()` in `use_effect` to avoid update loops, contenteditable prefix-in-textContent bug.
- **Tracking**: no external tracker. `plans/<topic>.md` for big follow-ups, `// FUTURE:` comments in code for narrow ones, commit messages as the activity log.
- **Plans**: open architectural follow-ups in `plans/*.md`. Current top of stack: `plans/loro-text-editor-upgrade.md`.
- **Verify before done**: `cargo check -p task-ui` + `cargo check -p task-app-web --target wasm32-unknown-unknown` clean.
- **Research checkouts** (read-only references): `~/Development/research/{logseq,obsidian-api,obsidian-developer-docs,obsidian-sample-plugin}`.

Everything else — workflow, hard rules, gotchas, where things live — is in [AGENTS.md](AGENTS.md).
