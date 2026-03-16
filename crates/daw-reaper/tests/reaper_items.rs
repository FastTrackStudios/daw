//! REAPER integration test: item and MIDI creation.
//!
//! Tests the full pipeline: create track → add item → add take → add MIDI notes.
//! Debugs each step to identify where item creation fails.
//!
//! Run with: `cargo test -p daw-reaper --test reaper_items -- --ignored --nocapture`

use reaper_test::reaper_test;
use std::time::Duration;

#[reaper_test(isolated)]
async fn item_create_basic(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let tracks = project.tracks();

    // Step 1: Create a track
    let track = tracks.add("Item Test", None).await?;
    ctx.log(&format!("Track created: guid={}", track.guid()));

    // Step 2: Check initial item count
    let count_before = track.items().count().await?;
    ctx.log(&format!("Items before: {}", count_before));
    assert_eq!(count_before, 0, "Should start with 0 items");

    // Step 3: Try to add an item
    ctx.log("Attempting to add item at 0.0s, length 2.0s...");
    let item = match track.items().add(
        daw_proto::primitives::PositionInSeconds::from_seconds(0.0),
        daw_proto::primitives::Duration::from_seconds(2.0),
    ).await {
        Ok(item) => {
            ctx.log(&format!("Item created successfully: guid={}", item.guid()));
            item
        }
        Err(e) => {
            ctx.log(&format!("FAILED to create item: {:?}", e));

            // Debug: check item count (ignoring errors)
            match track.items().count().await {
                Ok(c) => ctx.log(&format!("Items after failed add: {}", c)),
                Err(e2) => ctx.log(&format!("count() also failed: {:?}", e2)),
            }

            return Err(eyre::eyre!("Failed to create item: {:?}", e));
        }
    };

    // Step 4: Verify item was created
    let count_after = track.items().count().await?;
    ctx.log(&format!("Items after: {}", count_after));
    assert_eq!(count_after, 1, "Should have 1 item");

    // Step 5: Check item properties
    let info = item.info().await?;
    ctx.log(&format!("Item position: {:.2}s, length: {:.2}s",
        info.position.as_seconds(), info.length.as_seconds()));

    // Step 6: Try to get the active take
    ctx.log("Getting active take...");
    let take = match item.takes().active().await {
        Ok(t) => {
            ctx.log(&format!("Active take found"));
            t
        }
        Err(e) => {
            ctx.log(&format!("No active take: {:?}", e));
            // Try adding a take
            ctx.log("Trying to add a take...");
            match item.takes().add().await {
                Ok(t) => {
                    ctx.log("Take added successfully");
                    t
                }
                Err(e2) => {
                    ctx.log(&format!("Failed to add take: {:?}", e2));
                    return Err(eyre::eyre!("No active take and can't add one: {:?}, {:?}", e, e2));
                }
            }
        }
    };

    // Step 7: Try adding a MIDI note
    ctx.log("Adding MIDI note (C4, vel=100)...");
    match take.midi().add_note(60, 100, 0.0, 480.0).await {
        Ok(idx) => ctx.log(&format!("Note added at index {}", idx)),
        Err(e) => {
            ctx.log(&format!("Failed to add note: {:?}", e));
            return Err(eyre::eyre!("Failed to add MIDI note: {:?}", e));
        }
    }

    // Step 8: Verify note exists
    let notes = take.midi().notes().await?;
    ctx.log(&format!("Notes in take: {}", notes.len()));
    assert!(notes.len() >= 1, "Should have at least 1 note");
    ctx.log(&format!("Note 0: pitch={}, vel={}, start={:.1}, len={:.1}",
        notes[0].pitch, notes[0].velocity, notes[0].start_ppq, notes[0].length_ppq));

    ctx.log("item_create_basic: PASSED");
    Ok(())
}

#[reaper_test(isolated)]
async fn item_count_on_empty_track(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let track = project.tracks().add("Empty Track", None).await?;

    let count = track.items().count().await?;
    ctx.log(&format!("Item count on empty track: {}", count));
    assert_eq!(count, 0);

    let all = track.items().all().await?;
    ctx.log(&format!("Items list: {:?}", all.len()));
    assert_eq!(all.len(), 0);

    ctx.log("item_count_on_empty_track: PASSED");
    Ok(())
}
