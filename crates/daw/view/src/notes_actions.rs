//! The Editor's write-back, as a named action
//! (`docs/design/actions-everywhere.md`): what the Editor (the piano roll,
//! drum lanes, curves) commits when its take changed — `NOTES_WRITE`, so
//! `watch` sees an edit of notes and the CLI or a model can make the same
//! one.

use architect::action::{ActionList, Effect};
use daw::service::midi::{MidiTakeContent, WriteMode};
use facet::Facet;

use crate::actions::meta;

pub static NOTES_WRITE: architect::action::ActionMeta = meta(
    "NOTES_WRITE",
    "notes_write",
    "Write a take's notes",
    "Replace the MIDI of item `item`'s active take (its GUID) with `content` — `notes` (each `start_ppq`, `length_ppq`, `pitch`, `velocity`, `channel`, …), `ccs`, `pitch_bends`, `note_expressions`, `channel_pressures` — positions in quarter notes from the item's start. One undo step. What the Editor commits when its take changed.",
    "Edit",
    "",
);

/// The undo step a write is, in the engine and so in the song's doc.
pub const EDIT_NOTES: &str = "Edit notes";

/// A take's new contents.
#[derive(Facet)]
pub struct NotesArg {
    pub item: String,
    pub content: MidiTakeContent,
}

/// Write `content` to item `item`'s active take, replacing it, as one undo
/// step: through the facade, on whichever engine this process drives.
///
/// # Errors
/// No engine, no song, no such item, or the write refused.
pub async fn write(item: &str, content: MidiTakeContent) -> Result<(), String> {
    let daw = daw::rpc::Daw::try_get().ok_or("no engine")?;
    let project = daw.current_project().await.map_err(|e| e.to_string())?;
    let found = project
        .items()
        .by_guid(&crate::engine::made(item))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no item {item}"))?;
    // One engine undo block, so one step in the song's doc (the block's
    // end records it: `project_file::record_steps`). The replace deletes
    // and adds every note; bare, it was no step at all, folded into
    // whatever was recorded next.
    let _ = project.begin_undo_block(EDIT_NOTES).await;
    let written = found
        .active_take()
        .midi()
        .write_take(content, WriteMode::Replace)
        .await
        .map_err(|e| e.to_string());
    let _ = project.end_undo_block(EDIT_NOTES).await;
    written.map(drop)
}

/// Put `NOTES_WRITE` on the list.
///
/// # Errors
/// When it is on it already.
pub fn register(list: &ActionList) -> Result<(), architect::action::ActionError> {
    list.register_async(&NOTES_WRITE, Effect::Mutating, |a: NotesArg| async move {
        write(&a.item, a.content).await?;
        crate::actions::list().changed("items", &a.item);
        Ok(())
    })
}
