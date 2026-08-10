//! Meters on the strip.
//!
//! The plumbing tests live beside the code; these are the two things only a
//! mounted strip can show: that a meter draws its track's level with a hold
//! mark, and that the ordering the frame is indexed by is the ordering the
//! track store actually maintains.

use daw_proto::peak::TrackLevels;
use daw_proto::{Track, TrackEvent};
use daw_ui::controls::{Meters, TrackMeter, TrackStore};
use dioxus::prelude::*;

fn track(guid: &str, index: u32) -> Track {
    Track {
        guid: guid.to_string(),
        index,
        name: guid.into(),
        ..Default::default()
    }
}

thread_local! {
    static STORE: std::cell::Cell<Option<TrackStore>> = const { std::cell::Cell::new(None) };
}

fn store() -> TrackStore {
    STORE.with(|s| s.get()).expect("store handle")
}

/// The order a meter frame is indexed by. Getting this wrong is silent —
/// every meter still moves, each showing the wrong track — which is why the
/// store owns it rather than each strip guessing.
#[test]
fn the_track_order_follows_adds_removes_and_moves() {
    fn app() -> Element {
        let mut s = use_hook(TrackStore::new);
        use_hook(|| {
            s.seed([track("A", 0), track("B", 1), track("C", 2)]);
            provide_context(s);
            STORE.with(|c| c.set(Some(s)));
        });
        rsx! { div {} }
    }
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();

    dom.in_runtime(|| {
        let mut store = store();
        assert_eq!(store.order(), ["A", "B", "C"]);

        // A track arrives in the middle.
        store.apply(&TrackEvent::Added(track("D", 1)));
        // D and B now both claim index 1 until the backend renumbers, but
        // the list has four entries either way — the meter frame's own
        // length is what decides whether it can be trusted.
        assert_eq!(store.order().len(), 4);

        store.apply(&TrackEvent::Removed("D".into()));
        assert_eq!(store.order(), ["A", "B", "C"]);

        // C moves to the top.
        store.apply(&TrackEvent::Moved {
            guid: "C".into(),
            old_index: 2,
            new_index: 0,
        });
        assert_eq!(store.order()[0], "C", "a move did not re-aim the meters");
    });
}

/// The meter draws two channels and their holds, as vectors.
#[test]
fn a_strip_meter_draws_both_channels_and_their_holds() {
    fn app() -> Element {
        let mut meters = use_hook(Meters::new);
        use_hook(|| {
            meters.apply(
                &["T1".to_string()],
                &[TrackLevels {
                    peak_left: 0.5,
                    peak_right: 0.25,
                    hold_left: 0.9,
                    hold_right: 0.8,
                }],
            );
            provide_context(meters);
        });
        rsx! { TrackMeter { track: "T1", height: 100 } }
    }

    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);

    assert_eq!(html.matches("<svg").count(), 2, "not two channels:\n{html}");
    // Two bars, two holds, two backgrounds — the hold marks are the 1px
    // rects, and without them the meter is peak-only.
    assert!(html.contains("height=\"1\""), "no peak-hold mark:\n{html}");
    assert!(!html.contains("<img"), "the meter is blitting:\n{html}");
    assert!(!html.contains("currentColor"), "a colour is left to CSS:\n{html}");
}

/// A meter with nothing behind it draws silence rather than failing to
/// mount — a strip has to render before the first frame arrives.
#[test]
fn a_meter_with_no_frames_yet_draws_silence() {
    let mut dom = VirtualDom::new(|| rsx! { TrackMeter { track: "T1" } });
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert_eq!(html.matches("<svg").count(), 2);
}
