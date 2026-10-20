//! The dual-buffer split: which tracks a render covers, and what passes
//! between the two halves.
//!
//! A song's tracks fall in two sets ([`super::graph::live_set`]): the live
//! ones — armed and monitoring, played now, and everything they feed — and
//! the rest, which only play back what is already recorded. The rest can be
//! rendered ahead of the playhead, in big blocks, on threads with time to
//! spare; only the live ones need the device's small block, in the audio
//! callback. Cubase calls this ASIO-Guard, REAPER anticipative FX.
//!
//! An [`Part::Ahead`] render runs the rest and leaves in a [`Feed`] what
//! they hand the live ones: what they sum into each live bus and sidechain,
//! and into master (before the metronome and the master fader), with the
//! meter peaks they would have written. A [`Part::Live`] render starts each
//! of those buses from the feed and runs only the live tracks, then master.
//! The two together are a [`Part::Whole`] render, sample for sample.

/// Which tracks a render covers.
pub(crate) enum Part<'a> {
    /// Every track, then master: a render as there always was.
    Whole,
    /// The tracks that are not live; what they hand the live ones left in
    /// the feed.
    Ahead(&'a mut Feed),
    /// The live tracks, each bus started from the feed; then master.
    Live(&'a Feed),
}

impl<'a> Part<'a> {
    /// Which side this is, without the feed.
    pub(crate) const fn side(&self) -> Side {
        match self {
            Self::Whole => Side::Whole,
            Self::Ahead(_) => Side::Ahead,
            Self::Live(_) => Side::Live,
        }
    }

    /// The feed an ahead render fills, and the one a live render reads.
    pub(crate) fn feeds(self) -> (Option<&'a mut Feed>, Option<&'a Feed>) {
        match self {
            Self::Whole => (None, None),
            Self::Ahead(f) => (Some(f), None),
            Self::Live(f) => (None, Some(f)),
        }
    }

    /// Whether this render runs track `ti`, `live` marking the live set.
    pub(crate) fn covers(&self, live: &[bool], ti: usize) -> bool {
        self.side().covers(live, ti)
    }
}

/// A [`Part`] without its feed: what a render asks of it per track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Whole,
    Ahead,
    Live,
}

impl Side {
    /// Whether this render runs track `ti`, `live` marking the live set.
    pub(crate) fn covers(self, live: &[bool], ti: usize) -> bool {
        let is_live = live.get(ti).copied().unwrap_or(false);
        match self {
            Self::Whole => true,
            Self::Ahead => !is_live,
            Self::Live => is_live,
        }
    }

    /// Whether this render finishes the block: the live input, master's
    /// metronome and fader, the meters written as it goes.
    pub(crate) const fn finishes(self) -> bool {
        !matches!(self, Self::Ahead)
    }
}

/// What the tracks rendered ahead hand the live ones over a run of frames.
/// Every buffer is interleaved stereo.
#[derive(Debug, Default, Clone)]
pub struct Feed {
    pub(crate) frames: usize,
    /// The live set it was rendered for ([`super::graph::live_print`]).
    pub(crate) print: u64,
    /// What reached master, before the metronome and the master fader.
    pub(crate) master: Vec<f32>,
    /// Per live track that received from the ahead ones: its bus's sum.
    pub(crate) buses: Vec<Lane>,
    /// Per live track keyed from an ahead one: its sidechain (3/4).
    pub(crate) sidechains: Vec<Lane>,
    /// Each track's post-fader peaks where the ahead render metered it.
    pub(crate) peaks: Vec<Option<(f32, f32)>>,
}

/// One live track's share of a [`Feed`].
#[derive(Debug, Default, Clone)]
pub(crate) struct Lane {
    pub(crate) track: usize,
    pub(crate) samples: Vec<f32>,
}

impl Feed {
    /// Empty, for `frames` frames of the live set `print`, `tracks` long —
    /// its buffers kept, so a feed refilled every block allocates only as
    /// it grows.
    pub(crate) fn reset(&mut self, frames: usize, print: u64, tracks: usize) {
        self.frames = frames;
        self.print = print;
        self.master.clear();
        self.master.resize(frames * 2, 0.0);
        self.buses.clear();
        self.sidechains.clear();
        self.peaks.clear();
        self.peaks.resize(tracks, None);
    }

    /// Frames in it.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Its live set's print.
    #[must_use]
    pub fn print(&self) -> u64 {
        self.print
    }

    /// `src`'s frames `from..from + frames` appended to this one — a live
    /// block's window of the feeds rendered ahead, which a block can take
    /// from two of. Both must be of the same live set.
    pub(crate) fn append(&mut self, src: &Feed, from: usize, frames: usize) {
        let span = from * 2..(from + frames) * 2;
        let at = self.frames;
        self.frames += frames;
        if let Some(s) = src.master.get(span.clone()) {
            self.master.extend_from_slice(s);
        }
        append_lanes(&mut self.buses, &src.buses, span.clone(), at, self.frames);
        append_lanes(&mut self.sidechains, &src.sidechains, span, at, self.frames);
        for (mine, theirs) in self.peaks.iter_mut().zip(&src.peaks) {
            if let Some((l, r)) = theirs {
                let (ml, mr) = mine.get_or_insert((0.0, 0.0));
                *ml = ml.max(*l);
                *mr = mr.max(*r);
            }
        }
    }

    /// Silence for `frames` more frames: what a live block hears of the
    /// tracks ahead where no feed was ready.
    pub(crate) fn append_silence(&mut self, frames: usize) {
        self.frames += frames;
        self.master.resize(self.frames * 2, 0.0);
        for lane in self.buses.iter_mut().chain(self.sidechains.iter_mut()) {
            lane.samples.resize(self.frames * 2, 0.0);
        }
    }
}

/// `src`'s lanes' `span` added after frame `at` of `dst`'s, which end at
/// `frames`: a lane only one side has is silence on the other.
fn append_lanes(
    dst: &mut Vec<Lane>,
    src: &[Lane],
    span: std::ops::Range<usize>,
    at: usize,
    frames: usize,
) {
    for lane in src {
        let slot = match dst.iter().position(|l| l.track == lane.track) {
            Some(i) => i,
            None => {
                dst.push(Lane {
                    track: lane.track,
                    samples: Vec::new(),
                });
                dst.len() - 1
            }
        };
        let samples = &mut dst[slot].samples;
        samples.resize(at * 2, 0.0);
        if let Some(s) = lane.samples.get(span.clone()) {
            samples.extend_from_slice(s);
        }
    }
    for lane in dst.iter_mut() {
        lane.samples.resize(frames * 2, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(frames: usize, master: f32, bus: Option<(usize, f32)>) -> Feed {
        let mut f = Feed::default();
        f.reset(frames, 7, 3);
        f.master.fill(master);
        if let Some((track, v)) = bus {
            f.buses.push(Lane {
                track,
                samples: vec![v; frames * 2],
            });
        }
        f
    }

    /// A live block across two feeds takes the end of one and the start of
    /// the next; a bus only the second carries is silent before it.
    #[test]
    fn a_window_across_two_feeds_is_both_in_order() {
        let (a, b) = (feed(4, 1.0, None), feed(4, 2.0, Some((2, 5.0))));
        let mut w = Feed::default();
        w.reset(0, 7, 3);
        w.append(&a, 3, 1);
        w.append(&b, 0, 2);
        assert_eq!(w.frames, 3);
        assert_eq!(w.master, vec![1.0, 1.0, 2.0, 2.0, 2.0, 2.0]);
        assert_eq!(w.buses.len(), 1);
        assert_eq!(w.buses[0].samples, vec![0.0, 0.0, 5.0, 5.0, 5.0, 5.0]);
    }

    #[test]
    fn silence_pads_every_lane() {
        let mut w = feed(1, 1.0, Some((0, 3.0)));
        w.append_silence(2);
        assert_eq!(w.master, vec![1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(w.buses[0].samples.len(), 6);
    }

    #[test]
    fn a_part_covers_its_side_of_the_live_set() {
        let live = [true, false];
        let mut f = Feed::default();
        assert!(Part::Whole.covers(&live, 0) && Part::Whole.covers(&live, 1));
        assert!(!Part::Ahead(&mut f).covers(&live, 0));
        assert!(Part::Ahead(&mut f).covers(&live, 1));
        assert!(Part::Live(&Feed::default()).covers(&live, 0));
        assert!(!Part::Live(&Feed::default()).covers(&live, 1));
    }
}
