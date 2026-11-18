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
    /// The same every [`PEAK_GRAIN`] frames (`grains` of them a track,
    /// `grain_peaks[track * grains + g]`): a live block takes the peaks of
    /// its own frames, not of the whole chunk it falls in, so a meter on a
    /// track rendered ahead moves with what is heard.
    grains: usize,
    grain_peaks: Vec<(f32, f32)>,
    /// Lanes of an earlier fill, kept for their buffers: a feed refilled
    /// on the audio thread allocates only as it grows.
    spare: Vec<Lane>,
}

/// Frames a feed's meter peaks are kept by: a little under 1.5 ms at 48 kHz.
pub(crate) const PEAK_GRAIN: usize = 64;

/// The peaks of interleaved stereo `samples`, left and right.
pub(crate) fn peak(samples: &[f32]) -> (f32, f32) {
    samples.chunks_exact(2).fold((0.0, 0.0), |(l, r), fr| {
        (f32::max(l, fr[0].abs()), f32::max(r, fr[1].abs()))
    })
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
        self.spare.append(&mut self.buses);
        self.spare.append(&mut self.sidechains);
        self.peaks.clear();
        self.peaks.resize(tracks, None);
        self.grains = frames.div_ceil(PEAK_GRAIN);
        self.grain_peaks.clear();
        self.grain_peaks.resize(tracks * self.grains, (0.0, 0.0));
    }

    /// Track `track`'s post-fader signal over the feed's frames (`post`,
    /// interleaved; empty: silence), metered: its peaks over all of them,
    /// and grain by grain.
    pub(crate) fn meter(&mut self, track: usize, post: &[f32]) {
        if let Some(slot) = self.peaks.get_mut(track) {
            *slot = Some(peak(post));
        }
        let grains = self.grains;
        let Some(cells) = self
            .grain_peaks
            .get_mut(track * grains..(track + 1) * grains)
        else {
            return;
        };
        for (g, cell) in cells.iter_mut().enumerate() {
            let span = g * PEAK_GRAIN * 2..((g + 1) * PEAK_GRAIN * 2).min(post.len());
            *cell = post.get(span).map_or((0.0, 0.0), peak);
        }
    }

    /// Track `track`'s peaks over frames `from..from + frames`, to the
    /// grain; `None` where it was not metered.
    fn peaks_over(&self, track: usize, from: usize, frames: usize) -> Option<(f32, f32)> {
        let whole = (*self.peaks.get(track)?)?;
        if self.grains == 0 {
            return Some(whole);
        }
        let first = track * self.grains + from / PEAK_GRAIN;
        let last = track * self.grains + (from + frames).div_ceil(PEAK_GRAIN).min(self.grains);
        let cells = self.grain_peaks.get(first..last)?;
        Some(cells.iter().fold((0.0, 0.0), |(l, r), (a, b)| {
            (f32::max(l, *a), f32::max(r, *b))
        }))
    }

    /// Room for `frames` frames and `lanes` lanes of them without
    /// allocating — for a feed filled on the audio thread.
    pub(crate) fn reserve(&mut self, frames: usize, lanes: usize) {
        self.master.reserve(frames * 2);
        self.buses.reserve(lanes);
        self.sidechains.reserve(lanes);
        self.spare.reserve(lanes * 2);
        while self.spare.len() < lanes {
            self.spare.push(Lane {
                track: 0,
                samples: Vec::with_capacity(frames * 2),
            });
        }
    }

    /// `samples` as track `track`'s bus (or, `sidechain`, its sidechain),
    /// in a spare lane's buffer.
    pub(crate) fn give_lane(&mut self, track: usize, samples: &[f32], sidechain: bool) {
        let mut lane = self.spare.pop().unwrap_or_default();
        lane.track = track;
        lane.samples.clear();
        lane.samples.extend_from_slice(samples);
        if sidechain {
            self.sidechains.push(lane);
        } else {
            self.buses.push(lane);
        }
    }

    /// Empty, as a live block's window over the feeds rendered ahead —
    /// nothing freed (the audio thread fills it), its lanes kept for the
    /// next window, silent until filled.
    pub(crate) fn clear_window(&mut self) {
        self.frames = 0;
        self.print = 0;
        self.master.clear();
        for lane in self.buses.iter_mut().chain(self.sidechains.iter_mut()) {
            lane.samples.clear();
        }
        self.peaks.fill(None);
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
        if self.peaks.len() < src.peaks.len() {
            self.peaks.resize(src.peaks.len(), None);
        }
        for track in 0..src.peaks.len() {
            if let Some((l, r)) = src.peaks_over(track, from, frames) {
                let (ml, mr) = self.peaks[track].get_or_insert((0.0, 0.0));
                *ml = ml.max(l);
                *mr = mr.max(r);
            }
        }
    }

    /// Frames `at..at + frames` of this one faded in over `old`'s frames
    /// `from..`: each frame `k` is `old · (1 − g) + this · g`, `g =
    /// gain(k)` — a switch from what was rendered before an edit to what
    /// was rendered after it, without a click. Linear: the two are the
    /// same song a fader's move apart, so they add up in phase and a
    /// linear fade keeps the level where an equal-power one would swell
    /// by 3 dB at its middle. A lane only one side has fades against
    /// silence.
    pub(crate) fn fade_from(
        &mut self,
        at: usize,
        old: &Feed,
        from: usize,
        frames: usize,
        gain: impl Fn(usize) -> f32,
    ) {
        fade_samples(&mut self.master, at, &old.master, from, frames, &gain);
        fade_lanes(
            &mut self.buses,
            at,
            &old.buses,
            from,
            frames,
            self.frames,
            &gain,
        );
        fade_lanes(
            &mut self.sidechains,
            at,
            &old.sidechains,
            from,
            frames,
            self.frames,
            &gain,
        );
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

/// `new`'s interleaved frames `at..at + frames` faded in over `old`'s from
/// `from` (`old` silence where it is short).
fn fade_samples(
    new: &mut [f32],
    at: usize,
    old: &[f32],
    from: usize,
    frames: usize,
    gain: &impl Fn(usize) -> f32,
) {
    for k in 0..frames {
        let g = gain(k);
        for c in 0..2 {
            let o = old.get((from + k) * 2 + c).copied().unwrap_or(0.0);
            if let Some(n) = new.get_mut((at + k) * 2 + c) {
                *n = o * (1.0 - g) + *n * g;
            }
        }
    }
}

/// [`fade_samples`] lane by lane, matched by track: a lane `old` has and
/// `new` does not is added (`new` silent there), sized to `len` frames.
fn fade_lanes(
    new: &mut Vec<Lane>,
    at: usize,
    old: &[Lane],
    from: usize,
    frames: usize,
    len: usize,
    gain: &impl Fn(usize) -> f32,
) {
    for lane in new.iter_mut() {
        let theirs = old
            .iter()
            .find(|o| o.track == lane.track)
            .map_or(&[][..], |o| o.samples.as_slice());
        fade_samples(&mut lane.samples, at, theirs, from, frames, gain);
    }
    for lane in old {
        if new.iter().any(|n| n.track == lane.track) {
            continue;
        }
        let mut samples = vec![0.0; len * 2];
        fade_samples(&mut samples, at, &lane.samples, from, frames, gain);
        new.push(Lane {
            track: lane.track,
            samples,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Halfway through a fade the frame is half each; a bus only the old
    /// side carries fades out against silence.
    #[test]
    fn a_fade_is_linear_between_the_two() {
        let old = feed(4, 1.0, Some((2, 4.0)));
        let mut new = feed(4, 3.0, None);
        new.fade_from(0, &old, 0, 4, |k| k as f32 / 4.0);
        assert_eq!(new.master[..2], [1.0, 1.0], "all old at the start");
        assert_eq!(new.master[4..6], [2.0, 2.0], "half each at the middle");
        assert_eq!(new.buses.len(), 1);
        assert_eq!(new.buses[0].samples[4], 2.0, "half the old bus");
    }

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

    /// A block that takes the start of a chunk meters its own frames: a
    /// peak late in the chunk is not on its meter until its frames play.
    #[test]
    fn a_window_meters_its_own_frames() {
        let mut chunk = Feed::default();
        chunk.reset(512, 7, 2);
        let mut post = vec![0.1; 512 * 2];
        post[450 * 2] = 0.9;
        chunk.meter(1, &post);
        assert_eq!(chunk.peaks[1], Some((0.9, 0.1)), "the chunk's own peak");
        let mut w = Feed::default();
        w.reset(0, 7, 2);
        w.append(&chunk, 0, 128);
        assert_eq!(w.peaks[1], Some((0.1, 0.1)), "not yet");
        assert_eq!(w.peaks[0], None, "not metered");
        w.clear_window();
        w.append(&chunk, 384, 128);
        assert_eq!(w.peaks[1], Some((0.9, 0.1)), "as its frames play");
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
