//! Per-frame envelope evaluation for the render hot path.

use daw_proto::automation::EnvelopePoint;

use crate::plugin::{PluginParamEvent, PluginParamEventKind};

/// Stateful per-frame envelope cursor. `eval_at(t)` advances through
/// the points list without rescanning from the start each call.
/// Construct fresh per render block.
///
/// Time *must* be monotonically non-decreasing across `eval_at` calls
/// — meets the audio callback contract where frames march forward.
pub(crate) struct EnvelopeCursor<'a> {
    points: &'a [EnvelopePoint],
    /// Index of the next point ahead of the cursor. Once
    /// `points[next].time > t`, the active segment is
    /// `points[next-1]..points[next]`.
    next: usize,
}

impl<'a> EnvelopeCursor<'a> {
    pub(crate) fn new(points: &'a [EnvelopePoint]) -> Self {
        Self { points, next: 1 }
    }

    /// Evaluate the envelope at `time_seconds`. Returns `None` only
    /// if the envelope is empty.
    pub(crate) fn eval_at(&mut self, time_seconds: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        // Clamp left of first point.
        let first = &self.points[0];
        if time_seconds < first.time.as_seconds() {
            return Some(first.value);
        }
        // Clamp right of last point.
        let last = self.points.last().unwrap();
        if time_seconds >= last.time.as_seconds() {
            return Some(last.value);
        }
        // Advance `next` past every point at or before `t`: where two
        // points share a time the later holds from there (a jump).
        while self.next < self.points.len()
            && self.points[self.next].time.as_seconds() <= time_seconds
        {
            self.next += 1;
        }
        let a = &self.points[self.next - 1];
        let b = &self.points[self.next];
        let span = b.time.as_seconds() - a.time.as_seconds();
        if span <= 0.0 {
            return Some(b.value);
        }
        let f = (time_seconds - a.time.as_seconds()) / span;
        Some((b.value - a.value).mul_add(a.shape.ease(f, a.tension), a.value))
    }
}

/// An FX's parameters for a block starting `at` seconds: its stored values
/// (`fixed`), each automated one replaced by its envelope's value there.
/// Into `out`, cleared first and reused across blocks, so a block does not
/// allocate once it has grown to the chain's size.
pub(crate) fn params_at(
    fixed: &[(u32, f64)],
    envelopes: &[Automated],
    at: f64,
    out: &mut Vec<(u32, f64)>,
) {
    out.clear();
    out.extend_from_slice(fixed);
    for a in envelopes {
        let Some(value) = EnvelopeCursor::new(&a.points).eval_at(at) else {
            continue;
        };
        // An envelope is 0..1 along the parameter's range; the plugin
        // reads its plain value, under its own id.
        let value = (a.max - a.min).mul_add(value.clamp(0.0, 1.0), a.min);
        match out.iter_mut().find(|(i, _)| *i == a.id) {
            Some(slot) => slot.1 = value,
            None => out.push((a.id, value)),
        }
    }
}

/// How finely a moving automation segment is played as parameter events:
/// one event per this many frames (at the engine's rate), on a grid fixed to
/// the timeline, so a block's length never changes where they fall.
pub(crate) const STEP_FRAMES: u32 = 32;

/// The most parameter events one FX is handed in a block. Room for about a
/// hundred lanes all moving through a 1024-frame block; past it a lane holds
/// its last value to the block's end (see [`Schedule::dropped`]).
pub(crate) const FX_EVENT_CAPACITY: usize = 4096;

/// Where a block sits on the timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BlockClock {
    /// The timeline time of the block's first frame, seconds.
    pub(crate) start: f64,
    /// Timeline seconds each output frame advances (`rate / sample_rate`).
    pub(crate) seconds_per_frame: f64,
    /// The block's length, frames.
    pub(crate) frames: u32,
    /// The engine's sample rate: what [`STEP_FRAMES`] counts in.
    pub(crate) sample_rate: f64,
}

impl BlockClock {
    /// The first frame at or after timeline time `t`.
    fn frame_at(self, t: f64) -> u32 {
        // A hair under: a point that lands on a frame's time up to float
        // noise is that frame's, not the next one's.
        let f = ((t - self.start) / self.seconds_per_frame - 1e-6).ceil();
        if f <= 0.0 {
            0
        } else {
            f.min(u32::MAX as f64) as u32
        }
    }

    fn end(self) -> f64 {
        (self.frames as f64).mul_add(self.seconds_per_frame, self.start)
    }

    /// The length of one grid step, timeline seconds.
    fn step(self) -> f64 {
        STEP_FRAMES as f64 / self.sample_rate
    }
}

/// What [`schedule_params`] did with a block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub(crate) struct Schedule {
    /// Events written.
    pub(crate) events: u32,
    /// Lanes cut short because the buffer was full: each holds the last
    /// value it was sent to the block's end.
    pub(crate) dropped: u32,
}

/// An FX's parameter events for one block, sorted by frame, into `out`
/// (cleared first; it never grows past the capacity it came with, so the
/// audio thread does not allocate).
///
/// Every stored value (`fixed`) not automated is sent at frame 0. Each
/// automated lane is sent at frame 0 and then wherever it changes: at each
/// of its points, and every [`STEP_FRAMES`] along a moving segment, each
/// event holding the curve's value at the middle of the stretch it covers,
/// so the staircase sits on the curve. A flat stretch sends nothing.
pub(crate) fn schedule_params(
    fixed: &[(u32, f64)],
    envelopes: &[Automated],
    clock: BlockClock,
    out: &mut Vec<PluginParamEvent>,
) -> Schedule {
    out.clear();
    let mut dropped = 0;
    let room = out.capacity();
    for &(param_id, value) in fixed
        .iter()
        .filter(|(id, _)| !envelopes.iter().any(|a| a.id == *id))
    {
        if out.len() >= room {
            dropped += 1;
            continue;
        }
        out.push(PluginParamEvent {
            offset: 0,
            param_id,
            kind: PluginParamEventKind::Value(value),
        });
    }
    for (n, lane) in envelopes.iter().enumerate() {
        // Keep one slot for each lane still to come: every lane gets its
        // frame-0 value even when an earlier one is dense.
        let later = envelopes.len() - n - 1;
        let limit = room.saturating_sub(out.len() + later);
        let mut sent = 0;
        let cut = lane_events(&lane.points, clock, |offset, value| {
            if sent >= limit {
                return false;
            }
            sent += 1;
            out.push(PluginParamEvent {
                offset,
                param_id: lane.id,
                kind: PluginParamEventKind::Value(lane.plain(value)),
            });
            true
        });
        dropped += u32::from(cut);
    }
    out.sort_unstable_by_key(|e| e.offset);
    Schedule {
        events: out.len() as u32,
        dropped,
    }
}

/// Where a lane's value changes inside the block: `(frame, value)` in
/// frame order, at most one per frame, the first at frame 0, values 0..1.
/// `emit` returns `false` when it takes no more; then this returns `true`
/// (the lane was cut short).
fn lane_events(
    points: &[EnvelopePoint],
    clock: BlockClock,
    mut emit: impl FnMut(u32, f64) -> bool,
) -> bool {
    if points.is_empty() || clock.frames == 0 {
        return false;
    }
    let end = clock.end();
    let step = clock.step();
    let mut t = clock.start;
    // `points[next]` is the first point after `t`: the segment is
    // `points[next - 1]..points[next]`.
    let mut next = points.partition_point(|p| p.time.as_seconds() <= t);
    // The value waiting to be sent at `pending.0`: a later stretch that
    // starts on the same frame replaces it.
    let mut pending: Option<(u32, f64)> = None;
    let mut sent: Option<f64> = None;
    let mut flush = |at: u32, value: f64, sent: &mut Option<f64>| -> bool {
        if sent.is_some_and(|s| (s - value).abs() <= VALUE_EPSILON) {
            return true;
        }
        *sent = Some(value);
        emit(at, value)
    };
    loop {
        let segment = Segment::of(points, next);
        let point_after = points
            .get(next)
            .map_or(f64::INFINITY, |p| p.time.as_seconds());
        // The stretch one event covers, and where its value is read: the
        // middle of its grid step, cut at points (never at the block's end,
        // so a block's length does not change the value).
        let (lo, hi) = if segment.moves() {
            let mut cell = (t / step).floor() * step;
            if cell + step <= t {
                cell += step; // float noise put `t` on the grid line
            }
            (cell.max(segment.from), (cell + step).min(point_after))
        } else {
            (t, point_after)
        };
        let value = segment.value_at(0.5 * (lo + hi.min(lo + step)));
        let at = clock.frame_at(t);
        if at >= clock.frames {
            break;
        }
        match pending {
            Some((frame, _)) if frame == at => pending = Some((at, value)),
            Some((frame, held)) => {
                if !flush(frame, held, &mut sent) {
                    return true;
                }
                pending = Some((at, value));
            }
            None => pending = Some((at, value)),
        }
        if hi >= end {
            break;
        }
        t = hi;
        while points.get(next).is_some_and(|p| p.time.as_seconds() <= t) {
            next += 1;
        }
    }
    match pending {
        Some((frame, value)) => !flush(frame, value, &mut sent),
        None => false,
    }
}

/// Two values closer than this (along the 0..1 range) are the same value: a
/// lane holding still sends nothing.
const VALUE_EPSILON: f64 = 1e-9;

/// The piece of an envelope around a time: between two points, or held
/// before the first or after the last.
#[derive(Clone, Copy)]
struct Segment<'a> {
    /// Where it starts (`-inf` before the first point).
    from: f64,
    a: &'a EnvelopePoint,
    /// `None` when held (before the first point or after the last).
    b: Option<&'a EnvelopePoint>,
}

impl<'a> Segment<'a> {
    /// The segment ending at `points[next]` (`points` not empty).
    fn of(points: &'a [EnvelopePoint], next: usize) -> Self {
        match next {
            0 => Self {
                from: f64::NEG_INFINITY,
                a: &points[0],
                b: None,
            },
            n if n >= points.len() => Self {
                from: points[points.len() - 1].time.as_seconds(),
                a: &points[points.len() - 1],
                b: None,
            },
            n => Self {
                from: points[n - 1].time.as_seconds(),
                a: &points[n - 1],
                b: Some(&points[n]),
            },
        }
    }

    /// Whether its value changes along it.
    fn moves(self) -> bool {
        self.b.is_some_and(|b| {
            b.value != self.a.value && self.a.shape != daw_proto::automation::EnvelopeShape::Square
        })
    }

    /// Its value at `t`: what [`EnvelopeCursor::eval_at`] gives there.
    fn value_at(self, t: f64) -> f64 {
        let (a, Some(b)) = (self.a, self.b) else {
            return self.a.value;
        };
        let span = b.time.as_seconds() - a.time.as_seconds();
        if span <= 0.0 {
            return b.value;
        }
        let f = (t - a.time.as_seconds()) / span;
        (b.value - a.value).mul_add(a.shape.ease(f, a.tension), a.value)
    }
}

/// One automated parameter of an FX: the id its plugin reads it under, its
/// plain range, and its envelope (0..1 along that range).
#[derive(Clone, Debug)]
pub(crate) struct Automated {
    pub(crate) id: u32,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) points: Vec<EnvelopePoint>,
}

impl Automated {
    /// The plugin's plain value for envelope value `v` (0..1 along the range).
    fn plain(&self, v: f64) -> f64 {
        (self.max - self.min).mul_add(v.clamp(0.0, 1.0), self.min)
    }
}

#[cfg(test)]
mod params_tests {
    use super::*;

    fn point(time: f64, value: f64) -> EnvelopePoint {
        EnvelopePoint {
            time: daw_proto::primitives::PositionInSeconds::from_seconds(time),
            value,
            ..EnvelopePoint::default()
        }
    }

    /// An automated parameter takes its envelope's value at the block;
    /// the rest keep theirs.
    #[test]
    fn an_automated_parameter_follows_its_envelope() {
        let fixed = [(0, 0.2), (1, 0.7)];
        let envelopes = vec![Automated {
            id: 1,
            min: 0.0,
            max: 1.0,
            points: vec![point(0.0, 0.0), point(2.0, 1.0)],
        }];
        let mut out = Vec::new();
        params_at(&fixed, &envelopes, 1.0, &mut out);
        assert_eq!(out[0], (0, 0.2));
        assert!((out[1].1 - 0.5).abs() < 1e-9, "{out:?}");
        params_at(&fixed, &envelopes, 3.0, &mut out);
        assert!((out[1].1 - 1.0).abs() < 1e-9, "held after the last point");
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn point(time: f64, value: f64) -> EnvelopePoint {
        EnvelopePoint {
            time: daw_proto::primitives::PositionInSeconds::from_seconds(time),
            value,
            ..EnvelopePoint::default()
        }
    }

    fn lane(id: u32, points: Vec<EnvelopePoint>) -> Automated {
        Automated {
            id,
            min: 0.0,
            max: 1.0,
            points,
        }
    }

    fn clock(start_frame: u64, frames: u32) -> BlockClock {
        BlockClock {
            start: start_frame as f64 / RATE,
            seconds_per_frame: 1.0 / RATE,
            frames,
            sample_rate: RATE,
        }
    }

    fn buffer() -> Vec<PluginParamEvent> {
        Vec::with_capacity(FX_EVENT_CAPACITY)
    }

    fn value(e: &PluginParamEvent) -> f64 {
        match e.kind {
            PluginParamEventKind::Value(v) => v,
            PluginParamEventKind::Mod(_) => panic!("a modulation"),
        }
    }

    /// The value parameter `id` holds at each frame of `frames` blocks of
    /// `block` from frame 0, as a plugin applying each event from its frame.
    fn held(lanes: &[Automated], id: u32, block: u32, frames: u32) -> Vec<f64> {
        let mut out = buffer();
        let mut held = Vec::with_capacity(frames as usize);
        let mut now = f64::NAN;
        for start in (0..frames).step_by(block as usize) {
            let _ = schedule_params(&[], lanes, clock(u64::from(start), block), &mut out);
            let mut events = out.iter().filter(|e| e.param_id == id).peekable();
            for f in 0..block {
                while let Some(e) = events.next_if(|e| e.offset == f) {
                    now = value(e);
                }
                held.push(now);
            }
        }
        held
    }

    #[test]
    fn a_flat_lane_sends_one_event_a_block() {
        let flat = [lane(3, vec![point(0.0, 0.4), point(10.0, 0.4)])];
        let mut out = buffer();
        for b in 0..20 {
            let s = schedule_params(&[], &flat, clock(b * 1024, 1024), &mut out);
            assert_eq!(s.events, 1);
            assert_eq!(out[0].offset, 0);
        }
    }

    #[test]
    fn a_moving_segment_is_sent_every_step_on_the_curve() {
        let ramp = [lane(0, vec![point(0.0, 0.0), point(1.0, 1.0)])];
        let mut out = buffer();
        let s = schedule_params(&[], &ramp, clock(4096, 1024), &mut out);
        assert_eq!(s.events, 1024 / STEP_FRAMES);
        assert!(out.iter().all(|e| e.offset % STEP_FRAMES == 0));
        // Each holds the middle of its step: off by half a step's rise at most.
        let held = held(&ramp, 0, 1024, 8192);
        let half_step = 0.5 * f64::from(STEP_FRAMES) / RATE;
        for (f, v) in held.iter().enumerate() {
            let want = f as f64 / RATE;
            assert!(
                (v - want).abs() <= half_step + 1e-12,
                "frame {f}: {v} for {want}"
            );
        }
    }

    #[test]
    fn a_jump_lands_on_its_frame() {
        let jump = [lane(
            0,
            vec![point(0.0, 0.2), point(0.01, 0.2), point(0.01, 0.9)],
        )];
        let held = held(&jump, 0, 1024, 2048);
        let at = (0.01 * RATE).ceil() as usize; // 480
        assert_eq!(held[at - 1], 0.2);
        assert_eq!(held[at], 0.9);
    }

    #[test]
    fn where_events_fall_does_not_depend_on_the_block_size() {
        let ride = [lane(
            0,
            vec![
                point(0.0, 0.1),
                point(0.0123, 0.7),
                point(0.0311, 0.7),
                point(0.0311, 0.2),
                point(0.0702, 0.55),
            ],
        )];
        let long = held(&ride, 0, 1024, 4096);
        for block in [64, 128, 256, 333, 512] {
            let frames = 4096 / block * block;
            let short = held(&ride, 0, block, frames);
            assert_eq!(&short[..], &long[..frames as usize], "blocks of {block}");
        }
    }

    #[test]
    fn a_full_buffer_cuts_lanes_short_but_every_lane_starts_the_block() {
        let lanes: Vec<Automated> = (0..4)
            .map(|id| lane(id, vec![point(0.0, 0.0), point(1.0, 1.0)]))
            .collect();
        let mut out = Vec::with_capacity(10);
        let s = schedule_params(&[(9, 0.5)], &lanes, clock(0, 1024), &mut out);
        assert_eq!(out.capacity(), 10, "never grown");
        assert_eq!(s.events, 10);
        assert!(s.dropped > 0);
        for id in 0..4 {
            assert!(
                out.iter().any(|e| e.param_id == id && e.offset == 0),
                "lane {id}"
            );
        }
        assert!(
            out.windows(2).all(|w| w[0].offset <= w[1].offset),
            "in frame order"
        );
    }

    #[test]
    fn a_stored_value_goes_out_unless_a_lane_plays_it() {
        let lanes = [lane(1, vec![point(0.0, 0.25)])];
        let mut out = buffer();
        let _ = schedule_params(&[(0, 0.5), (1, 0.9)], &lanes, clock(0, 512), &mut out);
        let mut at_start: Vec<(u32, f64)> = out.iter().map(|e| (e.param_id, value(e))).collect();
        at_start.sort_by_key(|(id, _)| *id);
        assert_eq!(at_start, vec![(0, 0.5), (1, 0.25)]);
    }

    #[test]
    fn scheduling_allocates_nothing() {
        let lanes: Vec<Automated> = (0..100)
            .map(|id| lane(id, vec![point(0.0, 0.0), point(0.01, 1.0), point(2.0, 0.3)]))
            .collect();
        let mut out = buffer();
        let ((), allocations) = crate::alloc_count::allocations(|| {
            for b in 0..50 {
                let _ = schedule_params(&[(500, 0.1)], &lanes, clock(b * 1024, 1024), &mut out);
            }
        });
        assert_eq!(allocations, 0);
    }
}
