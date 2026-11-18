use std::time::{Duration as Wait, Instant};

use daw_proto::primitives::{Duration, PositionInSeconds};
use daw_proto::service::Transport as _;
use daw_proto::{ItemSpan, Items, ProjectContext, ProjectInfo, TrackRef, Tracks};

use crate::sync::Standalone;

fn info(guid: &str) -> ProjectInfo {
    ProjectInfo {
        guid: guid.into(),
        name: guid.into(),
        path: String::new(),
    }
}

fn song(guid: &str) -> ProjectContext {
    ProjectContext::Project(guid.into())
}

/// `tracks` tracks of `items` items each, built in `daw`'s project `guid`.
fn build(daw: &Standalone, guid: &str, tracks: usize, items: usize) {
    for t in 0..tracks {
        let track = format!("track-{t}");
        daw.add_with_guid(song(guid), &track, &track, None)
            .expect("a track");
        for i in 0..items {
            let at = i as f64;
            daw.add_item_with_guid(
                song(guid),
                TrackRef::Guid(track.clone()),
                &format!("item-{t}-{i}"),
                ItemSpan::new(
                    PositionInSeconds::from_seconds(at),
                    Duration::from_seconds(0.5),
                ),
            )
            .expect("an item");
        }
    }
}

#[test]
fn a_landed_project_is_the_one_built_and_leaves_the_stage() {
    let home = Standalone::new();
    let stage = home.stage();
    stage.engine().seed_project(info("song"));
    build(stage.engine(), "song", 3, 4);
    let built_tracks = stage.engine().all(song("song"));
    let built_items = stage.engine().get_all_items(song("song"));
    let built_revision = stage
        .engine()
        .read_project("song", |p| p.revision)
        .expect("built");
    let engine = stage.engine().clone();

    stage.land("song").expect("landed");

    assert_eq!(home.all(song("song")), built_tracks);
    assert_eq!(home.get_all_items(song("song")), built_items);
    assert_eq!(
        home.read_project("song", |p| p.revision),
        Some(built_revision.wrapping_add(1)),
        "moved on once"
    );
    assert_eq!(
        engine.read_project("song", |_| ()),
        None,
        "gone from the stage"
    );
    assert_eq!(
        home.current_project_guid().as_deref(),
        Some("song"),
        "the first project an engine has is its current one"
    );
}

#[test]
fn a_project_already_home_is_not_landed_over() {
    let home = Standalone::new();
    home.seed_project(info("song"));
    let stage = home.stage();
    stage.engine().seed_project(info("song"));
    build(stage.engine(), "song", 1, 1);

    assert!(stage.land("song").is_err());
    assert!(home.all(song("song")).is_empty(), "the one home kept");
}

#[test]
fn the_transport_settings_made_on_the_stage_land_with_it() {
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let _inside = runtime.enter();
    let home = Standalone::new();
    let stage = home.stage();
    stage.engine().seed_project(info("song"));
    stage
        .engine()
        .set_loop(song("song"), true)
        .expect("looping");
    stage
        .engine()
        .set_playrate(song("song"), 0.5)
        .expect("half speed");
    stage
        .engine()
        .set_metronome(song("song"), true)
        .expect("click");

    stage.land("song").expect("landed");

    assert!(home.is_looping(song("song")));
    assert_eq!(home.get_playrate(song("song")), 0.5);
    assert!(
        home.transport_engine_for("song").shared.metronome(),
        "the click"
    );
}

/// A song built on a stage never holds the engine's lock while it builds:
/// a reader of another song waits no longer than a frame's slice for it.
#[test]
fn building_a_song_on_a_stage_does_not_stall_another_songs_reader() {
    let home = Standalone::new();
    home.seed_project(info("playing"));
    build(&home, "playing", 8, 8);
    let done = std::sync::atomic::AtomicBool::new(false);

    let worst = std::thread::scope(|scope| {
        scope.spawn(|| {
            let stage = home.stage();
            stage.engine().seed_project(info("opening"));
            build(stage.engine(), "opening", 160, 60);
            stage.land("opening").expect("landed");
            done.store(true, std::sync::atomic::Ordering::Release);
        });
        let mut worst = Wait::ZERO;
        while !done.load(std::sync::atomic::Ordering::Acquire) {
            let asked = Instant::now();
            let tracks = home.read_project("playing", |p| p.tracks.len());
            worst = worst.max(asked.elapsed());
            assert_eq!(tracks, Some(8));
        }
        worst
    });

    assert_eq!(home.all(song("opening")).len(), 160);
    assert!(
        worst < Wait::from_millis(20),
        "a read of another song waited {worst:?}"
    );
}
