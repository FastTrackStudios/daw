//! REAPER integration: the panel opens, and its edits reach a real
//! take.
//!
//! Deliberately narrow. Every piece of *logic* — the conversion, the
//! session round trip, the edit operations — is tested against the
//! standalone backend in `expression-editor-daw`, because that is code
//! in our control and a test that needs REAPER running to check
//! arithmetic is a slow test for no reason.
//!
//! What only exists inside REAPER, and so is only testable here:
//!
//! - the panel registering and docking through `reaper-dioxus`;
//! - `load_selected` finding REAPER's actual item selection;
//! - a write landing in a real take that REAPER then reports back.
//!
//! Run with:
//!   cargo xtask reaper-test reaper_panel

use std::time::Duration;

use daw::service::MidiNoteCreate;
use daw::test::{reaper_test, ReaperTestContext};

/// Let REAPER's main thread process what we just asked for.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// A track with one MIDI item holding a known set of notes.
async fn seed_item(ctx: &ReaperTestContext) -> eyre::Result<daw::rpc::ItemHandle> {
    let track = ctx.project().tracks().add("Expression Editor", None).await?;
    let item = track
        .items()
        .create_midi_item_with_notes(
            0.0,
            4.0,
            vec![
            MidiNoteCreate {
                channel: 0,
                pitch: 60,
                velocity: 100,
                start_ppq: 0.0,
                length_ppq: 960.0,
            },
            MidiNoteCreate {
                channel: 0,
                pitch: 64,
                velocity: 90,
                start_ppq: 960.0,
                length_ppq: 960.0,
            },
            MidiNoteCreate {
                channel: 0,
                pitch: 67,
                velocity: 80,
                start_ppq: 1920.0,
                length_ppq: 960.0,
            },
            ],
        )
        .await?
        .expect("REAPER must create the MIDI item");
    settle().await;
    Ok(item)
}

/// Run a named FTS action.
async fn action(ctx: &ReaperTestContext, id: &str) -> eyre::Result<()> {
    ctx.daw.action_registry().execute_action(id).await?;
    settle().await;
    Ok(())
}

#[reaper_test(isolated)]
async fn the_panel_registers_and_toggles(ctx: &ReaperTestContext) -> eyre::Result<()> {
    // Registration is the thing that silently does not happen when the
    // reaper-dioxus service has not come up.
    action(ctx, "FTS_EXPRESSION_EDITOR_TOGGLE").await?;
    ctx.assert_panel_visible(expression_editor_reaper::PANEL_ID)
        .await?;

    action(ctx, "FTS_EXPRESSION_EDITOR_TOGGLE").await?;
    ctx.assert_panel_hidden(expression_editor_reaper::PANEL_ID)
        .await?;
    Ok(())
}

#[reaper_test(isolated)]
async fn opening_on_a_selected_item_loads_its_notes(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    // The count comes from the panel's own session, so this checks the
    // whole path: REAPER's selection -> read_take -> to_doc.
    assert_eq!(
        expression_editor_reaper::loaded_note_count(),
        3,
        "the selected take's notes must reach the editor"
    );
    ctx.assert_panel_visible(expression_editor_reaper::PANEL_ID)
        .await?;
    Ok(())
}

#[reaper_test(isolated)]
async fn opening_with_nothing_selected_reports_rather_than_opening_empty(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    ctx.project().items().deselect_all().await?;
    settle().await;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    // An editor opened on nothing looks like a broken load, so the
    // action must decline rather than show an empty canvas.
    assert_eq!(expression_editor_reaper::loaded_note_count(), 0);
    Ok(())
}

#[reaper_test(isolated)]
async fn an_edit_written_back_lands_in_the_real_take(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    assert_eq!(expression_editor_reaper::loaded_note_count(), 3);

    // Transpose everything an octave through the editor's own edit
    // path, then write.
    expression_editor_reaper::with_editor(|ed| {
        let ids: Vec<_> = ed.doc.notes.iter().map(|n| n.id).collect();
        ed.apply(&expression_editor_core::Edit::Transpose {
            notes: ids,
            semitones: 12,
        });
    });
    assert!(
        expression_editor_reaper::is_dirty(),
        "the session must notice the edit"
    );

    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;

    // Ask REAPER, not the editor: this is the assertion that the
    // backend actually wrote something.
    let notes = item.active_take().midi().notes().await?;
    let mut pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    pitches.sort_unstable();
    assert_eq!(pitches, vec![72, 76, 79], "REAPER reports the transposed take");
    assert!(
        !expression_editor_reaper::is_dirty(),
        "writing clears the dirty flag"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn a_write_replaces_rather_than_appending(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;
    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    // Two writes with no edit between them must not double the take —
    // the failure mode a naive add-notes implementation has.
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;

    let notes = item.active_take().midi().notes().await?;
    assert_eq!(notes.len(), 3, "still three notes, not six");
    Ok(())
}

#[reaper_test(isolated)]
async fn reload_discards_editor_edits_in_favour_of_the_take(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;
    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    expression_editor_reaper::with_editor(|ed| {
        let ids: Vec<_> = ed.doc.notes.iter().map(|n| n.id).collect();
        ed.apply(&expression_editor_core::Edit::DeleteNotes(ids));
    });
    assert_eq!(expression_editor_reaper::loaded_note_count(), 0);

    action(ctx, "FTS_EXPRESSION_EDITOR_RELOAD").await?;

    assert_eq!(
        expression_editor_reaper::loaded_note_count(),
        3,
        "the take is authoritative on reload"
    );
    assert!(!expression_editor_reaper::is_dirty());
    Ok(())
}
