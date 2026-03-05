# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

## btca — Source Code Search

Use **btca** to query the actual source code of key dependencies before implementing features or debugging. Prefer this over web searches or docs that may be outdated.

```bash
btca ask -r <resource> -q "your question"
btca ask -r facet -r roam -q "How does roam generate TypeScript clients from Rust traits?"
btca resources   # list all available resources
```

### Relevant Resources for This Repo

| Resource | Repo | Description |
|----------|------|-------------|
| `facet` | facet-rs/facet | Rust reflection — shapes, derive macros, serialization, pretty-printing |
| `roam` | bearcove/roam | Rust-native RPC framework where Rust traits are the schema, with TS/Swift codegen |
| `tracey` | bearcove/tracey | Traceability tool linking requirements/specs to code implementations via annotations |
| `figue` | bearcove/figue | Config parsing from CLI args, env vars, and config files using facet reflection |
| `styx` | bearcove/styx | Cleaner serialization format — alternative to JSON/YAML with schema support |
| `reaper-rs` | helgoboss/reaper-rs | Low/medium/high-level Rust bindings for the REAPER DAW API (reaper-low, reaper-medium, reaper-high) |
| `rea-rs` | Levitanus/rea-rs | Higher-level idiomatic Rust wrapper around the REAPER C++ API |
| `helgobox` | helgoboss/helgobox | Full REAPER extension (ReaLearn/Playtime) — reference for building complex REAPER plugins in Rust |
| `sws` | reaper-oss/sws | SWS/S&M Extension — large open-source REAPER extension with actions, snapshots, and API extensions |

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
