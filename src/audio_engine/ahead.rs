//! The render ahead: the dual-buffer engine (Cubase's ASIO-Guard, REAPER's
//! anticipative FX processing).
//!
//! The audio callback has a deadline every block — 2.7 ms at 128 frames —
//! and a busy machine takes some of it, however fast the render. Most of a
//! song needs none of that urgency: tracks that play back what is already
//! recorded can be rendered any time before they are heard. So a thread of
//! its own ([`Worker`]) renders those tracks ([`Part::Ahead`]) up to
//! [`AheadPrefs::depth_ms`] ahead of the playhead, in big chunks, and hands
//! the callback what they feed the rest ([`Feed`]); the callback renders only
//! the live tracks — armed and monitoring, played now, and what they feed —
//! at the device's block, starting from that feed ([`Part::Live`]). A spike
//! on the machine eats into the slack ahead, not into the deadline, and a
//! song too big for one block's budget still plays.
//!
//! **Edits are heard at once.** A change to a track rendered ahead — a
//! fader, a plugin's parameter, an item under the playhead — makes what
//! was rendered past the playhead stale. The render ahead sees it (its
//! snapshot's [ahead print](super::render::Prints) moved), starts again
//! [`AheadPrefs::guard_ms`] past the playhead — a new *edition* of the run
//! from that frame on, its *cut* — and renders the frames nearest the
//! playhead first. The callback plays whatever is freshest for each frame
//! and crossfades ([`AheadPrefs::fade_ms`]) where it goes from one edition
//! to the next; where the new edition has not come yet it plays the old
//! one — the edit heard a little late, never a gap. While edits keep coming
//! (a fader dragged) the render ahead stays only a little ahead
//! ([`AheadPrefs::edit_depth_ms`]), so each edit throws little away; once
//! they stop it grows back to its depth.
//!
//! The trade-off: the plugins on the tracks ahead have already heard the
//! frames an edit throws away, and hear them again, edited — a reverb's
//! tail holds the stretch past the cut twice. The shallow depth while
//! editing keeps that stretch short (tens of ms, inside the tail), and the
//! crossfade keeps the switch itself click-free.
//!
//! **A run starts without a gap.** At play, a locate or a changed live set
//! (a track armed or disarmed), nothing is rendered ahead yet. The thread
//! is told to rest, and once it does the callback renders the tracks ahead
//! itself, with the same renderer and so the same plugins, this block and
//! — while the blocks leave time — the next. Once it is a few blocks
//! ahead it hands the run over and the thread goes on from that very frame.
//! Each plugin hears every frame once, in order. Until that renderer is
//! free (the thread finishing a chunk), or its snapshot is of the new live
//! set, the callback renders every track as it did before the render
//! ahead. The plugins ahead of a locate or a new live set, having heard
//! past where the run starts again, hear it from there: the same jump an
//! edit makes.
//!
//! The two sides agree on where the song is by *run*: every chunk says
//! which run of the transport it belongs to (its generation), which frame
//! of the run it starts at, and where that is on the timeline. Anything
//! the render ahead did not foresee — a locate, play after stop, a loop it
//! did not know of, a changed live set — starts a new run from where the
//! playhead is. Loops are foreseen: the render ahead knows the device's
//! block and wraps where the transport will. Anything the plan cannot
//! follow — stopped, varispeed, a locate landing mid-block — renders whole
//! in the callback, as before, with the render ahead paused.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use audio::io::duplex::EngineStats;

use super::render::{Feed, Part, Prints, ProjectRenderer, StereoBuffer};
use crate::transport_engine::{BlockPlan, TransportShared};

/// How the render ahead runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AheadPrefs {
    /// How far ahead of the playhead to render, ms.
    pub depth_ms: f64,
    /// The biggest chunk it renders at once, frames.
    pub chunk: usize,
    /// How far past the playhead an edit is heard from, ms, beyond the
    /// device's block: the time the render ahead has to render it.
    pub guard_ms: f64,
    /// How far ahead to render while edits come in, ms.
    pub edit_depth_ms: f64,
    /// How long after the last edit the render ahead goes back to its
    /// full depth.
    pub quiet_ms: f64,
    /// The crossfade from what was rendered before an edit to what was
    /// rendered after it, ms.
    pub fade_ms: f64,
}

impl Default for AheadPrefs {
    fn default() -> Self {
        Self {
            depth_ms: DEFAULT_DEPTH_MS,
            chunk: CHUNK,
            guard_ms: 3.0,
            edit_depth_ms: 25.0,
            quiet_ms: 300.0,
            fade_ms: 5.0,
        }
    }
}

/// REAPER's default render-ahead.
pub const DEFAULT_DEPTH_MS: f64 = 200.0;

/// The biggest chunk: big enough that a plugin's per-call cost is spread
/// thin, small enough that the first frames after a restart are not long
/// in coming.
const CHUNK: usize = 1024;

impl AheadPrefs {
    /// From the device prefs: `None` when the render ahead is off.
    #[must_use]
    pub fn from_io(prefs: &audio::io::AudioIoPrefs) -> Option<Self> {
        if prefs.no_render_ahead {
            return None;
        }
        let depth_ms = match prefs.render_ahead_ms {
            0 => DEFAULT_DEPTH_MS,
            ms => f64::from(ms),
        };
        Some(Self {
            depth_ms,
            ..Self::default()
        })
    }

    /// [`from_io`](Self::from_io), unless `FTS_RENDER_AHEAD_MS` says
    /// otherwise for this run (`0`: off) — for hearing the difference.
    #[must_use]
    pub fn chosen(prefs: &audio::io::AudioIoPrefs) -> Option<Self> {
        let asked = std::env::var("FTS_RENDER_AHEAD_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        match asked {
            Some(0) => None,
            Some(ms) => Self::from_io(&audio::io::AudioIoPrefs {
                render_ahead_ms: ms,
                no_render_ahead: false,
                ..prefs.clone()
            }),
            None => Self::from_io(prefs),
        }
    }

    fn frames(ms: f64, sample_rate: u32) -> usize {
        (ms.max(0.0) * f64::from(sample_rate) / 1000.0) as usize
    }
}

/// Where the render ahead stands on the timeline: the next frame it renders
/// (output-rate samples), and how far into one of the device's blocks that
/// frame falls — the transport only wraps a loop at the end of a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Course {
    pub(crate) at: f64,
    pub(crate) block: usize,
    pub(crate) into_block: usize,
}

/// A loop the transport wraps at, output-rate samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wrap {
    pub(crate) start: f64,
    pub(crate) end: f64,
}

impl Wrap {
    /// Where the transport goes from `at`, a block's end at or past the
    /// loop's end — as `TransportShared::advanced` wraps it.
    fn wrapped(self, at: f64) -> f64 {
        self.start + (at - self.start).rem_euclid(self.end - self.start)
    }
}

impl Course {
    /// The next chunk of at most `want` frames: how many to render from
    /// [`at`](Self::at), and where the course stands after them. A chunk
    /// never runs past the end of the block that wraps the loop; the course
    /// after that block starts where the transport wraps to.
    pub(crate) fn next(self, want: usize, wrap: Option<Wrap>) -> (usize, Course) {
        let block = self.block.max(1);
        let want = want.max(1);
        let to_boundary = block - self.into_block.min(block - 1);
        // The end of the first block that ends at or past the loop's end.
        let wrap_after = wrap.filter(|w| w.end > w.start).map(|w| {
            let first = self.at + to_boundary as f64;
            let blocks = ((w.end - first) / block as f64).ceil().max(0.0) as usize;
            (to_boundary + blocks * block, w)
        });
        let frames = match wrap_after {
            Some((limit, _)) => want.min(limit),
            None => want,
        };
        let next = match wrap_after {
            Some((limit, w)) if frames == limit => Course {
                at: w.wrapped(self.at + limit as f64),
                block,
                into_block: 0,
            },
            _ => Course {
                at: self.at + frames as f64,
                block,
                into_block: (self.into_block + frames) % block,
            },
        };
        (frames, next)
    }

    /// The course `frames` on, wrapping where the transport would.
    pub(crate) fn skip(mut self, mut frames: usize, wrap: Option<Wrap>) -> Course {
        while frames > 0 {
            let (went, next) = self.next(frames, wrap);
            self = next;
            frames -= went;
        }
        self
    }
}

/// How near a frame's position has to be to count as it, samples.
const SAME_FRAME: f64 = 0.5;

/// One chunk rendered ahead.
#[derive(Default)]
struct Chunk {
    /// The run it belongs to, and its edition of that run.
    generation: u64,
    edition: u64,
    /// The frame of the run it starts at, and where that is on the
    /// timeline.
    first: u64,
    start: f64,
    frames: usize,
    feed: Feed,
}

impl Chunk {
    fn end(&self) -> u64 {
        self.first + self.frames as u64
    }

    fn covers(&self, frame: u64) -> bool {
        self.first <= frame && frame < self.end()
    }
}

/// Editions remembered: an edition more than this many behind the newest
/// is all stale.
const CUTS: usize = 64;

/// What the callback tells the render ahead, and the render ahead the
/// callback — all atomics.
struct Control {
    /// The transport run the render ahead follows; the callback moves it on
    /// to start the render ahead again.
    generation: AtomicU64,
    /// Where that run starts (f64 bits), the device's block, how far into
    /// one it starts.
    restart_at: AtomicU64,
    block: AtomicUsize,
    into_block: AtomicUsize,
    /// The callback renders whole meanwhile, or renders ahead itself:
    /// render nothing.
    paused: AtomicBool,
    /// The render ahead has seen it paused and rests: its renderer is the
    /// callback's to use until it is told to go on.
    idle: AtomicBool,
    /// The frame of the run the render ahead goes on from, and the
    /// fingerprint of the tracks ahead the frames before it were rendered
    /// with (`has_print` false: none).
    restart_frame: AtomicU64,
    run_print: AtomicU64,
    has_print: AtomicBool,
    /// Frames of the run the callback has played.
    played: AtomicU64,
    /// The newest edition, and each edition's cut — the frame of the run it
    /// starts at (`cuts[e % CUTS]`). Edition 0 has none.
    edition: AtomicU64,
    cuts: [AtomicU64; CUTS],
    stop: AtomicBool,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            restart_at: AtomicU64::new(0),
            block: AtomicUsize::new(0),
            into_block: AtomicUsize::new(0),
            paused: AtomicBool::new(true),
            idle: AtomicBool::new(true),
            restart_frame: AtomicU64::new(0),
            run_print: AtomicU64::new(0),
            has_print: AtomicBool::new(false),
            played: AtomicU64::new(0),
            edition: AtomicU64::new(0),
            cuts: std::array::from_fn(|_| AtomicU64::new(0)),
            stop: AtomicBool::new(false),
        }
    }
}

impl Control {
    /// A new edition from frame `cut` of the run on: its cut first, then
    /// the edition, so a callback that sees the edition sees its cut.
    fn cut(&self, edition: u64, cut: u64) {
        self.cuts[edition as usize % CUTS].store(cut, Ordering::Relaxed);
        self.edition.store(edition, Ordering::Release);
    }
}

/// Frames into a run before a block short of the render ahead counts as
/// an underrun (about 85 ms): until then the first chunks are on their way.
const STARTUP: u64 = 4096;

/// Chunks there are at most: what the render ahead has in hand and the
/// callback holds, stale ones kept until passed or replaced.
const POOL: usize = 256;

/// The render ahead's thread: renders, hands over, follows the callback.
pub(crate) struct Ahead {
    control: Arc<Control>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Ahead {
    fn drop(&mut self) {
        self.control.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// A switch from one edition to another under way: from which, and the
/// frame of the run it began at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fade {
    from: u64,
    at: u64,
}

/// The frames a block plays from one chunk: which, and up to which frame
/// of the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pick {
    slot: usize,
    until: u64,
    stale: bool,
}

/// The callback's side: the chunks handed over, the window a live block
/// reads, and where in the run it is.
pub(crate) struct Player {
    control: Arc<Control>,
    filled: rtrb::Consumer<Box<Chunk>>,
    spent: rtrb::Producer<Box<Chunk>>,
    /// Every chunk of this run handed over and not yet passed.
    pending: Vec<Box<Chunk>>,
    window: Feed,
    generation: u64,
    /// Frames of the run played.
    at: u64,
    /// The block a run was started for (0: none running).
    block: usize,
    /// The newest edition, and up to which frame each edition is fresh
    /// (`valid[e % CUTS]`, for the [`CUTS`] newest).
    latest: u64,
    valid: [u64; CUTS],
    /// The edition the last frame played came from, and a switch from
    /// another under way.
    playing: Option<u64>,
    fade: Option<Fade>,
    fade_len: u64,
    /// The live set of the feed the last block played.
    print: Option<u64>,
    /// A run's start, rendered ahead here until the render ahead takes
    /// over; and the chunks kept for it, their buffers ready.
    prime: Option<Prime>,
    own: Vec<Box<Chunk>>,
    stats: Arc<EngineStats>,
}

/// A run's first blocks, the tracks ahead rendered by the callback itself:
/// the next frame of the run it renders ahead and where that is, the
/// fingerprint it rendered them with, blocks so far, and whether the last
/// one left time to render past its own block.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Prime {
    course: Course,
    produced: u64,
    print: Option<u64>,
    blocks: u32,
    extra: bool,
}

/// Chunks the callback keeps for a run's start.
const OWN: usize = 16;

/// The most frames a chunk the callback renders holds without allocating
/// (two device blocks of 2048), and live lanes.
const OWN_FRAMES: usize = 4096;
const OWN_LANES: usize = 16;

/// How far ahead the callback renders a run's start before the render
/// ahead takes over, device blocks: enough for its first chunk in time.
const HANDOVER: u64 = 3;

/// Blocks a run's start may take before the render ahead takes over
/// whatever it is given (about 170 ms at 128 frames).
const MAX_PRIME: u32 = 64;

/// What a block heard of the tracks ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Heard {
    /// The window holds them (or silence where there were none).
    Took(Took),
    /// What was rendered ahead is for somewhere else from this very
    /// block's start: nothing taken; a new run from here.
    Elsewhere,
}

/// The tracks a block's feed came from: all the chunks it took were of one
/// live set (`Some(print)`), or it took none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Took(Option<u64>);

impl Player {
    fn new(
        control: Arc<Control>,
        filled: rtrb::Consumer<Box<Chunk>>,
        spent: rtrb::Producer<Box<Chunk>>,
        fade_len: usize,
        stats: Arc<EngineStats>,
    ) -> Self {
        let mut window = Feed::default();
        window.reset(0, 0, 0);
        Self {
            control,
            filled,
            spent,
            pending: Vec::with_capacity(POOL),
            window,
            generation: 0,
            at: 0,
            block: 0,
            latest: 0,
            valid: [u64::MAX; CUTS],
            playing: None,
            fade: None,
            fade_len: fade_len.max(1) as u64,
            print: None,
            prime: None,
            own: (0..OWN)
                .map(|_| {
                    let mut chunk = Box::<Chunk>::default();
                    chunk.feed.reserve(OWN_FRAMES, OWN_LANES);
                    chunk
                })
                .collect(),
            stats,
        }
    }

    /// A new run at `at`, `into_block` frames into a device block of
    /// `block`: the render ahead stops, and the callback renders the run's
    /// start itself ([`Prime`]) until it hands over.
    fn begin_run(&mut self, at: f64, block: usize, into_block: usize) {
        self.recycle_all();
        self.generation = self.generation.wrapping_add(1);
        self.block = block;
        self.at = 0;
        self.playing = None;
        self.fade = None;
        self.print = None;
        self.prime = Some(Prime {
            course: Course {
                at,
                block,
                into_block,
            },
            produced: 0,
            print: None,
            blocks: 0,
            extra: true,
        });
        self.control.played.store(0, Ordering::Relaxed);
        self.control.paused.store(true, Ordering::SeqCst);
        self.stats.ahead_restarts.fetch_add(1, Ordering::Relaxed);
    }

    /// The render ahead goes on from where the run's start, rendered here,
    /// left off: the same frames, the same plugins, each heard once.
    fn hand_over(&mut self) {
        let Some(prime) = self.prime.take() else {
            return;
        };
        let c = &self.control;
        c.played.store(self.at, Ordering::Relaxed);
        c.restart_at
            .store(prime.course.at.to_bits(), Ordering::Relaxed);
        c.block.store(prime.course.block, Ordering::Relaxed);
        c.into_block
            .store(prime.course.into_block, Ordering::Relaxed);
        c.restart_frame.store(prime.produced, Ordering::Relaxed);
        c.run_print
            .store(prime.print.unwrap_or(0), Ordering::Relaxed);
        c.has_print.store(prime.print.is_some(), Ordering::Relaxed);
        c.generation.store(self.generation, Ordering::Release);
        c.paused.store(false, Ordering::SeqCst);
    }

    /// [`begin_run`](Self::begin_run) handed over at once: the render ahead
    /// renders the run from its first frame, and the first blocks find
    /// nothing until it has.
    fn restart(&mut self, at: f64, block: usize, into_block: usize) {
        self.begin_run(at, block, into_block);
        self.hand_over();
    }

    /// The callback renders whole for now: the render ahead rests, and its
    /// run is over.
    fn pause(&mut self) {
        if self.block != 0 {
            self.block = 0;
            self.prime = None;
            self.control.paused.store(true, Ordering::SeqCst);
            self.recycle_all();
        }
    }

    /// A chunk given back: kept for a run's start while the callback has
    /// too few, else to the render ahead.
    fn give_back(&mut self, chunk: Box<Chunk>) {
        if self.own.len() < OWN {
            self.own.push(chunk);
        } else {
            let _ = self.spent.push(chunk);
        }
    }

    fn recycle_all(&mut self) {
        while let Some(chunk) = self.pending.pop() {
            self.give_back(chunk);
        }
    }

    /// The chunks handed over since the last block: this run's kept, any
    /// other's given back.
    fn take_handed(&mut self) {
        while let Ok(chunk) = self.filled.pop() {
            if chunk.generation == self.generation && self.pending.len() < POOL {
                self.pending.push(chunk);
            } else {
                self.give_back(chunk);
            }
        }
    }

    /// Frames of the run's start played whole by the callback — every
    /// track, the tracks ahead too, or silence for them: the run goes on
    /// past them.
    fn skip_whole(&mut self, frames: usize, wrap: Option<Wrap>) {
        let end = self.at + frames as u64;
        if let Some(prime) = self.prime.as_mut() {
            if prime.produced < end {
                prime.course = prime.course.skip((end - prime.produced) as usize, wrap);
                prime.produced = end;
            }
            prime.blocks += 1;
        }
        self.at = end;
        self.control.played.store(end, Ordering::Relaxed);
    }

    /// The tracks ahead rendered here, by `ahead`, up to frame `to` of the
    /// run, into chunks of the callback's own; `print` the fingerprint they
    /// are rendered with.
    fn render_ahead(&mut self, ahead: &ProjectRenderer, to: u64, print: u64, wrap: Option<Wrap>) {
        let edition = self.control.edition.load(Ordering::Acquire);
        while let Some(prime) = self.prime.as_mut()
            && prime.produced < to
        {
            let Some(mut chunk) = self.own.pop() else {
                return;
            };
            let want = ((to - prime.produced) as usize).min(OWN_FRAMES);
            let (frames, next) = prime.course.next(want, wrap);
            let _ = ahead.render_part(prime.course.at, frames, 1.0, Part::Ahead(&mut chunk.feed));
            chunk.generation = self.generation;
            chunk.edition = edition;
            chunk.first = prime.produced;
            chunk.start = prime.course.at;
            chunk.frames = frames;
            prime.course = next;
            prime.produced += frames as u64;
            prime.print = Some(print);
            self.pending.push(chunk);
        }
    }

    /// The newest edition and how far each edition is fresh: up to the
    /// earliest cut of any edition after it.
    fn learn_cuts(&mut self) {
        let latest = self.control.edition.load(Ordering::Acquire);
        if latest == self.latest && latest != 0 {
            return;
        }
        self.latest = latest;
        let mut until = u64::MAX;
        let oldest = latest.saturating_sub(CUTS as u64 - 1);
        let mut e = latest;
        loop {
            self.valid[e as usize % CUTS] = until;
            if e == oldest {
                break;
            }
            let cut = self.control.cuts[e as usize % CUTS].load(Ordering::Relaxed);
            until = until.min(cut);
            e -= 1;
        }
    }

    /// Up to which frame of the run `edition` is fresh.
    fn fresh_until(&self, edition: u64) -> u64 {
        if self.latest - edition.min(self.latest) >= CUTS as u64 {
            0
        } else {
            self.valid[edition as usize % CUTS]
        }
    }

    /// Where frame `frame` of the run is played from: the freshest chunk
    /// that has it fresh, or — the newer edition not here yet — the newest
    /// that has it at all; until the chunk ends, its edition goes stale or
    /// a fresher one starts.
    fn pick(&self, frame: u64) -> Option<Pick> {
        let mut fresh: Option<(u64, usize)> = None;
        let mut stale: Option<(u64, usize)> = None;
        for (slot, c) in self.pending.iter().enumerate() {
            if !c.covers(frame) {
                continue;
            }
            let best = if frame < self.fresh_until(c.edition) {
                &mut fresh
            } else {
                &mut stale
            };
            if (*best).is_none_or(|(e, _)| c.edition > e) {
                *best = Some((c.edition, slot));
            }
        }
        let ((edition, slot), is_stale) = match (fresh, stale) {
            (Some(f), _) => (f, false),
            (None, Some(s)) => (s, true),
            (None, None) => return None,
        };
        let mut until = self.pending[slot].end();
        if !is_stale {
            until = until.min(self.fresh_until(edition));
        }
        for c in &self.pending {
            if c.edition > edition
                && c.first > frame
                && c.first < until
                && c.first < self.fresh_until(c.edition)
            {
                until = c.first;
            }
        }
        Some(Pick {
            slot,
            until,
            stale: is_stale,
        })
    }

    /// The feed for a live block of `frames` at `start`: the frames
    /// rendered ahead for it, freshest first, silence where there are none.
    fn window(&mut self, start: f64, frames: usize) -> Heard {
        if self.block != frames {
            self.restart(start, frames, 0);
        }
        self.take_handed();
        self.learn_cuts();
        self.window.clear_window();
        let end = self.at + frames as u64;
        let mut frame = self.at;
        let mut print = None;
        while frame < end {
            let Some(pick) = self.pick(frame) else {
                // A run's first blocks find nothing yet: that is its start,
                // not a dropout.
                if self.at >= STARTUP {
                    self.stats.ahead_underruns.fetch_add(1, Ordering::Relaxed);
                }
                break;
            };
            let (into, its_start, its_print, edition) = {
                let c = &self.pending[pick.slot];
                let into = (frame - c.first) as usize;
                (into, c.start + into as f64, c.feed.print(), c.edition)
            };
            let expected = start + (frame - self.at) as f64;
            let elsewhere = (its_start - expected).abs() >= SAME_FRAME;
            let other_set = print.is_some_and(|p| p != its_print);
            if elsewhere || other_set {
                // On another course (a locate, a loop not foreseen), or of
                // another live set from here on: a new run from here.
                if frame == self.at {
                    return Heard::Elsewhere;
                }
                let rest = (end - frame) as usize;
                self.begin_run(expected, frames, frames - rest);
                self.skip_whole(rest, None);
                return Heard::Took(self.finish(frames, print));
            }
            print = Some(its_print);
            let take = (pick.until.min(end) - frame) as usize;
            if pick.stale {
                self.stats
                    .ahead_stale_frames
                    .fetch_add(take as u64, Ordering::Relaxed);
            }
            if let Some(from) = self.playing.filter(|e| *e != edition) {
                self.fade = Some(Fade { from, at: frame });
            }
            self.playing = Some(edition);
            let at = self.window.frames();
            self.window
                .append(&self.pending[pick.slot].feed, into, take);
            self.fade_in(frame, at, take);
            frame += take as u64;
        }
        self.at = end;
        let took = self.finish(frames, print);
        self.recycle_passed();
        Heard::Took(took)
    }

    /// The block's window padded to `frames` with silence, the frames
    /// played told to the render ahead.
    fn finish(&mut self, frames: usize, print: Option<u64>) -> Took {
        let short = frames.saturating_sub(self.window.frames());
        if short > 0 {
            self.window.append_silence(short);
        }
        self.window.print = print.unwrap_or(0);
        if print.is_some() {
            self.print = print;
        }
        self.control.played.store(self.at, Ordering::Relaxed);
        Took(print)
    }

    /// Frames `frame..frame + frames` of the run, at `at` in the window,
    /// faded in over the edition a switch is from, while the switch lasts.
    fn fade_in(&mut self, frame: u64, at: usize, frames: usize) {
        let Some(fade) = self.fade else {
            return;
        };
        let fade_end = fade.at + self.fade_len;
        let end = (frame + frames as u64).min(fade_end);
        let mut k = frame;
        while k < end {
            let Some(old) = self
                .pending
                .iter()
                .find(|c| c.edition == fade.from && c.covers(k))
            else {
                // Nothing of it rendered this far: the switch is a jump.
                break;
            };
            let n = (old.end().min(end) - k) as usize;
            let (len, began) = (self.fade_len as f32, fade.at);
            self.window.fade_from(
                at + (k - frame) as usize,
                &old.feed,
                (k - old.first) as usize,
                n,
                |j| ((k + j as u64 - began + 1) as f32 / (len + 1.0)).min(1.0),
            );
            k += n as u64;
        }
        if frame + frames as u64 >= fade_end {
            self.fade = None;
        }
    }

    /// The chunks no frame to come will be played from given back: those
    /// behind the playhead, and stale ones a fresher edition covers — but
    /// never the edition playing or one a switch is fading from.
    fn recycle_passed(&mut self) {
        let mut slot = 0;
        while slot < self.pending.len() {
            let c = &self.pending[slot];
            let held =
                Some(c.edition) == self.playing || self.fade.is_some_and(|f| f.from == c.edition);
            let passed = c.end() <= self.at;
            let replaced = !held
                && c.first >= self.fresh_until(c.edition)
                && self.pending.iter().any(|o| {
                    o.edition > c.edition
                        && o.covers(c.first)
                        && c.first < self.fresh_until(o.edition)
                });
            if passed || replaced {
                let chunk = self.pending.swap_remove(slot);
                self.give_back(chunk);
            } else {
                slot += 1;
            }
        }
    }
}

/// A project's render for the audio callback, ahead and live: what the
/// callback calls in place of `ProjectRenderer::render_plan`.
pub struct DualRender {
    live: Arc<ProjectRenderer>,
    /// The render ahead's renderer: the thread's, and the callback's while
    /// it renders a run's start.
    ahead: Option<Arc<ProjectRenderer>>,
    shared: Option<Arc<TransportShared>>,
    player: Option<Player>,
    _ahead: Option<Ahead>,
}

impl DualRender {
    /// Everything rendered in the callback, as before.
    #[must_use]
    pub fn whole(live: Arc<ProjectRenderer>) -> Self {
        Self {
            live,
            ahead: None,
            shared: None,
            player: None,
            _ahead: None,
        }
    }

    /// A render ahead beside `live` (a realtime renderer for the audio
    /// callback): its own renderer of the same song on a thread of its own.
    /// `None` prefs: [`whole`](Self::whole).
    #[must_use]
    pub fn new(
        live: Arc<ProjectRenderer>,
        shared: Arc<TransportShared>,
        prefs: Option<AheadPrefs>,
        stats: Arc<EngineStats>,
    ) -> Self {
        let Some(prefs) = prefs else {
            return Self::whole(live);
        };
        let sample_rate = shared.sample_rate();
        let ahead = Arc::new(ProjectRenderer::new(
            live.daw(),
            live.project_guid(),
            sample_rate,
        ));
        ahead.set_realtime_ahead();
        // Which live set a block is of is known before it is rendered.
        live.hold();
        // Live MIDI for a track rendered ahead goes on to its renderer.
        let (to_ahead, from_live) = rtrb::RingBuffer::new(1024);
        live.set_midi_forward(to_ahead);
        ahead.set_live_midi(from_live);

        let control = Arc::new(Control::default());
        let (filled_tx, filled_rx) = rtrb::RingBuffer::new(POOL);
        let (spent_tx, spent_rx) = rtrb::RingBuffer::new(POOL);
        let frames = |ms| AheadPrefs::frames(ms, sample_rate);
        let depth = frames(prefs.depth_ms).max(1);
        let worker = Worker {
            control: control.clone(),
            renderer: ahead.clone(),
            shared: shared.clone(),
            filled: filled_tx,
            spent: spent_rx,
            depth,
            edit_depth: frames(prefs.edit_depth_ms).clamp(1, depth),
            guard: frames(prefs.guard_ms),
            quiet: Duration::from_secs_f64(prefs.quiet_ms.max(0.0) / 1000.0),
            chunk: prefs.chunk.max(1),
            stats: stats.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("render-ahead".into())
            .spawn(move || worker.run())
            .ok();
        if thread.is_none() {
            tracing::warn!("render ahead: no thread; every track renders in the callback");
            return Self::whole(live);
        }
        Self {
            live,
            ahead: Some(ahead),
            shared: Some(shared),
            player: Some(Player::new(
                control.clone(),
                filled_rx,
                spent_tx,
                frames(prefs.fade_ms),
                stats,
            )),
            _ahead: Some(Ahead { control, thread }),
        }
    }

    /// Nothing is played (stopped): the render ahead rests until play.
    pub fn rest(&mut self) {
        if let Some(player) = self.player.as_mut() {
            player.pause();
        }
    }

    /// The renderer the callback renders the live tracks with.
    #[must_use]
    pub fn live(&self) -> &Arc<ProjectRenderer> {
        &self.live
    }

    /// Render one audio buffer as the transport planned it: the live tracks
    /// from the feed rendered ahead when the plan is plain playback, the
    /// whole song otherwise (see `ProjectRenderer::render_plan` for
    /// `render_stopped`).
    pub fn render(&mut self, plan: &BlockPlan, render_stopped: bool) -> StereoBuffer {
        let Some(player) = self.player.as_mut() else {
            return self.live.render_plan(plan, render_stopped);
        };
        // The live renderer's snapshot for this block, and its live set.
        let live_set = self.live.refresh().map(|p| p.live);
        let [seg] = plan.segments() else {
            player.pause();
            return self.live.render_plan(plan, render_stopped);
        };
        if !(seg.playing && seg.rate == 1.0 && seg.frames > 0) {
            player.pause();
            return self.live.render_plan(plan, render_stopped);
        }
        let (start, frames) = (seg.start_samples, seg.frames);
        // A run starts with play, a new block size, or a changed live set:
        // what was rendered ahead was for other buses.
        let changed_set = player.print.is_some() && player.print != live_set;
        if player.block != frames || changed_set {
            player.begin_run(start, frames, 0);
        }
        if player.prime.is_none() {
            match player.window(start, frames) {
                Heard::Took(_) => {
                    return self
                        .live
                        .render_part(start, frames, 1.0, Part::Live(&player.window));
                }
                // A locate, a loop the render ahead did not foresee.
                Heard::Elsewhere => player.begin_run(start, frames, 0),
            }
        }
        self.prime(start, frames, live_set)
    }

    /// A block of a run's start: the tracks ahead rendered here, by the
    /// render ahead's renderer while its thread rests, this block and — if
    /// the last left time — the next; handed over once a few blocks are
    /// ready. Until that renderer can be used (its thread finishing a chunk,
    /// its snapshot not yet of this live set) every track is rendered here,
    /// as before the render ahead: each frame of each plugin heard once, in
    /// order, and never a block of silence.
    fn prime(&mut self, start: f64, frames: usize, live_set: Option<u64>) -> StereoBuffer {
        let wrap = self.shared.as_deref().and_then(wrap_of);
        let budget = self
            .shared
            .as_deref()
            .map_or(0.0, |s| frames as f64 / f64::from(s.sample_rate()));
        let began = Instant::now();
        let from_ahead = {
            let (Some(player), Some(ahead)) = (self.player.as_mut(), self.ahead.as_ref()) else {
                return self.live.render_part(start, frames, 1.0, Part::Whole);
            };
            let at = player.at;
            let ready = player.control.idle.load(Ordering::SeqCst) && !player.own.is_empty();
            let prints = if ready { ahead.refresh() } else { None };
            match prints.filter(|p| live_set == Some(p.live)) {
                Some(prints) => {
                    let extra = player.prime.is_some_and(|p| p.extra);
                    let to = at + frames as u64 * if extra { 2 } else { 1 };
                    player.render_ahead(ahead, to, prints.ahead, wrap);
                    true
                }
                // Ahead of the playhead already, from blocks before: play
                // that.
                None => player.prime.map_or(0, |p| p.produced.saturating_sub(at)) >= frames as u64,
            }
        };
        let out = if from_ahead {
            self.live_from_ahead(start, frames)
        } else {
            let out = self.live.render_part(start, frames, 1.0, Part::Whole);
            if let Some(player) = self.player.as_mut() {
                player.skip_whole(frames, wrap);
            }
            out
        };
        let Some(player) = self.player.as_mut() else {
            return out;
        };
        if let Some(prime) = player.prime.as_mut() {
            // Room for the next block's next block, if this one left half
            // its time.
            prime.extra = began.elapsed().as_secs_f64() < budget * 0.5;
            let ahead_by = prime.produced.saturating_sub(player.at);
            let ready = ahead_by >= HANDOVER * frames as u64 && prime.print.is_some();
            if ready || prime.blocks >= MAX_PRIME {
                player.hand_over();
            }
        }
        out
    }

    /// The live tracks rendered here over what was rendered ahead for this
    /// block.
    fn live_from_ahead(&mut self, start: f64, frames: usize) -> StereoBuffer {
        let Some(player) = self.player.as_mut() else {
            return self.live.render_part(start, frames, 1.0, Part::Whole);
        };
        if let Some(prime) = player.prime.as_mut() {
            prime.blocks += 1;
        }
        match player.window(start, frames) {
            Heard::Took(_) => self
                .live
                .render_part(start, frames, 1.0, Part::Live(&player.window)),
            Heard::Elsewhere => {
                // What the callback rendered ahead is for its own course, so
                // this is a transport that moved meanwhile: start again.
                player.begin_run(start, frames, 0);
                let out = self.live.render_part(start, frames, 1.0, Part::Whole);
                player.skip_whole(frames, self.shared.as_deref().and_then(wrap_of));
                out
            }
        }
    }
}

/// The loop the transport wraps at now, if it loops.
fn wrap_of(shared: &TransportShared) -> Option<Wrap> {
    if !shared.is_looping() {
        return None;
    }
    shared.loop_region().map(|r| Wrap {
        start: r.start.0 as f64,
        end: r.end.0 as f64,
    })
}

/// The render ahead's thread.
struct Worker {
    control: Arc<Control>,
    renderer: Arc<ProjectRenderer>,
    shared: Arc<TransportShared>,
    filled: rtrb::Producer<Box<Chunk>>,
    spent: rtrb::Consumer<Box<Chunk>>,
    depth: usize,
    edit_depth: usize,
    guard: usize,
    quiet: Duration,
    chunk: usize,
    stats: Arc<EngineStats>,
}

/// How long the render ahead rests when it is far enough ahead, or paused.
const REST: Duration = Duration::from_millis(1);

/// The render ahead's place in a run: the next frame it renders, where
/// that is, and where each chunk still ahead of the playhead began — so an
/// edit can take it back to any frame of them.
struct Place {
    produced: u64,
    course: Course,
    marks: VecDeque<(u64, Course)>,
}

impl Place {
    fn new(course: Course) -> Self {
        Self {
            produced: 0,
            course,
            marks: VecDeque::with_capacity(POOL * 4),
        }
    }

    /// A chunk of `frames` rendered from here: on past it.
    fn advance(&mut self, frames: usize, next: Course) {
        self.marks.push_back((self.produced, self.course));
        self.course = next;
        self.produced += frames as u64;
    }

    /// On to frame `frame` of the run, which the callback has reached: what
    /// would come before it is already past.
    fn catch_up(&mut self, frame: u64, wrap: Option<Wrap>) {
        if frame > self.produced {
            self.course = self.course.skip((frame - self.produced) as usize, wrap);
            self.produced = frame;
        }
        // Marks behind the playhead are of no use but the last.
        while self.marks.len() >= 2 && self.marks[1].0 <= frame {
            self.marks.pop_front();
        }
    }

    /// Back to frame `frame` of the run, rendered before: the course from
    /// the chunk it fell in.
    fn rewind(&mut self, frame: u64, wrap: Option<Wrap>) {
        if frame >= self.produced {
            return;
        }
        let Some(&(from, course)) = self.marks.iter().rev().find(|(f, _)| *f <= frame) else {
            return;
        };
        self.course = course.skip((frame - from) as usize, wrap);
        self.produced = frame;
        while self.marks.back().is_some_and(|(f, _)| *f >= frame) {
            self.marks.pop_back();
        }
    }
}

impl Worker {
    fn run(mut self) {
        let _ = audio::io::realtime::promote_current_thread_at(audio::io::realtime::AHEAD_PRIORITY);
        let mut spare: Vec<Box<Chunk>> = Vec::new();
        let mut made = 0usize;
        let mut generation = u64::MAX;
        let mut place = Place::new(Course {
            at: 0.0,
            block: 1,
            into_block: 0,
        });
        let mut edition = self.control.edition.load(Ordering::Relaxed);
        let mut ahead_print: Option<u64> = None;
        let mut edited: Option<Instant> = None;
        loop {
            if self.control.stop.load(Ordering::Acquire) {
                return;
            }
            while let Ok(chunk) = self.spent.pop() {
                spare.push(chunk);
            }
            if self.control.paused.load(Ordering::SeqCst) {
                // Resting: the renderer is the callback's meanwhile.
                self.control.idle.store(true, Ordering::SeqCst);
                self.stats.ahead_fill_frames.store(0, Ordering::Relaxed);
                std::thread::park_timeout(REST);
                continue;
            }
            self.control.idle.store(false, Ordering::SeqCst);
            // Told to rest between the look and the word: rest.
            if self.control.paused.load(Ordering::SeqCst) {
                continue;
            }
            let now = self.control.generation.load(Ordering::Acquire);
            if now != generation {
                // A new run, its start rendered by the callback up to here.
                generation = now;
                place = Place::new(Course {
                    at: f64::from_bits(self.control.restart_at.load(Ordering::Relaxed)),
                    block: self.control.block.load(Ordering::Relaxed).max(1),
                    into_block: self.control.into_block.load(Ordering::Relaxed),
                });
                place.produced = self.control.restart_frame.load(Ordering::Relaxed);
                edition = self.control.edition.load(Ordering::Relaxed);
                if self.control.has_print.load(Ordering::Relaxed) {
                    ahead_print = Some(self.control.run_print.load(Ordering::Relaxed));
                }
            }
            let played = self.control.played.load(Ordering::Relaxed);
            place.catch_up(played, self.wrap());

            // An edit to what is rendered ahead: what was rendered past
            // the guard is stale — a new edition from there.
            let fresh = self.renderer.refresh().map(|p: Prints| p.ahead);
            if let (Some(was), Some(now)) = (ahead_print, fresh)
                && was != now
            {
                edited = Some(Instant::now());
                let cut = played + (place.course.block + self.guard) as u64;
                if cut < place.produced {
                    edition += 1;
                    self.control.cut(edition, cut);
                    place.rewind(cut, self.wrap());
                    self.stats.ahead_rewinds.fetch_add(1, Ordering::Relaxed);
                }
            }
            ahead_print = fresh.or(ahead_print);

            // Shallow while edits come in: each throws away little.
            let depth = match edited {
                Some(at) if at.elapsed() < self.quiet => self.edit_depth,
                _ => self.depth,
            };
            self.stats
                .ahead_depth_frames
                .store(depth as u64, Ordering::Relaxed);
            let ahead_by = place.produced.saturating_sub(played) as usize;
            self.stats
                .ahead_fill_frames
                .store(ahead_by as u64, Ordering::Relaxed);
            if ahead_by >= depth || self.filled.is_full() {
                std::thread::park_timeout(REST);
                continue;
            }
            let Some(mut chunk) = spare.pop().or_else(|| {
                (made < POOL).then(|| {
                    made += 1;
                    Box::<Chunk>::default()
                })
            }) else {
                std::thread::park_timeout(REST);
                continue;
            };
            // Small chunks while little is ready — the frames nearest the
            // playhead first —, big ones once there is slack.
            let want = chunk_size(ahead_by, place.course.block, self.chunk, depth);
            let (frames, next) = place.course.next(want, self.wrap());
            let started = Instant::now();
            let _ = self.renderer.render_part(
                place.course.at,
                frames,
                1.0,
                Part::Ahead(&mut chunk.feed),
            );
            self.stats
                .ahead_render_ns
                .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
            self.stats
                .ahead_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
            if self.control.generation.load(Ordering::Acquire) != generation {
                spare.push(chunk);
                continue;
            }
            chunk.generation = generation;
            chunk.edition = edition;
            chunk.first = place.produced;
            chunk.start = place.course.at;
            chunk.frames = frames;
            if let Err(rtrb::PushError::Full(chunk)) = self.filled.push(chunk) {
                spare.push(chunk);
                continue;
            }
            place.advance(frames, next);
        }
    }

    /// The loop the transport wraps at now, if it loops.
    fn wrap(&self) -> Option<Wrap> {
        wrap_of(&self.shared)
    }
}

/// How much to render next with `ahead_by` frames ready: one device block
/// while there is less than two, doubling from there up to `chunk`, never
/// past the depth.
fn chunk_size(ahead_by: usize, block: usize, chunk: usize, depth: usize) -> usize {
    let block = block.max(1);
    let grown = if ahead_by < 2 * block {
        block
    } else {
        ahead_by.next_power_of_two().min(chunk)
    };
    grown.min(depth.saturating_sub(ahead_by).max(block)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::Standalone;

    fn course(at: f64, block: usize) -> Course {
        Course {
            at,
            block,
            into_block: 0,
        }
    }

    /// With no loop a chunk is as long as asked, and the course runs on.
    #[test]
    fn without_a_loop_the_course_runs_straight() {
        let (frames, next) = course(1000.0, 128).next(1024, None);
        assert_eq!(frames, 1024);
        assert_eq!(next.at, 2024.0);
        assert_eq!(next.into_block, 0);
        let (frames, next) = course(0.0, 128).next(100, None);
        assert_eq!((frames, next.into_block), (100, 100));
    }

    /// The block that reaches the loop's end plays to its own end, past
    /// the loop; the course then starts where the transport wraps to — what
    /// `TransportShared::advanced` does block by block.
    #[test]
    fn a_loop_wraps_at_the_end_of_the_block_that_reaches_it() {
        let wrap = Some(Wrap {
            start: 1000.0,
            end: 2000.0,
        });
        // Blocks from 1000: 1000, 1128, …, 1896 (ends 2024 ≥ 2000).
        let (frames, next) = course(1000.0, 128).next(4096, wrap);
        assert_eq!(frames, 1024);
        assert_eq!(next.at, 1024.0, "2024 wraps to 1000 + 24");
        assert_eq!(next.into_block, 0);
        // A chunk that stops short of that block is not wrapped.
        let (frames, next) = course(1000.0, 128).next(512, wrap);
        assert_eq!((frames, next.at), (512, 1512.0));
    }

    /// A course already past the loop's end wraps at its very next block
    /// end, as the transport does.
    #[test]
    fn past_the_loop_the_next_block_end_wraps() {
        let wrap = Some(Wrap {
            start: 0.0,
            end: 500.0,
        });
        let start = Course {
            at: 600.0,
            block: 128,
            into_block: 28,
        };
        let (frames, next) = start.next(1024, wrap);
        assert_eq!(frames, 100);
        assert_eq!(next.at, 200.0, "700 wraps to 200");
    }

    /// Taken back to a frame it rendered, the render ahead is where the
    /// course was then — across a loop's wrap too.
    #[test]
    fn a_place_rewinds_to_any_frame_still_ahead() {
        let wrap = Some(Wrap {
            start: 0.0,
            end: 1000.0,
        });
        let mut place = Place::new(course(512.0, 128));
        for _ in 0..4 {
            let (frames, next) = place.course.next(256, wrap);
            place.advance(frames, next);
        }
        assert_eq!(place.produced, 1024);
        let ahead = place.course;
        place.rewind(600, wrap);
        assert_eq!(place.produced, 600);
        assert_eq!(
            place.course.at, 112.0,
            "the block ending 1024 wraps to 24; 88 on"
        );
        assert_eq!(
            place.course.skip(424, wrap),
            ahead,
            "and on to where it was"
        );
    }

    #[test]
    fn chunks_start_small_and_grow() {
        assert_eq!(chunk_size(0, 128, 1024, 9600), 128);
        assert_eq!(chunk_size(300, 128, 1024, 9600), 512);
        assert_eq!(chunk_size(5000, 128, 1024, 9600), 1024);
        assert_eq!(chunk_size(9500, 128, 1024, 9600), 128);
    }

    fn player(
        stats: &Arc<EngineStats>,
        fade_len: usize,
    ) -> (Player, rtrb::Producer<Box<Chunk>>, Arc<Control>) {
        let (tx, rx) = rtrb::RingBuffer::new(POOL);
        let (spent_tx, _spent_rx) = rtrb::RingBuffer::new(POOL);
        let control = Arc::new(Control::default());
        let p = Player::new(control.clone(), rx, spent_tx, fade_len, stats.clone());
        (p, tx, control)
    }

    fn chunk(generation: u64, edition: u64, first: u64, frames: usize, value: f32) -> Box<Chunk> {
        let mut feed = Feed::default();
        feed.reset(frames, 5, 0);
        feed.master.fill(value);
        Box::new(Chunk {
            generation,
            edition,
            first,
            start: first as f64,
            frames,
            feed,
        })
    }

    /// The left channel of `blocks` blocks of 128 played from block
    /// `from_block`.
    fn heard(p: &mut Player, from_block: usize, blocks: usize) -> Vec<f32> {
        (from_block..from_block + blocks)
            .flat_map(|b| {
                p.window((b * 128) as f64, 128);
                p.window
                    .master
                    .iter()
                    .step_by(2)
                    .copied()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The callback plays the frames rendered for where it is, across
    /// chunks; the first block of a run, with nothing ready, is silent and
    /// not counted against the render ahead.
    #[test]
    fn a_block_plays_the_chunks_for_where_it_is() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx, _) = player(&stats, 4);
        assert_eq!(
            p.window(0.0, 4),
            Heard::Took(Took(None)),
            "nothing ready yet"
        );
        assert_eq!(stats.ahead_underruns.load(Ordering::Relaxed), 0);
        assert_eq!(p.window.master, vec![0.0; 8]);
        let generation = p.generation;
        let _ = tx.push(chunk(generation, 0, 4, 6, 1.0));
        let _ = tx.push(chunk(generation, 0, 10, 6, 2.0));
        assert_eq!(p.window(4.0, 4), Heard::Took(Took(Some(5))));
        assert_eq!(p.window.master, vec![1.0; 8]);
        assert_eq!(p.window(8.0, 4), Heard::Took(Took(Some(5))));
        assert_eq!(
            p.window.master,
            vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]
        );
        assert_eq!(
            p.pending.len(),
            1,
            "the first chunk is passed and given back"
        );
    }

    /// A chunk of an older run is passed over; one for another place on
    /// the timeline from the block's start says so, taking nothing; from
    /// within the block, it starts a new run from that frame.
    #[test]
    fn a_chunk_for_elsewhere_starts_a_new_run() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx, _) = player(&stats, 4);
        let _ = p.window(0.0, 4);
        let generation = p.generation;
        let _ = tx.push(chunk(generation.wrapping_sub(1), 0, 4, 4, 9.0));
        let mut far = chunk(generation, 0, 4, 4, 7.0);
        far.start = 50_000.0;
        let _ = tx.push(far);
        assert_eq!(p.window(4.0, 4), Heard::Elsewhere);
        assert_eq!(p.generation, generation, "the caller starts it");

        let (mut p, mut tx, _) = player(&stats, 4);
        let _ = p.window(0.0, 4);
        let generation = p.generation;
        let _ = tx.push(chunk(generation, 0, 4, 2, 1.0));
        let mut far = chunk(generation, 0, 6, 2, 7.0);
        far.start = 50_000.0;
        let _ = tx.push(far);
        assert_eq!(p.window(4.0, 4), Heard::Took(Took(Some(5))));
        assert_eq!(
            p.window.master,
            vec![1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_ne!(p.generation, generation, "a new run from frame 6");
        let prime = p.prime.expect("its start rendered here");
        assert_eq!((prime.course.at, prime.produced, p.at), (8.0, 2, 2));
    }

    /// After an edit the new edition takes over at its cut, faded in over
    /// the old one; what came before the cut is the old one's.
    #[test]
    fn an_edition_takes_over_at_its_cut_with_a_fade() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx, control) = player(&stats, 4);
        p.restart(0.0, 128, 0);
        let g = p.generation;
        let _ = tx.push(chunk(g, 0, 0, 1024, 1.0));
        control.cut(1, 512);
        let _ = tx.push(chunk(g, 1, 512, 512, 3.0));
        let left = heard(&mut p, 0, 8);
        assert!(left[..512].iter().all(|s| *s == 1.0), "before the cut");
        let fade: Vec<f32> = left[512..517]
            .iter()
            .map(|s| (s * 10.0).round() / 10.0)
            .collect();
        assert_eq!(fade, vec![1.4, 1.8, 2.2, 2.6, 3.0], "linear over the fade");
        assert!(left[517..].iter().all(|s| *s == 3.0), "the new edition");
        assert_eq!(stats.ahead_stale_frames.load(Ordering::Relaxed), 0);
    }

    /// An edit whose new frames are not here yet: the old ones play on
    /// meanwhile (the edit late, not a gap), and the new take over where
    /// they arrive.
    #[test]
    fn a_late_edition_plays_the_old_one_meanwhile() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx, control) = player(&stats, 4);
        p.restart(0.0, 128, 0);
        let g = p.generation;
        let _ = tx.push(chunk(g, 0, 0, 1024, 1.0));
        control.cut(1, 256);
        let early = heard(&mut p, 0, 3);
        assert!(early.iter().all(|s| *s == 1.0), "no gap: {early:?}");
        assert_eq!(stats.ahead_stale_frames.load(Ordering::Relaxed), 128);
        let _ = tx.push(chunk(g, 1, 256, 512, 3.0));
        let later = heard(&mut p, 3, 2);
        assert!(
            (later[0] - 1.4).abs() < 1e-6,
            "faded in where the new arrived"
        );
        assert!(later[4..].iter().all(|s| *s == 3.0));
        assert_eq!(stats.ahead_underruns.load(Ordering::Relaxed), 0);
    }

    /// Stale chunks a newer edition covers are given back, and so is
    /// everything behind the playhead.
    #[test]
    fn replaced_and_passed_chunks_are_given_back() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx, control) = player(&stats, 4);
        p.restart(0.0, 128, 0);
        let g = p.generation;
        let _ = tx.push(chunk(g, 0, 0, 256, 1.0));
        let _ = tx.push(chunk(g, 0, 256, 256, 1.0));
        let _ = tx.push(chunk(g, 0, 512, 256, 1.0));
        control.cut(1, 256);
        let _ = tx.push(chunk(g, 1, 256, 512, 3.0));
        let _ = heard(&mut p, 0, 3);
        assert!(
            p.pending.iter().all(|c| c.first != 512),
            "the replaced chunk is gone"
        );
        let _ = heard(&mut p, 3, 3);
        assert!(p.pending.is_empty(), "all passed");
    }

    /// What the dual render played, and its counters.
    struct Played {
        out: Vec<f32>,
        stats: Arc<EngineStats>,
    }

    impl Played {
        fn count(&self, of: impl Fn(&EngineStats) -> &AtomicU64) -> u64 {
            of(&self.stats).load(Ordering::Relaxed)
        }
    }

    /// What a test does to the song between blocks: the song, and its
    /// transport.
    struct Song<'a> {
        daw: &'a Standalone,
        guid: &'a str,
        transport: &'a TransportShared,
    }

    /// The song as the transport plays it, block by block — paced like a
    /// device, so the render ahead keeps up: rendered whole in the callback
    /// (`prefs` `None`), or with a render ahead. `looped` loops the
    /// transport over that span; `edit` is called before each block with
    /// the song and the block's number.
    fn play(
        prefs: Option<AheadPrefs>,
        looped: Option<(i64, i64)>,
        blocks: usize,
        mut edit: impl FnMut(&Song<'_>, usize),
    ) -> Played {
        use crate::transport_engine::{InstantSamples, LoopRegionSamples, PlayStateRepr};
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = crate::audio_engine::render::folder_routing_tests::split_song(None);
        // Every take a second long, and moving, so a frame out of place
        // shows. (Its tone by its track's name: the two songs' maps
        // iterate in orders of their own.)
        let takes: Vec<(String, usize)> = daw
            .read_project(&guid, |p| {
                p.takes
                    .iter()
                    .flat_map(|(item, tl)| {
                        let track = p.items.get(item).map(|e| e.item.track_guid.clone());
                        let name = track
                            .and_then(|g| p.tracks.iter().find(|t| t.guid == g))
                            .map(|t| t.name.len() * 7 + t.name.as_bytes()[0] as usize)
                            .unwrap_or(1);
                        tl.takes.iter().map(move |t| (t.guid.clone(), name))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (take, k) in &takes {
            let samples = (0..48_000)
                .map(|i| ((i as f32) * 0.0003 * *k as f32).sin() * 0.3)
                .collect();
            crate::audio_engine::materialize::attach_audio_source(
                &daw,
                &guid,
                take,
                crate::audio_engine::decoder::DecodedAudio::new(samples, 1, 48_000),
            );
        }
        let shared = Arc::new(TransportShared::new(48_000, 120.0));
        if let Some((start, end)) = looped {
            shared.set_looping(true);
            shared.set_loop_region(Some(LoopRegionSamples {
                start: InstantSamples(start),
                end: InstantSamples(end),
            }));
        }
        shared.set_play_state(PlayStateRepr::Playing);
        let live = Arc::new(ProjectRenderer::new(&daw, &guid, 48_000));
        live.set_realtime();
        let stats = Arc::new(EngineStats::default());
        let mut render = DualRender::new(live, shared.clone(), prefs, stats.clone());
        let mut out = Vec::new();
        let period = Duration::from_secs_f64(128.0 / 48_000.0);
        let began = Instant::now();
        for b in 0..blocks {
            let song = Song {
                daw: &daw,
                guid: &guid,
                transport: &shared,
            };
            edit(&song, b);
            let stamp = b as f64 * period.as_secs_f64() * 1e6;
            let plan = shared.begin_block(128, stamp, period.as_secs_f64() * 1e6);
            out.extend(render.render(&plan, false).samples);
            // Like a device: the next block comes a block's time later.
            if let Some(rest) = (period * (b as u32 + 1)).checked_sub(began.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        Played { out, stats }
    }

    fn no_edit(_: &Song<'_>, _: usize) {}

    fn track(daw: &Standalone, guid: &str, name: &str) -> daw_proto::TrackRef {
        daw_proto::TrackRef::Guid(crate::audio_engine::render::folder_routing_tests::guid_of(
            daw, guid, name,
        ))
    }

    fn set_volume(daw: &Standalone, guid: &str, name: &str, volume: f64) {
        use daw_proto::Tracks;
        let ctx = daw_proto::ProjectContext::Project(guid.to_string());
        Tracks::set_volume(daw, ctx, track(daw, guid, name), volume).expect("the fader");
    }

    fn assert_same_from(a: &[f32], b: &[f32], from_block: usize) {
        let from = from_block * 128 * 2;
        let worst = a[from..]
            .iter()
            .zip(&b[from..])
            .map(|(x, y)| (x - y).abs())
            .fold(0.0_f32, f32::max);
        let heard = a[from..].iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(heard > 0.01, "the song is heard");
        let first = a
            .iter()
            .zip(b)
            .skip(from)
            .position(|(x, y)| (x - y).abs() > 1e-6)
            .map(|i| i + from)
            .map(|i| (i / 256, (i % 256) / 2, a[i], b[i]));
        assert!(
            worst < 1e-6,
            "the dual render plays the song: worst {worst}, first (block, frame, whole, dual) {first:?}"
        );
    }

    /// The first block from `from` where `a` and `b` part.
    fn first_difference(a: &[f32], b: &[f32], from: usize) -> Option<usize> {
        (from..a.len() / 256).find(|blk| {
            a[blk * 256..(blk + 1) * 256]
                .iter()
                .zip(&b[blk * 256..(blk + 1) * 256])
                .any(|(x, y)| (x - y).abs() > 1e-4)
        })
    }

    /// Once the render ahead has caught up, the dual render plays exactly
    /// what the whole render plays — the live track rendered in the
    /// callback, the rest handed over from the thread ahead.
    #[test]
    fn played_ahead_the_song_is_the_same_song() {
        let whole = play(None, None, 160, no_edit);
        let dual = play(Some(AheadPrefs::default()), None, 160, no_edit);
        assert_eq!(
            dual.count(|s| &s.ahead_underruns),
            0,
            "nothing played short"
        );
        assert_eq!(
            dual.count(|s| &s.ahead_restarts),
            1,
            "one run, from the first block"
        );
        assert_same_from(&whole.out, &dual.out, 60);
    }

    /// The same over a loop: the render ahead wraps where the transport
    /// does, so the loop plays on without starting again.
    #[test]
    fn played_ahead_a_loop_wraps_where_the_transport_does() {
        let whole = play(None, Some((1_000, 6_000)), 220, no_edit);
        let dual = play(
            Some(AheadPrefs::default()),
            Some((1_000, 6_000)),
            220,
            no_edit,
        );
        assert_eq!(
            dual.count(|s| &s.ahead_restarts),
            1,
            "the loop was foreseen"
        );
        assert_same_from(&whole.out, &dual.out, 120);
    }

    /// A fader moved on a track rendered ahead is heard within the guard
    /// (and the snapshot's build), not the depth later — about as soon as
    /// rendering everything in the callback hears it.
    #[test]
    fn an_edit_ahead_is_heard_at_once() {
        const AT: usize = 120;
        let turn_down = |s: &Song<'_>, b: usize| {
            if b == AT {
                set_volume(s.daw, s.guid, "Kick", 0.25);
            }
        };
        let prefs = Some(AheadPrefs::default());
        let before = play(prefs, None, 220, no_edit);
        let after = play(prefs, None, 220, turn_down);
        let heard = first_difference(&before.out, &after.out, 60).expect("the edit is heard");
        assert!(heard >= AT, "not before it was made: block {heard}");
        let whole_before = play(None, None, 220, no_edit);
        let whole_after = play(None, None, 220, turn_down);
        let whole = first_difference(&whole_before.out, &whole_after.out, 60).expect("heard whole");
        let late_ms = |b: usize| (b - AT) as f64 * 128.0 / 48.0;
        // The guard (3 ms) and the device's block, past what the whole
        // render needs — a long way short of the 200 ms depth.
        assert!(
            late_ms(heard) <= late_ms(whole) + 12.0,
            "heard {} ms after the edit; rendered whole, {} ms",
            late_ms(heard),
            late_ms(whole)
        );
        assert!(
            after.count(|s| &s.ahead_rewinds) >= 1,
            "the render ahead went back"
        );
        assert_eq!(after.count(|s| &s.ahead_underruns), 0);
    }

    /// A change nothing ahead hears — a live track's fader, a track's
    /// name — throws nothing away.
    #[test]
    fn a_change_nothing_ahead_hears_throws_nothing_away() {
        let edit = |s: &Song<'_>, b: usize| {
            if b == 80 {
                set_volume(s.daw, s.guid, "Lead", 0.5);
            }
            if b == 100 {
                s.daw.write_project(s.guid, |p| {
                    if let Some(t) = p.tracks.iter_mut().find(|t| t.name == "Kick") {
                        t.name = "Kick In".into();
                    }
                });
            }
        };
        let played = play(Some(AheadPrefs::default()), None, 160, edit);
        assert_eq!(played.count(|s| &s.ahead_rewinds), 0);
    }

    /// A fader dragged as fast as a control surface sends (1 kHz): every
    /// block plays, nothing short, the render ahead never runs dry.
    #[test]
    fn a_storm_of_edits_never_plays_short() {
        let stop = Arc::new(AtomicBool::new(false));
        let mut storm = None;
        let edit = |s: &Song<'_>, b: usize| {
            if b == 40 {
                let (daw, guid, stop) = (s.daw.clone(), s.guid.to_string(), stop.clone());
                storm = Some(std::thread::spawn(move || {
                    let mut i = 0u32;
                    while !stop.load(Ordering::Relaxed) {
                        let v = 0.3 + 0.6 * f64::from(i % 100) / 100.0;
                        set_volume(&daw, &guid, "Kick", v);
                        set_volume(&daw, &guid, "Loop", 1.0 - v / 2.0);
                        i += 1;
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    i
                }));
            }
        };
        let played = play(Some(AheadPrefs::default()), None, 400, edit);
        stop.store(true, Ordering::Relaxed);
        let moves = storm.map_or(0, |t| t.join().unwrap_or(0));
        assert!(moves > 300, "the storm ran: {moves} moves");
        assert_eq!(played.count(|s| &s.ahead_underruns), 0, "never short");
        // (The takes are a second long: 370 blocks.)
        let silent: Vec<usize> = (40..370)
            .filter(|b| {
                played.out[b * 256..(b + 1) * 256]
                    .iter()
                    .all(|s| s.abs() < 1e-3)
            })
            .collect();
        assert!(
            silent.is_empty(),
            "every block heard: silent {silent:?}, restarts {}, stale {}, rewinds {}",
            played.count(|s| &s.ahead_restarts),
            played.count(|s| &s.ahead_stale_frames),
            played.count(|s| &s.ahead_rewinds)
        );
        assert!(
            played.count(|s| &s.ahead_rewinds) > 10,
            "edits were heard as they came"
        );
    }

    /// The blocks from `from` to `to` that are silent.
    fn silent_blocks(out: &[f32], from: usize, to: usize) -> Vec<usize> {
        (from..to)
            .filter(|b| out[b * 256..(b + 1) * 256].iter().all(|s| s.abs() < 1e-3))
            .collect()
    }

    fn set_armed(daw: &Standalone, guid: &str, name: &str, armed: bool) {
        use daw_proto::Tracks;
        let ctx = daw_proto::ProjectContext::Project(guid.to_string());
        Tracks::set_armed(daw, ctx, track(daw, guid, name), armed).expect("the arm");
    }

    /// Play from stop: the callback renders the run's start itself until
    /// the render ahead has caught up, so the song is the same song from
    /// its very first block — not one frame of silence.
    #[test]
    fn a_run_starts_without_a_gap() {
        let whole = play(None, None, 100, no_edit);
        let dual = play(Some(AheadPrefs::default()), None, 100, no_edit);
        assert_same_from(&whole.out, &dual.out, 0);
        assert_eq!(dual.count(|s| &s.ahead_underruns), 0);
        assert_eq!(dual.count(|s| &s.ahead_restarts), 1);
    }

    /// A locate while playing: the next block plays the new place, with
    /// every track, and the song is the same song once the plugins on the
    /// tracks ahead (which had heard past the locate) have settled.
    #[test]
    fn a_locate_starts_again_without_a_gap() {
        use crate::transport_engine::InstantSamples;
        const AT: usize = 100;
        let locate = |s: &Song<'_>, b: usize| {
            if b == AT {
                s.transport.set_playhead(InstantSamples(30_000));
            }
        };
        let whole = play(None, None, 160, locate);
        let dual = play(Some(AheadPrefs::default()), None, 160, locate);
        assert_eq!(silent_blocks(&dual.out, 0, 160), Vec::<usize>::new());
        assert_eq!(dual.count(|s| &s.ahead_underruns), 0);
        assert_eq!(dual.count(|s| &s.ahead_restarts), 2, "play, and the locate");
        assert_same_from(&whole.out[..AT * 256], &dual.out[..AT * 256], 0);
        assert_same_from(&whole.out, &dual.out, AT + 10);
    }

    /// Disarming the live track and arming it again: the tracks ahead play
    /// on through both, the new live set's run started by the callback.
    #[test]
    fn arming_and_disarming_play_on_without_a_gap() {
        let arm = |s: &Song<'_>, b: usize| match b {
            80 => set_armed(s.daw, s.guid, "Lead", false),
            160 => set_armed(s.daw, s.guid, "Lead", true),
            _ => {}
        };
        let whole = play(None, None, 260, arm);
        let dual = play(Some(AheadPrefs::default()), None, 260, arm);
        assert_eq!(silent_blocks(&dual.out, 0, 260), Vec::<usize>::new());
        assert_eq!(dual.count(|s| &s.ahead_underruns), 0);
        assert_eq!(
            dual.count(|s| &s.ahead_restarts),
            3,
            "play, and each change"
        );
        assert_same_from(&whole.out[..150 * 256], &dual.out[..150 * 256], 110);
        assert_same_from(&whole.out, &dual.out, 200);
    }
}
