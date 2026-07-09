//! Collaborative notes — the real-time sync showcase.
//!
//! Everything on this page runs against the **local replica** provided
//! by `crdt::use_synced_doc` at the app root:
//!
//! - reads come from the derive-emitted `use_note_crdt_list()` and
//!   re-render on every doc revision — your edits and every peer's,
//!   through one path;
//! - writes go through `use_note_crdt_actions()` straight into the
//!   replica: instant, no spinner, no rollback arm — concurrent edits
//!   *merge* instead of conflicting;
//! - the connection is only a sync pipe. Kill the server and the page
//!   keeps working; edits buffer and merge on reconnect (the status
//!   badge shows which mode you're in);
//! - the presence strip is Loro's `EphemeralStore` over the same
//!   socket — who's here right now, expiring on its own.
//!
//! Open the page in two windows (or web + desktop) and type.

use architect::AtomResult::{Defect, Error, Initial, Loading, Reloading, Success};
use architect::form::TextField;
use crdt::SyncStatus;
use dioxus::prelude::*;
use example::{Note, NoteUpdate, use_note_create_fields};
use example::{use_note_crdt_actions, use_note_crdt_list};
use uuid::Uuid;

use crate::status::{DefectPanel, Spinner, StatusError};

#[component]
pub fn Collab() -> Element {
    let actions = use_note_crdt_actions();
    let notes = use_note_crdt_list();
    let handle = crdt::use_doc_handle();
    let presence = crdt::use_presence();
    let fields = use_note_create_fields();

    // Announce ourselves on the presence channel while this page is
    // mounted: key = this client's id, value = the author name being
    // typed (live — peers see who's composing). Expires server-side
    // ~30s after we go quiet.
    let client_id = use_hook(|| Uuid::new_v4().to_string());
    let author_field = fields.author;
    use_effect({
        let client_id = client_id.clone();
        move || {
            let name = author_field.value();
            let name = if name.trim().is_empty() {
                "anonymous".to_string()
            } else {
                name
            };
            presence.set(&client_id, name);
        }
    });

    rsx! {
        section { class: "composer",
            div { class: "collab-header",
                h2 { class: "composer__title", "Collaborative notes" }
                SyncBadge { status: handle.status() }
            }
            p { class: "status",
                "Every window holds the full doc — edits are instant, offline works, peers merge in live."
            }
            PresenceStrip { presence }
            form {
                class: "example-form",
                onsubmit: move |evt| {
                    evt.prevent_default();
                    if let Some(input) = fields.submit() {
                        // Straight into the local replica — no round-trip,
                        // no optimistic bookkeeping to reconcile.
                        actions.create(input);
                        // Keep the author for the next note; clear the text.
                        let author = fields.author.value();
                        fields.reset();
                        fields.author.set(author);
                    }
                },
                TextField { field: fields.text, label: "Note", placeholder: "Type here — peers see it as you save" }
                TextField { field: fields.author, label: "Author", placeholder: "Your name" }
                button { class: "btn primary", r#type: "submit", "Add note" }
            }
        }

        match notes {
            Initial | Loading => rsx! { Spinner {} },
            Success(rows) | Reloading(rows) => rsx! { NoteList { rows } },
            Error { error, last } => rsx! {
                StatusError { message: "{error}" }
                if let Some(rows) = last {
                    NoteList { rows }
                }
            },
            Defect { defect, .. } => rsx! { DefectPanel { defect } },
        }
    }
}

/// Where the sync session stands. The replica is fully usable in every
/// state — this is a pipe indicator, not a gate.
#[component]
fn SyncBadge(status: SyncStatus) -> Element {
    let (class, label) = match status {
        SyncStatus::Connecting => ("badge badge--connecting", "connecting…"),
        SyncStatus::Live => ("badge badge--live", "live"),
        SyncStatus::Offline => ("badge badge--offline", "offline — edits buffer locally"),
    };
    rsx! { span { class: "{class}", "{label}" } }
}

/// Who's on this doc right now (Loro `EphemeralStore` over the shared
/// socket; entries expire on their own when a peer goes quiet).
#[component]
fn PresenceStrip(presence: crdt::Presence) -> Element {
    let mut names: Vec<String> = presence
        .states()
        .into_values()
        .map(|v| match v {
            crdt::loro::LoroValue::String(s) => s.to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    names.sort();
    let count = names.len();
    rsx! {
        div { class: "presence-strip",
            span { class: "presence-strip__count",
                if count == 1 { "1 person here" } else { "{count} people here" }
            }
            span { class: "presence-strip__names", "{names.join(\", \")}" }
        }
    }
}

#[component]
fn NoteList(rows: Vec<Note>) -> Element {
    let mut rows = rows;
    // Newest first. The replica returns rows unordered; pages own
    // presentation order.
    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    if rows.is_empty() {
        return rsx! { p { class: "status empty", "No notes yet — add one (or open this page in another window and add it there)." } };
    }
    rsx! {
        ul { class: "example-list",
            for note in rows {
                NoteRow { key: "{note.id}", note }
            }
        }
    }
}

/// One note: text + author, inline edit, delete. Edits write the local
/// replica and merge with whatever peers are doing to other fields.
#[component]
fn NoteRow(note: Note) -> Element {
    let actions = use_note_crdt_actions();
    let mut editing = use_signal(|| false);
    let mut draft = use_signal(String::new);
    let id = note.id;

    rsx! {
        li { class: "example-link-row",
            div { class: "row",
                div { class: "row__content",
                    if editing() {
                        form {
                            class: "note-edit",
                            onsubmit: move |evt| {
                                evt.prevent_default();
                                let text = draft();
                                if !text.trim().is_empty() {
                                    actions.update(id, NoteUpdate { text: Some(text), author: None });
                                }
                                editing.set(false);
                            },
                            input {
                                class: "field__input",
                                value: "{draft}",
                                autofocus: true,
                                oninput: move |evt| draft.set(evt.value()),
                            }
                            button { class: "btn primary", r#type: "submit", "Save" }
                            button {
                                class: "btn",
                                r#type: "button",
                                onclick: move |_| editing.set(false),
                                "Cancel"
                            }
                        }
                    } else {
                        p { class: "note-text", "{note.text}" }
                        {
                            let when = note.updated_at.format("%H:%M:%S").to_string();
                            rsx! { p { class: "note-meta", "{note.author} · {when}" } }
                        }
                    }
                }
                if !editing() {
                    div { class: "row__actions",
                        button {
                            class: "btn",
                            onclick: {
                                let text = note.text.clone();
                                move |_| {
                                    draft.set(text.clone());
                                    editing.set(true);
                                }
                            },
                            "Edit"
                        }
                        button {
                            class: "btn danger",
                            onclick: move |_| actions.delete(id),
                            "Delete"
                        }
                    }
                }
            }
        }
    }
}
