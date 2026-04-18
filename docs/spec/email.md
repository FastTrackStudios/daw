# Email Spec

## Framework

### t[email.purpose]
Email is a first-class citizen in the task system. Every message on a
Proton Mail account can be linked to a task or project, tagged with
Proton labels that propagate to every IMAP client, and sorted by an
agent user (`curator`) that reads the inbox and files messages into
the correct project context. The task CLI is the agent's interface;
the authoritative link lives in the task DB, and tags/folders echo
that decision across Proton clients.

### t[email.architecture]
```
  Proton Mail (origin)
      ↓   (mailbox contents)
  ProtonMail Bridge  (systemd user service on starcommand:127.0.0.1)
    ├── IMAP  127.0.0.1:1143  (STARTTLS, self-signed cert)
    └── SMTP  127.0.0.1:1025  (STARTTLS)
      ↓   (IMAP sync + SMTP send)
  Nextcloud Mail app  (cloud.starcommand.live)
    │   account rows in oc_mail_accounts, mailboxes in oc_mail_mailboxes,
    │   tags in oc_mail_tags
      ↓   (HTTPS + Basic auth + app passwords)
  task-core `MailClient`  →  `task email …` CLI  →  agents (curator, humans)
```

Bridge lives on `starcommand` as a lingering user service under the
`starcommand` user. Login is interactive-once (`protonmail-bridge --cli
→ login`); afterwards the vault file stores credentials and the service
comes up on boot without intervention.

### t[email.accounts]
Three Nextcloud Mail account rows exist, provisioned declaratively by
`systemd.services.nextcloud-mail-accounts` on starcommand:

| NC user      | Email                    | Purpose                                   |
|--------------|--------------------------|-------------------------------------------|
| `codywright` | cody@fasttrackaudio.com  | Cody's personal access                    |
| `curator`    | cody@fasttrackaudio.com  | Curator's triage view of cody's inbox     |
| `curator`    | agent@fasttrackaudio.com | Shared agent inbox owned by curator       |

All three point at the same Bridge (`127.0.0.1:1143` STARTTLS) using
`cody/proton/bridge_password` from SOPS. Bridge accepts all of Cody's
Proton addresses under a single credential; the account row's
`imap-user` field selects which address it serves.

The dedicated NC user `agent` has no mail account — it's the sender
identity only, never a reader. All reading is curator's job.

### t[email.tags-and-folders]
Four organizational layers exist, each with a different reach:

| Layer              | Stored where                  | Visible in                                  | Managed via                                                  |
|--------------------|-------------------------------|---------------------------------------------|--------------------------------------------------------------|
| IMAP folders       | Proton (native)               | NC Mail, Proton web/mobile, any IMAP client | `task email folder-create --name "Folders/<n>"`              |
| Proton labels      | Proton (native)               | Same                                        | `task email folder-create --name "Labels/<n>"` (same API)    |
| NC Mail tags       | NC DB (`oc_mail_tags`)        | NC Mail + our CLI                           | `task email tag create / set / unset`                        |
| Task DB links      | task markdown + SQLite index  | `task email list`, agent queries            | `task email link --to task --message-id …`                   |

Bridge exposes Proton labels as pseudo-folders under `Labels/<name>`.
Creating a mailbox under `Labels/` on Bridge translates to a native
Proton label; assigning it (IMAP COPY, not implemented in the CLI yet)
would add the label without removing from source. The CLI's
`task email move` is a true IMAP MOVE — fine for filing into folders,
not for "keep in INBOX + label" semantics.

### t[email.authoritative-link]
The canonical link between an email and a task/project lives in the
task repo, never in Proton or NC Mail. `task email link` writes a
`EmailRef` entry on the task's or project's markdown frontmatter; the
RFC-2822 Message-Id is the stable key. Tags and labels on the mail
server are for discoverability in other clients; they are not
authoritative and MUST NOT be relied on by downstream consumers of the
task DB.

### t[email.curator-routing]
The curator user is the agent that files incoming mail. Contract:

1. Human or bot forwards an email to `agent@fasttrackaudio.com` (or
   curator notices a message in cody@ that needs triage).
2. Curator runs `task email search --mailbox <inbox-id>` against
   Nextcloud Mail using its own app password.
3. For each message, curator decides:
   - Which project or task it belongs to (via subject heuristics,
     sender match, thread reference, or direct prompt).
   - Whether to apply a Proton label so the filing is visible in
     Proton clients.
4. Curator executes:
   ```
   task email link --to task --reference <task-id> \
     --message-id <rfc-2822-id> --from … --subject … --date …
   task email folder-create --account <curator-acct-id> \
     --name "Labels/project.<slug>"   # once per project, idempotent
   task email tag set '$project.<slug>' --email-id <db-id>
   ```
5. On subsequent reads, `task email list --to task <task-id>` returns
   the `EmailRef` entries. Agents can refetch bodies live via
   `task email show <db-id> --body`.

Curator runs as `TASK_USER=curator` so every link/tag operation is
audited to the curator actor in the changes table.

### t[email.cli-reference]
Read:
```
task email accounts
task email mailboxes --account <id>
task email search --mailbox <id> [--filter "from:… subject:…"] [--limit N]
task email show <db-id> [--body]
```

Organization:
```
task email folder-create --account <id> --name "Folders/<path>"
task email folder-create --account <id> --name "Labels/<name>"   # Proton label
task email folder-delete --mailbox <id>
task email move --email-id <id> --to-folder <id>
task email tag list
task email tag create --name "<displayName>" --color "#RRGGBB"
task email tag set '<imapLabel>' --email-id <id>
task email tag unset '<imapLabel>' --email-id <id>
task email tag delete --account <id> --tag <id>
```

Triage loop (used by the curator skill):
```
task email sweep --account <id> [--mailbox <id>] [--limit N] [--filter …]
task email mark-processed --email-id <id> [--note "<reason>"]
```

`sweep` returns messages that are neither linked to a task/project
nor tagged `$processed`. `mark-processed` applies the `$processed`
NC Mail tag (auto-creating it on first call). See
[`skills/email-triage.md`](../../skills/email-triage.md) for the full
curator contract.

Push (IMAP IDLE):
```
IMAP_PASSWORD=<bridge_password> task email watch \
  --host 127.0.0.1 --port 1143 \
  --user <address> --mailbox INBOX \
  --ca-bundle /var/lib/nc-mail-trust/ca-bundle.crt
```

Long-running. Connects to Bridge via STARTTLS, LOGIN, SELECT, IDLE.
Emits one JSON line per server-pushed update:

```json
{"ts":"2026-04-18T08:36:00Z","mailbox":"INBOX","exists":8,"raw":"* 8 EXISTS"}
```

The `exists` field is the new total mailbox count. Downstream
consumers react by invoking the curator skill (typically via
`task email sweep`) on each line.

Linking:
```
task email link --to task|project --reference <ref> \
  --message-id <rfc-2822-id> [--subject …] [--from …] [--date …] \
  [--account-id N] [--mailbox <name>] [--imap-uid N] [--nc-db-id N] \
  [--attachments N] [--tags "a,b,c"]
task email unlink --to task|project <ref> --message-id <rfc-2822-id>
task email list --to task|project <ref>
```

All commands respect `TASK_USER` / `--as` for attribution, and
`NEXTCLOUD_URL` / `NEXTCLOUD_USER` / `NEXTCLOUD_PASSWORD` for NC auth.

### t[email.known-limitations]
- NC Mail's router splits URL paths on `/`, so tags whose `imapLabel`
  contains a slash (e.g. `$project/acme`) cannot be set or unset via
  the API. Use dot-separated names (`project.acme`) instead.
- `task email move` is a true MOVE, not a COPY. For Proton-style
  "tagged but still in INBOX" semantics, use `tag set` against an NC
  Mail tag. Cross-client label propagation requires direct IMAP COPY,
  which is not yet exposed in the CLI.
- Bridge presents a per-install self-signed cert. NC Mail's peer
  verification is disabled (`app.mail.verify-tls-peer = false`);
  this is safe because Bridge is loopback-only but MUST be
  reconsidered if Bridge is ever exposed beyond 127.0.0.1.
- NC Mail has no `GET /api/tags` endpoint; tag listing works by
  scraping the base64-JSON `initial-state-mail-tags` input from the
  Mail app's page HTML. Any major version bump of NC Mail should be
  re-tested.

### t[email.deploy-requirements]
- `cody/proton/bridge_password` must exist in
  `users/cody/secrets.yaml`; its value is the IMAP/SMTP password
  printed by `protonmail-bridge --cli → info cody@fasttrackaudio.com`.
- On first deploy, the ProtonMail Bridge must be logged in
  interactively on starcommand exactly once:
  ```
  ssh root@starcommand
  sudo -iu starcommand
  systemctl --user stop protonmail-bridge
  protonmail-bridge --cli
    login          # Proton email + password + 2FA
    exit
  systemctl --user start protonmail-bridge
  ```
  Subsequent reboots do not need this step.
- `services.nextcloud.extraApps` must include `mail`.
- `services.nextcloud.settings."app.mail.verify-tls-peer" = false`.
- `systemd.services.nextcloud-mail-appconfig` runs
  `config:app:set mail allow_local_remote_servers --value yes`.
- `systemd.services.protonmail-bridge-cert` extracts Bridge's cert
  into `/var/lib/nc-mail-trust/ca-bundle.crt`;
  `services.phpfpm.pools.nextcloud.phpOptions` sets
  `openssl.cafile` to that bundle.
- `systemd.services.nextcloud-mail-accounts` seeds and migrates
  the NC Mail account rows (see `t[email.accounts]`).
