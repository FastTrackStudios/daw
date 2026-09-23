//! Streamed sources: a take whose audio is decoded a few seconds at a time
//! around the playhead, from compressed bytes held in memory.
//!
//! The browser's answer to [`AudioSource::PcmFile`](super::source::AudioSource):
//! a page has no disk to map, and 21 seven-minute stems decoded whole are
//! 3 GB. So a take's source is a [`Streamed`] — fixed-size chunks of
//! decoded audio, resident only near the playhead — and a
//! [`StreamFeeder`] keeps the chunks ahead of it filled from the take's
//! proxy (an Ogg Vorbis file, see `fts_sample::ogg_stream`). The renderer
//! reads a chunk that is not there yet as silence and says where it is
//! reading ([`Streamed::want`]), which is where the feeder decodes next.
//!
//! Nothing here blocks or spawns: the feeder does a bounded amount of work
//! per [`StreamFeeder::pump`] and whoever drives playback calls it — a
//! browser's render loop, a test.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Frames per resident chunk (~0.37 s at 44.1 kHz).
pub const CHUNK: usize = 16_384;

/// A streamed take's audio: its shape, and the chunks resident now.
#[derive(Clone)]
pub struct Streamed {
    inner: Arc<Inner>,
}

struct Inner {
    channels: u16,
    sample_rate: u32,
    frames: u64,
    /// Chunk `i` holds frames `i * CHUNK ..`, interleaved.
    chunks: RwLock<Vec<Option<Arc<[f32]>>>>,
    /// The frame playback last read from — where to decode next.
    wanted: AtomicU64,
}

impl Streamed {
    #[must_use]
    pub fn new(channels: u16, sample_rate: u32, frames: u64) -> Self {
        let count = usize::try_from(frames.div_ceil(CHUNK as u64)).unwrap_or(0);
        Self {
            inner: Arc::new(Inner {
                channels: channels.max(1),
                sample_rate,
                frames,
                chunks: RwLock::new(vec![None; count]),
                wanted: AtomicU64::new(0),
            }),
        }
    }

    #[must_use]
    pub fn channels(&self) -> u16 {
        self.inner.channels
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate
    }

    #[must_use]
    pub fn frames(&self) -> u64 {
        self.inner.frames
    }

    /// Say where playback is reading, so the feeder decodes there.
    pub fn want(&self, frame: u64) {
        self.inner.wanted.store(frame, Ordering::Relaxed);
    }

    #[must_use]
    pub fn wanted(&self) -> u64 {
        self.inner.wanted.load(Ordering::Relaxed)
    }

    /// One sample; silence where the chunk is not resident (or past the
    /// end).
    #[inline]
    #[must_use]
    pub fn sample(&self, frame: usize, channel: usize) -> f32 {
        let ch = usize::from(self.inner.channels);
        let chunks = self.inner.chunks.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        chunks
            .get(frame / CHUNK)
            .and_then(Option::as_ref)
            .and_then(|c| c.get((frame % CHUNK) * ch + channel.min(ch - 1)).copied())
            .unwrap_or(0.0)
    }

    /// Whether chunk `index` is resident.
    #[must_use]
    pub fn resident(&self, index: usize) -> bool {
        self.inner
            .chunks
            .read()
            .map(|c| c.get(index).is_some_and(Option::is_some))
            .unwrap_or(false)
    }

    fn put(&self, index: usize, pcm: Arc<[f32]>) {
        if let Ok(mut chunks) = self.inner.chunks.write()
            && let Some(slot) = chunks.get_mut(index)
        {
            *slot = Some(pcm);
        }
    }

    /// Drop every chunk outside `keep`.
    fn evict(&self, keep: std::ops::Range<usize>) {
        if let Ok(mut chunks) = self.inner.chunks.write() {
            for (i, slot) in chunks.iter_mut().enumerate() {
                if !keep.contains(&i) {
                    *slot = None;
                }
            }
        }
    }

    fn chunk_count(&self) -> usize {
        self.inner.chunks.read().map_or(0, |c| c.len())
    }
}

/// Where a feeder decodes from: anything that seeks to a frame and then
/// decodes forward. `fts_sample::ogg_stream::OggStream` is the one.
pub trait Decode {
    /// Position so the next [`Decode::decode`] starts at `frame`.
    ///
    /// # Errors
    ///
    /// The stream could not be read there.
    fn seek(&mut self, frame: u64) -> Result<(), String>;
    /// Decode onward, appending interleaved audio to `out`; the frame it
    /// starts at and its length, or `None` at the end.
    ///
    /// # Errors
    ///
    /// The stream is corrupt.
    fn decode(&mut self, out: &mut Vec<f32>) -> Result<Option<(u64, usize)>, String>;
}

#[cfg(feature = "stream-ogg")]
impl Decode for fts_sample::ogg_stream::OggStream {
    fn seek(&mut self, frame: u64) -> Result<(), String> {
        fts_sample::ogg_stream::OggStream::seek(self, frame).map_err(|e| e.to_string())
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<Option<(u64, usize)>, String> {
        fts_sample::ogg_stream::OggStream::decode(self, out).map_err(|e| e.to_string())
    }
}

/// Keeps a [`Streamed`] take's chunks filled around where playback reads.
pub struct StreamFeeder<D> {
    source: Streamed,
    decoder: D,
    /// The frame the decoder will produce next, when known.
    at: Option<u64>,
    /// Decoded audio not yet a whole chunk, starting at chunk `pending.0`.
    pending: (usize, Vec<f32>),
    failed: bool,
}

/// How far ahead of the playhead to keep decoded, and how far behind.
const AHEAD: usize = 24; // ~9 s at 44.1 kHz
const BEHIND: usize = 2;

impl<D: Decode> StreamFeeder<D> {
    #[must_use]
    pub fn new(source: Streamed, decoder: D) -> Self {
        Self {
            source,
            decoder,
            at: Some(0),
            pending: (0, Vec::new()),
            failed: false,
        }
    }

    #[must_use]
    pub fn source(&self) -> &Streamed {
        &self.source
    }

    /// Decode toward the playhead: the first chunk it needs that is not
    /// resident, then onward. Stops after about `budget` frames of work.
    /// Returns whether anything was decoded — `false` means caught up.
    pub fn pump(&mut self, budget: usize) -> bool {
        if self.failed {
            return false;
        }
        let count = self.source.chunk_count();
        let here = usize::try_from(self.source.wanted()).unwrap_or(usize::MAX) / CHUNK;
        let window = here.saturating_sub(BEHIND)..(here + AHEAD).min(count);
        self.source.evict(window.start..window.end);
        let Some(next) = window.clone().find(|i| !self.source.resident(*i)) else {
            return false;
        };
        let ch = usize::from(self.source.channels());
        let want_frame = (next * CHUNK) as u64;
        // Carry on from where the decoder is if it is part way through the
        // chunk needed; otherwise seek there, dropping what was half built.
        let pending_end = (self.pending.0 * CHUNK + self.pending.1.len() / ch) as u64;
        if self.pending.0 != next || self.at != Some(pending_end) {
            if let Err(e) = self.decoder.seek(want_frame) {
                tracing::warn!(error = %e, frame = want_frame, "stream seek failed");
                self.failed = true;
                return false;
            }
            self.at = Some(want_frame);
            self.pending = (next, Vec::with_capacity(CHUNK * ch));
        }
        let mut done = 0usize;
        while done < budget {
            let before = self.pending.1.len();
            match self.decoder.decode(&mut self.pending.1) {
                Ok(Some((_, frames))) => {
                    done += frames;
                    self.at = self.at.map(|a| a + frames as u64);
                }
                Ok(None) => {
                    // The end: whatever is pending is the last chunk.
                    self.flush_pending(ch, true);
                    self.at = None;
                    return self.pending.1.len() != before || done > 0;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "stream decode failed");
                    self.failed = true;
                    return done > 0;
                }
            }
            self.flush_pending(ch, false);
            // Past the window, or onto a chunk already there: stop, and
            // let the next pump pick the next gap.
            let chunk = self.pending.0;
            if chunk >= window.end || self.source.resident(chunk) {
                break;
            }
        }
        true
    }

    /// Move every whole chunk out of `pending` into the source (and the
    /// last, partial one when `end`).
    fn flush_pending(&mut self, ch: usize, end: bool) {
        let whole = CHUNK * ch;
        while self.pending.1.len() >= whole || (end && !self.pending.1.is_empty()) {
            let take = self.pending.1.len().min(whole);
            let rest = self.pending.1.split_off(take);
            let pcm: Arc<[f32]> = std::mem::replace(&mut self.pending.1, rest).into();
            self.source.put(self.pending.0, pcm);
            self.pending.0 += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decoder over a known signal: frame `i`'s left sample is `i` and
    /// its right is `-i` (as f32), handed out in odd-sized packets.
    struct Ramp {
        frames: u64,
        at: u64,
        seeks: usize,
    }

    impl Decode for Ramp {
        fn seek(&mut self, frame: u64) -> Result<(), String> {
            self.at = frame;
            self.seeks += 1;
            Ok(())
        }

        fn decode(&mut self, out: &mut Vec<f32>) -> Result<Option<(u64, usize)>, String> {
            if self.at >= self.frames {
                return Ok(None);
            }
            let start = self.at;
            let n = 1000.min(self.frames - start);
            for i in start..start + n {
                out.push(i as f32);
                out.push(-(i as f32));
            }
            self.at += n;
            Ok(Some((start, n as usize)))
        }
    }

    fn feeder(frames: u64) -> StreamFeeder<Ramp> {
        StreamFeeder::new(
            Streamed::new(2, 44_100, frames),
            Ramp {
                frames,
                at: 0,
                seeks: 0,
            },
        )
    }

    fn fill(f: &mut StreamFeeder<Ramp>) {
        while f.pump(50_000) {}
    }

    #[test]
    fn the_window_ahead_of_the_playhead_fills_and_reads_true() {
        let mut f = feeder(CHUNK as u64 * 100);
        assert_eq!(f.source().sample(10, 0), 0.0, "nothing resident yet: silence");
        fill(&mut f);
        let s = f.source();
        for frame in [0, 1, CHUNK - 1, CHUNK, CHUNK * 5 + 123, CHUNK * AHEAD - 1] {
            assert_eq!(s.sample(frame, 0), frame as f32, "left at {frame}");
            assert_eq!(s.sample(frame, 1), -(frame as f32), "right at {frame}");
        }
        assert!(!s.resident(AHEAD), "nothing past the window");
    }

    #[test]
    fn a_jump_seeks_once_and_evicts_what_is_behind() {
        let mut f = feeder(CHUNK as u64 * 100);
        fill(&mut f);
        let seeks = f.decoder.seeks;
        let far = CHUNK as u64 * 60 + 7;
        f.source().want(far);
        fill(&mut f);
        assert_eq!(f.decoder.seeks, seeks + 1, "one seek for the jump");
        let s = f.source();
        assert_eq!(s.sample(far as usize, 0), far as f32);
        assert!(!s.resident(0), "the old window is let go");
        // Moving on within the window decodes onward, without a seek.
        f.source().want(far + CHUNK as u64 * 3);
        fill(&mut f);
        assert_eq!(f.decoder.seeks, seeks + 1);
    }

    #[test]
    fn the_last_chunk_is_short_and_the_end_reads_silence() {
        let frames = CHUNK as u64 * 2 + 500;
        let mut f = feeder(frames);
        fill(&mut f);
        let s = f.source();
        assert_eq!(s.sample(frames as usize - 1, 0), (frames - 1) as f32);
        assert_eq!(s.sample(frames as usize, 0), 0.0, "past the end");
    }
}

/// The native driver for [`StreamFeeder`]s: one thread pumping every
/// streamed take a bounded amount at a time, round robin, resting when
/// all are caught up.
///
/// A browser pumps from its render loop; a desktop has none to borrow,
/// and a proxy decoded whole instead is ~115 MB of float per stem — a
/// setlist of them was 17 GB. Takes join with [`butler_adopt`] and stay
/// for the life of the process; their decoded window is evicted as the
/// playhead moves, so what a take holds is its compressed bytes plus
/// about [`AHEAD`] chunks.
#[cfg(all(feature = "stream-ogg", not(target_arch = "wasm32")))]
pub mod butler {
    use super::StreamFeeder;
    use fts_sample::ogg_stream::OggStream;
    use std::sync::{Mutex, OnceLock};

    type Feeders = Mutex<Vec<StreamFeeder<OggStream>>>;

    fn feeders() -> &'static Feeders {
        static FEEDERS: OnceLock<Feeders> = OnceLock::new();
        FEEDERS.get_or_init(|| {
            std::thread::Builder::new()
                .name("stream-butler".into())
                .spawn(run)
                .ok();
            Mutex::new(Vec::new())
        })
    }

    /// Hand a streamed take to the butler.
    pub fn adopt(feeder: StreamFeeder<OggStream>) {
        if let Ok(mut all) = feeders().lock() {
            all.push(feeder);
        }
    }

    fn run() {
        loop {
            let busy = feeders()
                .lock()
                .map(|mut all| {
                    let mut busy = false;
                    for feeder in all.iter_mut() {
                        // ~0.1 s of audio per take per turn: enough to stay
                        // ahead of playback, small enough that one take's
                        // catch-up never starves the rest.
                        busy |= feeder.pump(4_096);
                    }
                    busy
                })
                .unwrap_or(false);
            if !busy {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}

#[cfg(all(feature = "stream-ogg", not(target_arch = "wasm32")))]
pub use butler::adopt as butler_adopt;
