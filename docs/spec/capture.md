# Capture Spec

## Quick Add

### t[capture.quick-add]
A persistent input bar is anchored to the bottom of every primary view. The field is always visible and focusable without navigating away from the current context. Pressing Enter (or tapping the submit button on iOS) submits the raw text to the NLP parser (see `t[capture.nlp]`) and creates a task with the parsed fields. The input bar clears immediately after submission to allow rapid sequential capture.

---

## NLP Parsing

### t[capture.nlp]
The NLP parser extracts structured fields from a raw text string using the following rules, applied in order:

- Date tokens: `today` → `due = today`; `tomorrow` → `due = tomorrow`; `next monday` (or any weekday) → `due = next occurrence of that weekday`; `in N days` → `due = today + N days`.
- Priority tokens: `!low` → `priority = low`; `!high` → `priority = high`; `!urgent` → `priority = urgent`.
- Tag tokens: `#word` → appended to `tags`.
- Project tokens: `[[Note Name]]` → appended to `projects`.
- Context tokens: `@word` → appended to `contexts`.
- All remaining text after token extraction becomes the `title`.

Tokens are removed from the string before the title is computed, so the title contains only human-readable prose. Parsing is case-insensitive for date and priority tokens.

### t[capture.nlp.fallback]
If no date token is found in the input, `due` is left empty and `scheduled` is left empty. A task with no `due`, no `scheduled`, and no `projects` field will appear in the Inbox view (see `t[views.inbox]`), making the Inbox the natural destination for unclassified quick-captures.

---

## Full Sheet

### t[capture.full-sheet]
The full task creation sheet is a modal that exposes all task fields: title, status, priority, due date, scheduled date, start date, projects, areas, contexts, tags, time estimate, recurrence, reminders, and body. It is accessible from the `+` button in the navigation bar and from the quick-add bar's expand affordance. Submitting the sheet creates the task with the same defaults as quick-add (see `t[capture.defaults]`) plus any explicitly set fields.

---

## Voice Capture

### t[capture.voice]
A microphone button in the quick-add bar triggers the platform's speech-to-text transcription. The transcribed string is passed directly to the NLP parser (see `t[capture.nlp]`) as if the user had typed it. On iOS, AVSpeechRecognizer is used. The microphone button is only shown when microphone permission has been granted; otherwise it is hidden without a prompt.

---

## Lock Screen Queue

### t[capture.lock-screen-queue]
The iOS lock screen widget provides a capture button that accepts text input without unlocking the device. Input captured from the lock screen is written to the App Group shared container as a pending JSON entry (same format as `t[sync.offline-queue]`). On next app open, pending lock-screen captures are read from the container, passed through the NLP parser, created as tasks, and cleared from the queue. Until processed, they do not appear in any view. After processing they land in the Inbox if no project or date was captured.

---

## Capture Defaults

### t[capture.defaults]
All tasks created through any capture path receive the following default values unless explicitly overridden:

- `status`: `open`
- `priority`: `none`
- `dateCreated`: current UTC datetime
- `dateModified`: current UTC datetime
- `id`: UUIDv4, auto-generated

No other fields receive implicit defaults; omitted fields are absent from frontmatter rather than set to empty strings or zero values.
