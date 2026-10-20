//! The render ahead: the dual-buffer engine (Cubase's ASIO-Guard, REAPER's
//! anticipative FX processing).
//!
//! The audio callback has a deadline every block — 2.7 ms at 128 frames —
//! and a busy machine takes some of it, however fast the render. Most of a
//! song needs none of that urgency: tracks that play back what is already
//! recorded can be rendered any time before they are heard. So a thread of
//! its own ([`Ahead`]) renders those tracks ([`Part::Ahead`]) up to
//! [`AheadPrefs::depth_ms`] ahead of the playhead, in big chunks, and hands
//! the callback what they feed the rest ([`Feed`]); the callback renders only
//! the live tracks — armed and monitoring, played now, and what they feed —
//! at the device's block, starting from that feed ([`Part::Live`]). A spike
//! on the machine eats into the slack ahead, not into the deadline, and a
//! song too big for one block's budget still plays.
//!
//! The price: a change to a track rendered ahead (a fader, a plugin's
//! knob, an edit under the playhead) is heard up to the depth late, and
//! playback starts with the tracks ahead silent for the first blocks while
//! the first chunks render. Anything the plan cannot follow — stopped,
//! varispeed, a locate landing mid-block — renders whole in the callback,
//! as before, with the render ahead paused.
//!
//! The two sides agree on where the song is by timeline position: every
//! chunk says where it starts and which run of the transport it belongs to
//! (its generation). The callback takes the frames it is at; anything else
//! — a locate, play after stop, a loop the render ahead did not foresee,
//! a changed live set — starts the render ahead again from where the
//! playhead is. Loops are foreseen: the render ahead knows the device's
//! block and wraps where the transport will.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use audio::io::duplex::EngineStats;

use super::render::{Feed, Part, ProjectRenderer, StereoBuffer};
use crate::transport_engine::{BlockPlan, TransportShared};

/// How the render ahead runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AheadPrefs {
    /// How far ahead of the playhead to render, ms.
    pub depth_ms: f64,
    /// The biggest chunk it renders at once, frames.
    pub chunk: usize,
}

impl Default for AheadPrefs {
    fn default() -> Self {
        Self {
            depth_ms: DEFAULT_DEPTH_MS,
            chunk: CHUNK,
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

    fn depth_frames(self, sample_rate: u32) -> usize {
        (self.depth_ms.max(0.0) * f64::from(sample_rate) / 1000.0) as usize
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
}

impl Course {
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

/// Where a live block's frame `expected` falls in a chunk running from
/// `start` for `frames`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// At this frame of it.
    At(usize),
    /// It is all behind: rendered for frames already played.
    Behind,
    /// Neither: the render ahead is on another course.
    Elsewhere,
}

/// How near a frame's position has to be to count as it, samples.
const SAME_FRAME: f64 = 0.5;

pub(crate) fn place(start: f64, frames: usize, expected: f64, late_limit: f64) -> Place {
    let into = expected - start;
    let at = into.round();
    if (into - at).abs() < SAME_FRAME && at >= 0.0 && (at as usize) < frames {
        return Place::At(at as usize);
    }
    let end = start + frames as f64;
    if end <= expected + SAME_FRAME && expected - end < late_limit {
        Place::Behind
    } else {
        Place::Elsewhere
    }
}

/// One chunk rendered ahead.
#[derive(Default)]
struct Chunk {
    generation: u64,
    start: f64,
    frames: usize,
    feed: Feed,
}

/// What the callback tells the render ahead, and the render ahead the
/// callback — all atomics.
#[derive(Default)]
struct Control {
    /// The transport run the render ahead follows; the callback moves it on
    /// to start the render ahead again.
    generation: AtomicU64,
    /// Where that run starts (f64 bits), the device's block, how far into
    /// one it starts.
    restart_at: AtomicU64,
    block: AtomicUsize,
    into_block: AtomicUsize,
    /// The callback renders whole meanwhile: render nothing.
    paused: AtomicBool,
    /// Frames of the run the callback has played.
    played: AtomicU64,
    stop: AtomicBool,
}

/// Frames into a run before a block short of the render ahead counts as
/// an underrun (about 85 ms): until then the first chunks are on their way.
const STARTUP: u64 = 4096;

/// Chunks in flight at most, each way.
const RING: usize = 64;

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

/// The callback's side: the chunks handed over, the window a live block
/// reads, and the run it is on.
pub(crate) struct Player {
    control: Arc<Control>,
    filled: rtrb::Consumer<Box<Chunk>>,
    spent: rtrb::Producer<Box<Chunk>>,
    current: Option<Box<Chunk>>,
    offset: usize,
    window: Feed,
    generation: u64,
    /// The block a run was started for (0: none running).
    block: usize,
    late_limit: f64,
    stats: Arc<EngineStats>,
}

/// The tracks a block's feed came from: all the chunks it took were of one
/// live set (`Some(print)`), or it took none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Took(Option<u64>);

impl Player {
    /// Start the render ahead again at `at`, `into_block` frames into a
    /// device block of `block`.
    fn restart(&mut self, at: f64, block: usize, into_block: usize) {
        self.recycle_current();
        self.generation = self.generation.wrapping_add(1);
        self.block = block;
        let c = &self.control;
        c.played.store(0, Ordering::Relaxed);
        c.restart_at.store(at.to_bits(), Ordering::Relaxed);
        c.block.store(block, Ordering::Relaxed);
        c.into_block.store(into_block, Ordering::Relaxed);
        c.paused.store(false, Ordering::Relaxed);
        c.generation.store(self.generation, Ordering::Release);
        self.stats.ahead_restarts.fetch_add(1, Ordering::Relaxed);
    }

    /// The callback renders whole for now: the render ahead rests, and its
    /// run is over.
    fn pause(&mut self) {
        if self.block != 0 {
            self.block = 0;
            self.control.paused.store(true, Ordering::Relaxed);
            self.recycle_current();
        }
    }

    fn recycle_current(&mut self) {
        if let Some(chunk) = self.current.take() {
            let _ = self.spent.push(chunk);
        }
        self.offset = 0;
    }

    /// The feed for a live block of `frames` at `start`: the frames
    /// rendered ahead for it, silence where there are none.
    fn window(&mut self, start: f64, frames: usize) -> Took {
        if self.block != frames {
            self.restart(start, frames, 0);
        }
        self.window.clear_window();
        let mut expected = start;
        let mut need = frames;
        let mut print = None;
        while need > 0 {
            if self.current.is_none() {
                self.current = self.filled.pop().ok();
                self.offset = 0;
            }
            let Some((generation, from, span, its_print)) = self.current.as_ref().map(|c| {
                (
                    c.generation,
                    c.start + self.offset as f64,
                    c.frames - self.offset,
                    c.feed.print(),
                )
            }) else {
                // A run's first blocks find nothing yet: that is its start,
                // not a dropout.
                if self.control.played.load(Ordering::Relaxed) >= STARTUP {
                    self.stats.ahead_underruns.fetch_add(1, Ordering::Relaxed);
                }
                break;
            };
            if generation != self.generation {
                self.recycle_current();
                continue;
            }
            match place(from, span, expected, self.late_limit) {
                Place::At(i) => {
                    if print.is_some_and(|p| p != its_print) {
                        // Another live set from here on: not for this block.
                        self.restart(expected, frames, frames - need);
                        break;
                    }
                    print = Some(its_print);
                    let take = need.min(span - i);
                    if let Some(chunk) = self.current.as_ref() {
                        self.window.append(&chunk.feed, self.offset + i, take);
                    }
                    self.offset += i + take;
                    expected += take as f64;
                    need -= take;
                    if i + take == span {
                        self.recycle_current();
                    }
                }
                Place::Behind => self.recycle_current(),
                Place::Elsewhere => {
                    self.restart(expected, frames, frames - need);
                    break;
                }
            }
        }
        if need > 0 {
            self.window.append_silence(need);
        }
        self.window.print = print.unwrap_or(0);
        self.control
            .played
            .fetch_add(frames as u64, Ordering::Relaxed);
        Took(print)
    }
}

/// A project's render for the audio callback, ahead and live: what the
/// callback calls in place of `ProjectRenderer::render_plan`.
pub struct DualRender {
    live: Arc<ProjectRenderer>,
    player: Option<Player>,
    _ahead: Option<Ahead>,
}

impl DualRender {
    /// Everything rendered in the callback, as before.
    #[must_use]
    pub fn whole(live: Arc<ProjectRenderer>) -> Self {
        Self {
            live,
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
        // Live MIDI for a track rendered ahead goes on to its renderer.
        let (to_ahead, from_live) = rtrb::RingBuffer::new(1024);
        live.set_midi_forward(to_ahead);
        ahead.set_live_midi(from_live);

        let control = Arc::new(Control::default());
        control.paused.store(true, Ordering::Relaxed);
        let (filled_tx, filled_rx) = rtrb::RingBuffer::new(RING);
        let (spent_tx, spent_rx) = rtrb::RingBuffer::new(RING);
        let depth = prefs.depth_frames(sample_rate).max(1);
        let worker = Worker {
            control: control.clone(),
            renderer: ahead,
            shared,
            filled: filled_tx,
            spent: spent_rx,
            depth,
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
        let mut window = Feed::default();
        window.reset(0, 0, 0);
        Self {
            live,
            player: Some(Player {
                control: control.clone(),
                filled: filled_rx,
                spent: spent_tx,
                current: None,
                offset: 0,
                window,
                generation: 0,
                block: 0,
                late_limit: depth as f64 * 2.0,
                stats,
            }),
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
        match plan.segments() {
            [seg] if seg.playing && seg.rate == 1.0 && seg.frames > 0 => {
                let took = player.window(seg.start_samples, seg.frames);
                let out = self.live.render_part(
                    seg.start_samples,
                    seg.frames,
                    1.0,
                    Part::Live(&player.window),
                );
                // The feed was of another live set than the song's now:
                // start again, so the render ahead renders the new one.
                if let Took(Some(print)) = took
                    && print != self.live.live_print()
                {
                    let next = seg.start_samples + seg.frames as f64;
                    player.restart(next, seg.frames, 0);
                }
                out
            }
            _ => {
                player.pause();
                self.live.render_plan(plan, render_stopped)
            }
        }
    }
}

/// The render ahead's thread.
struct Worker {
    control: Arc<Control>,
    renderer: Arc<ProjectRenderer>,
    shared: Arc<TransportShared>,
    filled: rtrb::Producer<Box<Chunk>>,
    spent: rtrb::Consumer<Box<Chunk>>,
    depth: usize,
    chunk: usize,
    stats: Arc<EngineStats>,
}

/// How long the render ahead rests when it is far enough ahead, or paused.
const REST: std::time::Duration = std::time::Duration::from_millis(1);

impl Worker {
    fn run(mut self) {
        let _ = audio::io::realtime::promote_current_thread_at(audio::io::realtime::AHEAD_PRIORITY);
        let mut spare: Vec<Box<Chunk>> = Vec::new();
        let mut made = 0usize;
        let mut generation = u64::MAX;
        let mut course = Course {
            at: 0.0,
            block: 1,
            into_block: 0,
        };
        let mut produced = 0u64;
        loop {
            if self.control.stop.load(Ordering::Acquire) {
                return;
            }
            while let Ok(chunk) = self.spent.pop() {
                spare.push(chunk);
            }
            if self.control.paused.load(Ordering::Relaxed) {
                self.stats.ahead_fill_frames.store(0, Ordering::Relaxed);
                std::thread::park_timeout(REST);
                continue;
            }
            let now = self.control.generation.load(Ordering::Acquire);
            if now != generation {
                generation = now;
                course = Course {
                    at: f64::from_bits(self.control.restart_at.load(Ordering::Relaxed)),
                    block: self.control.block.load(Ordering::Relaxed).max(1),
                    into_block: self.control.into_block.load(Ordering::Relaxed),
                };
                produced = 0;
            }
            let played = self.control.played.load(Ordering::Relaxed);
            if played > produced {
                // Behind the playhead: what would come next is already
                // past. On to where the callback is — a jump, like a
                // locate, rather than frames nobody hears.
                course = course.skip((played - produced) as usize, self.wrap());
                produced = played;
            }
            let ahead_by = produced.saturating_sub(played) as usize;
            self.stats
                .ahead_fill_frames
                .store(ahead_by as u64, Ordering::Relaxed);
            if ahead_by >= self.depth || self.filled.is_full() {
                std::thread::park_timeout(REST);
                continue;
            }
            let Some(mut chunk) = spare.pop().or_else(|| {
                (made < RING).then(|| {
                    made += 1;
                    Box::<Chunk>::default()
                })
            }) else {
                std::thread::park_timeout(REST);
                continue;
            };
            // Small chunks while little is ready, so playback starts soon;
            // big ones once there is slack.
            let want = chunk_size(ahead_by, course.block, self.chunk, self.depth);
            let (frames, next) = course.next(want, self.wrap());
            let started = std::time::Instant::now();
            let _ = self
                .renderer
                .render_part(course.at, frames, 1.0, Part::Ahead(&mut chunk.feed));
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
            chunk.start = course.at;
            chunk.frames = frames;
            if let Err(rtrb::PushError::Full(chunk)) = self.filled.push(chunk) {
                spare.push(chunk);
                continue;
            }
            course = next;
            produced += frames as u64;
        }
    }

    /// The loop the transport wraps at now, if it loops.
    fn wrap(&self) -> Option<Wrap> {
        if !self.shared.is_looping() {
            return None;
        }
        self.shared.loop_region().map(|r| Wrap {
            start: r.start.0 as f64,
            end: r.end.0 as f64,
        })
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

    /// A chunk is found by position: at a frame of it, all behind, or on
    /// another course altogether.
    #[test]
    fn a_frame_is_placed_in_a_chunk_or_not() {
        assert_eq!(place(1000.0, 256, 1000.0, 4096.0), Place::At(0));
        assert_eq!(place(1000.0, 256, 1100.0, 4096.0), Place::At(100));
        assert_eq!(place(1000.0, 256, 1300.0, 4096.0), Place::Behind);
        assert_eq!(place(1000.0, 256, 900.0, 4096.0), Place::Elsewhere);
        assert_eq!(place(1000.0, 256, 99_000.0, 4096.0), Place::Elsewhere);
        assert_eq!(place(1000.0, 256, 1100.25, 4096.0), Place::At(100));
    }

    #[test]
    fn chunks_start_small_and_grow() {
        assert_eq!(chunk_size(0, 128, 1024, 9600), 128);
        assert_eq!(chunk_size(300, 128, 1024, 9600), 512);
        assert_eq!(chunk_size(5000, 128, 1024, 9600), 1024);
        assert_eq!(chunk_size(9500, 128, 1024, 9600), 128);
    }

    fn player(stats: &Arc<EngineStats>) -> (Player, rtrb::Producer<Box<Chunk>>) {
        let (tx, rx) = rtrb::RingBuffer::new(RING);
        let (spent_tx, _spent_rx) = rtrb::RingBuffer::new(RING);
        let p = Player {
            control: Arc::new(Control::default()),
            filled: rx,
            spent: spent_tx,
            current: None,
            offset: 0,
            window: Feed::default(),
            generation: 0,
            block: 0,
            late_limit: 9600.0,
            stats: stats.clone(),
        };
        (p, tx)
    }

    fn chunk(generation: u64, start: f64, frames: usize, value: f32) -> Box<Chunk> {
        let mut feed = Feed::default();
        feed.reset(frames, 5, 0);
        feed.master.fill(value);
        Box::new(Chunk {
            generation,
            start,
            frames,
            feed,
        })
    }

    /// The callback plays the frames rendered for where it is, across
    /// chunks; the first block of a run, with nothing ready, is silent and
    /// counted.
    #[test]
    fn a_block_plays_the_chunks_for_where_it_is() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx) = player(&stats);
        assert_eq!(p.window(0.0, 4), Took(None), "nothing ready yet");
        assert_eq!(
            stats.ahead_underruns.load(Ordering::Relaxed),
            0,
            "a run's start is not an underrun"
        );
        assert_eq!(p.window.master, vec![0.0; 8]);
        let generation = p.generation;
        let _ = tx.push(chunk(generation, 0.0, 6, 1.0));
        let _ = tx.push(chunk(generation, 6.0, 6, 2.0));
        assert_eq!(p.window(4.0, 4), Took(Some(5)));
        assert_eq!(
            p.window.master,
            vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]
        );
    }

    /// The song as the transport plays it, block by block: rendered whole
    /// in the callback, and by a dual render with its thread running ahead
    /// — paced like a device, so the render ahead keeps up. `looped` loops
    /// the transport over that span.
    fn played(dual: bool, looped: Option<(i64, i64)>, blocks: usize) -> (Vec<f32>, [u64; 2]) {
        use crate::transport_engine::{InstantSamples, LoopRegionSamples, PlayStateRepr};
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let (daw, guid) = crate::audio_engine::render::folder_routing_tests::split_song(None);
        // Every take a second long, and moving, so a frame out of place
        // shows.
        // (Its tone by its track's name: the two songs' maps iterate in
        // orders of their own.)
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
        let prefs = dual.then(AheadPrefs::default);
        let mut render = DualRender::new(live, shared.clone(), prefs, stats.clone());
        let mut out = Vec::new();
        let period = std::time::Duration::from_secs_f64(128.0 / 48_000.0);
        let began = std::time::Instant::now();
        for b in 0..blocks {
            let stamp = b as f64 * period.as_secs_f64() * 1e6;
            let plan = shared.begin_block(128, stamp, period.as_secs_f64() * 1e6);
            out.extend(render.render(&plan, false).samples);
            // Like a device: the next block comes a block's time later.
            if let Some(rest) = (period * (b as u32 + 1)).checked_sub(began.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        (
            out,
            [
                stats.ahead_underruns.load(Ordering::Relaxed),
                stats.ahead_restarts.load(Ordering::Relaxed),
            ],
        )
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

    /// Once the render ahead has caught up, the dual render plays exactly
    /// what the whole render plays — the live track rendered in the
    /// callback, the rest handed over from the thread ahead.
    #[test]
    fn played_ahead_the_song_is_the_same_song() {
        let (whole, _) = played(false, None, 160);
        let (dual, [underruns, restarts]) = played(true, None, 160);
        assert_eq!(underruns, 0, "past its start, nothing played short");
        assert_eq!(restarts, 1, "one run, from the first block");
        assert_same_from(&whole, &dual, 60);
    }

    /// The same over a loop: the render ahead wraps where the transport
    /// does, so the loop plays on without starting again.
    #[test]
    fn played_ahead_a_loop_wraps_where_the_transport_does() {
        let (whole, _) = played(false, Some((1_000, 6_000)), 220);
        let (dual, [_, restarts]) = played(true, Some((1_000, 6_000)), 220);
        assert_eq!(restarts, 1, "the loop was foreseen");
        assert_same_from(&whole, &dual, 120);
    }

    /// A chunk of an older run is passed over; one for another place on
    /// the timeline starts the render ahead again from where the block is.
    #[test]
    fn a_chunk_for_elsewhere_starts_the_render_ahead_again() {
        let stats = Arc::new(EngineStats::default());
        let (mut p, mut tx) = player(&stats);
        let _ = p.window(0.0, 4);
        let generation = p.generation;
        let _ = tx.push(chunk(generation.wrapping_sub(1), 4.0, 4, 9.0));
        let _ = tx.push(chunk(generation, 50_000.0, 4, 7.0));
        assert_eq!(p.window(4.0, 4), Took(None));
        assert_ne!(p.generation, generation, "a new run");
        assert_eq!(
            f64::from_bits(p.control.restart_at.load(Ordering::Relaxed)),
            4.0
        );
    }
}
