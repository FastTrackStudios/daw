//! The track fields a second client is told about, inside a real REAPER.
//!
//! These were all readable on demand and none of them were announced,
//! so a window that had drawn one kept drawing it after it changed —
//! right on the first frame and a guess on every frame after, with
//! nothing on screen to say which. Found by pointing the session window
//! at a live REAPER (session#98).
//!
//! Pinned here as well as against standalone deliberately. The
//! standalone test proves the events exist and carry the right thing;
//! only this one proves REAPER's poller actually notices, which is the
//! half that was missing — the variants existed and the diff did not.
//!
//! Run with: `cargo run -p daw-reaper-xtask -- reaper_track_mirror`

use daw::test::reaper_test;
use daw_proto::track::{GroupFamily, GroupRole, TrackEvent};

/// How long to wait for the 30 Hz poller to notice.
///
/// Generous on purpose: the thing being measured is whether the diff
/// exists at all, and a tight bound would turn a busy REAPER into a
/// failure that reads like a missing event.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);

/// Wait for an event this test is interested in, ignoring the rest.
///
/// Attaching to a REAPER means the stream carries every track in the
/// tab, including whatever the harness did to set the test up, so a
/// test that took the FIRST event would be testing the setup.
async fn wait_for(
    stream: &mut daw::rpc::EventStream<daw_proto::track::TrackStreamEvent>,
    mut wanted: impl FnMut(&TrackEvent) -> bool,
) -> eyre::Result<TrackEvent> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            eyre::bail!("no matching track event within {PATIENCE:?}");
        }
        match tokio::time::timeout(left, stream.recv()).await {
            Err(_) => eyre::bail!("no matching track event within {PATIENCE:?}"),
            Ok(Ok(Some(event))) => {
                let event = event.get().event.clone();
                if wanted(&event) {
                    return Ok(event);
                }
            }
            Ok(Ok(None)) => eyre::bail!("the track stream ended"),
            Ok(Err(error)) => eyre::bail!("the track stream failed: {error}"),
        }
    }
}

/// Flip the six fields the poller read and never reported.
///
/// One test rather than six, because the cost here is the REAPER tab
/// and the subscription, not the assertions — and because a run that
/// reports five of six passing hides the shape of the bug, which was
/// that the whole group of them was missing at once.
#[reaper_test(isolated)]
async fn the_fields_the_poller_used_to_drop_reach_a_subscriber(
    ctx: &daw::test::ReaperTestContext,
) -> eyre::Result<()> {
    use daw_proto::primitives::AutomationMode;
    use daw_proto::track::InputMonitoringMode;

    let project = ctx.project().clone();
    let tracks = project.tracks();
    let kick = tracks.add("Mirror Kick", None).await?;
    let mut stream = tracks.subscribe().await?;

    kick.set_phase_inverted(true).await?;
    wait_for(&mut stream, |event| {
        matches!(
            event,
            TrackEvent::PhaseInvertedChanged { inverted: true, .. }
        )
    })
    .await?;

    kick.set_input_monitor(InputMonitoringMode::NotWhenPlaying)
        .await?;
    wait_for(&mut stream, |event| {
        matches!(
            event,
            TrackEvent::InputMonitorChanged {
                monitor: InputMonitoringMode::NotWhenPlaying,
                ..
            }
        )
    })
    .await?;

    kick.set_automation_mode(AutomationMode::Latch).await?;
    wait_for(&mut stream, |event| {
        matches!(
            event,
            TrackEvent::AutomationModeChanged {
                mode: AutomationMode::Latch,
                ..
            }
        )
    })
    .await?;

    // The one an engineer changes while tracking, which is the worst
    // moment for a second screen to be showing the old answer.
    kick.set_record_input(daw_proto::track::RecordInput::Audio { channel: 3 })
        .await?;
    wait_for(&mut stream, |event| {
        matches!(
            event,
            TrackEvent::RecordInputChanged {
                input: daw_proto::track::RecordInput::Audio { channel: 3 },
                ..
            }
        )
    })
    .await?;

    kick.set_visibility(false, true).await?;
    wait_for(&mut stream, |event| {
        matches!(
            event,
            TrackEvent::TcpVisibilityChanged { visible: false, .. }
        )
    })
    .await?;

    kick.set_visibility(false, false).await?;
    wait_for(&mut stream, |event| {
        matches!(
            event,
            TrackEvent::MixerVisibilityChanged { visible: false, .. }
        )
    })
    .await?;

    Ok(())
}

/// A group written through the facade is announced.
///
/// The FTS grouping watcher is what manages groups, so this is the path
/// every group FTS makes travels. A window attached beside the watcher
/// used to watch a VCA appear in REAPER and not in itself.
#[reaper_test(isolated)]
async fn a_group_written_through_the_facade_is_announced(
    ctx: &daw::test::ReaperTestContext,
) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let tracks = project.tracks();
    let kick = tracks.add("Grouped Kick", None).await?;
    let mut stream = tracks.subscribe().await?;

    kick.set_group_flags(128, GroupFamily::Vca, GroupRole::Lead)
        .await?;
    let event = wait_for(&mut stream, |event| {
        matches!(event, TrackEvent::GroupingChanged { .. })
    })
    .await?;

    // The event carries the state AFTER the change, so a client applies
    // it without asking again. Compared against a fresh read, because an
    // event that merely says "something changed" would pass a weaker
    // assertion and still leave every client one round trip behind.
    let TrackEvent::GroupingChanged { grouping, .. } = &event else {
        eyre::bail!("not a grouping event");
    };
    assert_eq!(
        grouping,
        &kick.group_flags().await?,
        "the event disagreed with a fresh read"
    );
    assert_eq!(
        grouping.role(GroupFamily::Vca, 128),
        GroupRole::Lead,
        "the VCA lead the watcher just wrote is not in the event"
    );

    Ok(())
}

/// A subscriber that arrives late is told what the groups already are.
///
/// This is the sweep, and it is the only thing that can see a group
/// this process did not write — the writers announce what they change
/// and nothing else, and the bulk track read leaves grouping empty
/// because reading it is about a hundred calls a track. So a window
/// attaching to a REAPER mid-session had no way to learn the grouping
/// at all, and a group edited by hand in REAPER's own matrix dialog
/// goes through no writer and would never be seen again.
///
/// Tested through a fresh subscription rather than through the dialog,
/// which cannot be driven: both reach the window by the same route, and
/// the route is the thing that was missing.
#[reaper_test(isolated)]
async fn a_late_subscriber_is_told_what_the_groups_already_are(
    ctx: &daw::test::ReaperTestContext,
) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let tracks = project.tracks();
    let snare = tracks.add("Swept Snare", None).await?;

    // Written and then forgotten about, exactly as a group that was
    // already in the project when the window opened.
    snare
        .set_group_flags(7, GroupFamily::Volume, GroupRole::Follow)
        .await?;

    let mut stream = tracks.subscribe().await?;
    let event = wait_for(&mut stream, |event| {
        matches!(event, TrackEvent::GroupingChanged { grouping, .. }
            if grouping.role(GroupFamily::Volume, 7) == GroupRole::Follow)
    })
    .await?;
    let TrackEvent::GroupingChanged { guid, .. } = &event else {
        eyre::bail!("not a grouping event");
    };
    assert_eq!(guid, snare.guid(), "the sweep named the wrong track");

    Ok(())
}
