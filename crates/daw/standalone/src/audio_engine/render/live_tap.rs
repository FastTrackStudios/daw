//! Live taps: a point of a track ([`super::offline::Probe`]) read while the
//! song plays, its audio handed off the audio thread through a lock-free
//! ring for something else to analyse.
//!
//! A tap is opened off the audio thread ([`LiveTaps::open`]), which makes
//! its ring there. On the audio thread a render copies each block of the
//! point into the ring and does nothing else: no allocation (the ring is
//! made already; `write_chunk_uninit` writes into it), no waiting (a slot
//! is taken with `try_lock` — if the control thread is changing it this
//! instant, the block is skipped), no I/O. A ring that is full
//! (its reader fell behind) drops the block and counts it too. The reader
//! drains the other end at its own pace ([`LiveTap::drain`]). Once a block
//! the render looks up which track and point each open tap reads (eight
//! `try_lock`s, a plan on the stack); its capture points then compare
//! indices, and lock a slot only to write to it.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::offline::{Probe, ProbeAt};

/// How many taps can be open at once.
pub const SLOTS: usize = 8;

/// One open tap, as the audio thread sees it.
struct Writer {
    /// The song it is on.
    project: String,
    at: ProbeAt,
    ring: rtrb::Producer<f32>,
    /// Blocks not written: the ring was full.
    dropped: Arc<AtomicU64>,
}

/// The engine's live taps.
pub struct LiveTaps {
    slots: Vec<Mutex<Option<Writer>>>,
    /// How many are open: zero, and a render looks no further.
    open: AtomicUsize,
}

impl Default for LiveTaps {
    fn default() -> Self {
        Self {
            slots: (0..SLOTS).map(|_| Mutex::new(None)).collect(),
            open: AtomicUsize::new(0),
        }
    }
}

/// The reader's end of a tap.
pub struct LiveTap {
    /// Its slot (what [`LiveTaps::close`] takes).
    pub slot: usize,
    ring: rtrb::Consumer<f32>,
    dropped: Arc<AtomicU64>,
}

impl LiveTap {
    /// Everything written since the last drain, interleaved stereo,
    /// appended to `into`.
    pub fn drain(&mut self, into: &mut Vec<f32>) {
        let n = self.ring.slots();
        if let Ok(chunk) = self.ring.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            into.extend_from_slice(a);
            into.extend_from_slice(b);
            chunk.commit_all();
        }
    }

    /// Blocks dropped so far (the ring was full: the reader fell behind).
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl LiveTaps {
    /// Open a tap on `at` of song `project`, its ring holding `frames`
    /// stereo frames. Off the audio thread.
    ///
    /// # Errors
    /// Every slot is taken.
    pub fn open(&self, project: &str, at: ProbeAt, frames: usize) -> Result<LiveTap, String> {
        let (ring, reader) = rtrb::RingBuffer::new(frames.max(1) * 2);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut writer = Some(Writer {
            project: project.to_owned(),
            at,
            ring,
            dropped: dropped.clone(),
        });
        for (i, slot) in self.slots.iter().enumerate() {
            let mut s = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if s.is_none() {
                *s = writer.take();
                self.open.fetch_add(1, Ordering::AcqRel);
                return Ok(LiveTap {
                    slot: i,
                    ring: reader,
                    dropped,
                });
            }
        }
        Err(format!("all {SLOTS} live taps are open"))
    }

    /// Close the tap in `slot`. Off the audio thread.
    pub fn close(&self, slot: usize) {
        let Some(s) = self.slots.get(slot) else {
            return;
        };
        let was = s
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if was.is_some() {
            self.open.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// Whether any tap is open: a render with none does nothing more.
    #[inline]
    pub(super) fn any(&self) -> bool {
        self.open.load(Ordering::Acquire) > 0
    }

    /// On the audio thread, once a block: which track (by index, through
    /// `index_of` its guid) and point each open tap on song `project`
    /// reads, into `plan` (a slot busy this instant is left out).
    pub(super) fn plan(
        &self,
        project: &str,
        index_of: impl Fn(&str) -> Option<usize>,
        plan: &mut [Option<(usize, Probe)>; SLOTS],
    ) {
        for (slot, out) in self.slots.iter().zip(plan.iter_mut()) {
            *out = slot.try_lock().ok().and_then(|s| {
                let w = s.as_ref()?;
                (w.project == project)
                    .then(|| index_of(&w.at.track).map(|ti| (ti, w.at.at)))
                    .flatten()
            });
        }
    }

    /// On the audio thread: `samples` (interleaved stereo) into the tap in
    /// `slot`; a full ring drops the block and counts it.
    pub(super) fn write(&self, slot: usize, samples: &[f32]) {
        let Some(Ok(mut s)) = self.slots.get(slot).map(Mutex::try_lock) else {
            return;
        };
        let Some(w) = s.as_mut() else { return };
        match w.ring.write_chunk_uninit(samples.len()) {
            Ok(chunk) => {
                chunk.fill_from_iter(samples.iter().copied());
            }
            Err(_) => {
                w.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
