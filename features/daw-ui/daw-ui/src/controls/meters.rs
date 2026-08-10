//! Meters, from the stream that already existed.
//!
//! There is no meter design in here, and that is the point. The backend
//! already publishes one argless subscription carrying the **whole mixer**
//! at ~30Hz — four floats per track, linear 0..1 — with peak-hold computed
//! publisher-side so both backends produce identical ballistics, gated on
//! subscriber count so a closed mixer costs the engine nothing, and dropping
//! frames rather than stalling under backpressure. So this is plumbing:
//! subscribe once, index the frame onto per-track levels, render.
//!
//! **One subscription for the whole mixer.** A per-strip subscription would
//! be N times the messages for the same bytes, and it would throw away the
//! thing that makes the frame cheap.
//!
//! # The alignment, which fails silently
//!
//! The frame is indexed by *track position*, not by guid. Anything that
//! reorders tracks — an add, a remove, a move — re-aims every index at a
//! different track, and the failure does not look like a failure: the meters
//! still move, they are just each showing somebody else's level. The
//! ordering therefore lives in [`TrackStore`][crate::controls::TrackStore],
//! which is the thing that already hears about adds, removes and moves, and
//! a frame whose length disagrees with it is dropped rather than guessed at.

use std::collections::{HashMap, HashSet};

use daw_proto::peak::TrackLevels;
use daw_theme_art::vector_controls as art;

use crate::controls::use_track_store;
use crate::prelude::*;

/// Every track's current levels, by guid.
#[derive(Clone, Copy, PartialEq)]
pub struct Meters {
    levels: Signal<HashMap<String, TrackLevels>>,
}

impl Meters {
    /// Must be called inside a component scope — it owns a Signal.
    pub fn new() -> Self {
        Self { levels: Signal::new(HashMap::new()) }
    }

    /// This track's levels, or silence if no frame has mentioned it.
    pub fn levels(&self, guid: &str) -> TrackLevels {
        self.levels.read().get(guid).copied().unwrap_or_default()
    }

    /// Index one frame onto `order`.
    ///
    /// A frame that does not have exactly one entry per known track is
    /// **dropped**. That happens for a beat after a track is added or
    /// removed, while the frame and the track list disagree about how many
    /// tracks exist — and showing a stale meter for that beat is much
    /// better than showing every strip its neighbour's level, which is what
    /// indexing a mismatched frame would do.
    pub fn apply(&mut self, order: &[String], frame: &[TrackLevels]) {
        if order.len() != frame.len() {
            return;
        }
        let mut levels = self.levels.write();
        for (guid, l) in order.iter().zip(frame) {
            levels.insert(guid.clone(), *l);
        }
        // A set, not `order.contains`: this runs inside a Signal write at
        // thirty frames a second, and a linear scan per track is quadratic
        // in the size of the mixer.
        let live: HashSet<&str> = order.iter().map(String::as_str).collect();
        levels.retain(|g, _| live.contains(g.as_str()));
    }
}

impl Default for Meters {
    fn default() -> Self {
        Self::new()
    }
}

/// The meters from context, creating and providing one if this is the first
/// asker — same provide-or-consume shape as the track store, so a meter can
/// draw in a test with nothing behind it.
pub fn use_meters() -> Meters {
    use_hook(|| match try_consume_context::<Meters>() {
        Some(m) => m,
        None => provide_context(Meters::new()),
    })
}

/// Keep `meters` fed from the connected DAW.
///
/// Call **once**, high up, beside
/// [`use_daw_tracks`][crate::controls::use_daw_tracks]. One subscription
/// serves every strip; the engine's pump is gated on subscriber count, so
/// not calling this is what keeps a closed mixer free.
#[component]
pub fn MeterFeed() -> Element {
    let store = use_track_store();
    let mut meters = use_meters();

    use_future(move || async move {
        let Some(project) = crate::controls::reach::connected_project().await else {
            return;
        };
        let mut frames = project.meter_events();
        while let Ok(Some(frame)) = frames.recv().await {
            meters.apply(&store.order(), &frame.get().tracks);
        }
    });

    rsx! {}
}

/// A track's stereo meter, with REAPER's dB scale printed down its left.
///
/// The scale belongs *inside* the meter, not beside it: `mcp.meter` is one
/// rect covering both and REAPER draws the numbers as part of the widget.
/// Laid out as a separate column the bars have nowhere to go but under the
/// button stack — which is exactly what the first version of this strip did.
///
/// Levels are linear 0..1 as the frame carries them; the marks are labels,
/// not a conversion.
#[component]
pub fn TrackMeter(
    /// The track's GUID.
    track: String,
    /// The meter's box, which includes the scale when it is printed.
    #[props(default = 24)]
    width: u32,
    #[props(default = 120)] height: u32,
    /// Print the dB scale inside the meter, as the mixer does.
    #[props(default = true)]
    scale: bool,
) -> Element {
    let meters = use_meters();
    let guid = track.clone();
    let levels = use_memo(use_reactive!(|guid| meters.levels(&guid)));
    let l = levels();

    rsx! {
        art::Meter {
            levels: vec![l.peak_left, l.peak_right],
            cell: (width as f32, height as f32),
            scale: scale,
            marks: if scale { MARKS.iter().map(|m| m.to_string()).collect() } else { Vec::new() },
            width: Some(width),
            height: Some(height),
        }
    }
}

/// REAPER's own scale down the mixer meter, top to bottom.
const MARKS: [&str; 6] = ["-inf", "-6-", "-18-", "-30-", "-42-", "-54-"];

#[cfg(test)]
mod tests {
    use super::*;

    fn with_runtime(f: impl FnOnce(Meters)) {
        let mut dom = VirtualDom::new(|| rsx! { div {} });
        dom.rebuild_in_place();
        dom.in_runtime(|| {
            f(Meters { levels: Signal::new_in_scope(HashMap::new(), ScopeId::ROOT) })
        });
    }

    fn levels(peak: f32) -> TrackLevels {
        TrackLevels { peak_left: peak, peak_right: peak, hold_left: peak, hold_right: peak }
    }

    #[test]
    fn a_frame_lands_on_the_track_at_its_position() {
        with_runtime(|mut meters| {
            let order = vec!["A".to_string(), "B".to_string()];
            meters.apply(&order, &[levels(0.25), levels(0.75)]);
            assert_eq!(meters.levels("A").peak_left, 0.25);
            assert_eq!(meters.levels("B").peak_left, 0.75);
        });
    }

    /// The silent failure this ticket exists to prevent: after a reorder
    /// the same index means a different track, and meters that still move
    /// look fine while being wrong.
    #[test]
    fn a_reorder_re_aims_every_index() {
        with_runtime(|mut meters| {
            let before = vec!["A".to_string(), "B".to_string()];
            meters.apply(&before, &[levels(0.25), levels(0.75)]);

            // B moved above A. The frame is unchanged; only the order is.
            let after = vec!["B".to_string(), "A".to_string()];
            meters.apply(&after, &[levels(0.25), levels(0.75)]);
            assert_eq!(meters.levels("B").peak_left, 0.25, "B kept A's meter");
            assert_eq!(meters.levels("A").peak_left, 0.75, "A kept B's meter");
        });
    }

    #[test]
    fn a_frame_that_does_not_match_the_track_list_is_dropped() {
        with_runtime(|mut meters| {
            let order = vec!["A".to_string(), "B".to_string()];
            meters.apply(&order, &[levels(0.5), levels(0.5)]);
            // A track was added; this frame still describes the old mixer.
            let grown = vec!["A".to_string(), "B".to_string(), "C".to_string()];
            meters.apply(&grown, &[levels(0.9), levels(0.9)]);
            assert_eq!(meters.levels("A").peak_left, 0.5, "a short frame was indexed anyway");
            assert_eq!(meters.levels("C").peak_left, 0.0);
        });
    }

    /// A removed track must not keep a meter, or its last level sits there
    /// forever on a strip that no longer exists.
    #[test]
    fn a_track_that_leaves_takes_its_meter_with_it() {
        with_runtime(|mut meters| {
            meters.apply(&["A".to_string(), "B".to_string()], &[levels(0.4), levels(0.6)]);
            meters.apply(&["A".to_string()], &[levels(0.4)]);
            assert_eq!(meters.levels("B"), TrackLevels::default());
        });
    }
}
