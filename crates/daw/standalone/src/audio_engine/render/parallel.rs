//! **Lane pool** — a renderer's worker threads, so the tracks of a block
//! run on several cores at once ([`super::schedule`] says in which order:
//! each track as soon as what feeds it is done). A realtime renderer's
//! pool runs at the audio callback's priority, a few threads; an offline
//! render's at normal priority, sized to the machine
//! ([`offline_threads`]), taken from a few kept process-wide
//! ([`OfflinePool`]).
//!
//! The render thread never waits on a lock or a wakeup: it publishes a
//! batch with one atomic store, unparks the workers (a semaphore signal,
//! no syscall when they are still spinning), and takes jobs itself like any
//! worker. It only waits for the last job running elsewhere, spinning —
//! the same wait a serial render would have spent on that job.
//!
//! Claims are one compare-exchange on a word holding the batch's epoch, the
//! next job and the job count, so a worker that wakes late for a finished
//! batch can never take a job of the next one: its claim fails on the epoch.

#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How long a worker spins for the next batch before parking. A block is
/// ~2.7 ms at 128 frames, so a worker parks between blocks and a batch
/// pays one wakeup; spinning covers back-to-back batches cheaply.
const SPIN: Duration = Duration::from_micros(60);

/// The batch being run: the job function lives on the render thread's
/// stack for the batch, so a worker reads it only after a successful claim
/// (which the render thread outlives by waiting for every claimed job).
type JobFn = *const (dyn Fn(usize) + Sync);

struct Shared {
    /// epoch (32) | next job (16) | job count (16).
    state: AtomicU64,
    /// Jobs finished in the current batch.
    done: AtomicUsize,
    job: UnsafeCell<Option<JobFn>>,
    quit: AtomicBool,
}

// SAFETY: `job` is written only by the render thread between batches (no
// claim can succeed then) and read by a worker only after a claim that the
// render thread waits out before writing it again.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

const fn pack(epoch: u32, next: u16, count: u16) -> u64 {
    ((epoch as u64) << 32) | ((next as u64) << 16) | count as u64
}

const fn unpack(s: u64) -> (u32, u16, u16) {
    ((s >> 32) as u32, (s >> 16) as u16, s as u16)
}

impl Shared {
    /// Take the next job of batch `epoch`, if there is one left.
    fn claim(&self, epoch: u32) -> Option<usize> {
        let mut s = self.state.load(Ordering::Acquire);
        loop {
            let (e, next, count) = unpack(s);
            if e != epoch || next >= count {
                return None;
            }
            match self.state.compare_exchange_weak(
                s,
                pack(e, next + 1, count),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(next as usize),
                Err(now) => s = now,
            }
        }
    }

    /// Run jobs of `epoch` until none are left.
    fn work(&self, epoch: u32) {
        while let Some(i) = self.claim(epoch) {
            // SAFETY: a claim succeeded, so the batch's job is published and
            // stays alive until this job is counted done.
            // A panic must still count the job done, or the render thread
            // waits forever; jobs catch their plugins' panics themselves.
            if let Some(f) = unsafe { *self.job.get() } {
                let _ =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*f)(i) }));
            }
            self.done.fetch_add(1, Ordering::Release);
        }
    }
}

pub(crate) struct LanePool {
    shared: Arc<Shared>,
    workers: Vec<std::thread::JoinHandle<()>>,
    epoch: std::cell::Cell<u32>,
    realtime: bool,
}

// SAFETY: `epoch` is touched only by `run`, which only the render thread
// calls (the renderer holds its scratch lock across it).
unsafe impl Sync for LanePool {}

impl LanePool {
    /// `threads` workers (the render thread makes one more).
    pub(crate) fn new(threads: usize) -> Self {
        Self::named(threads, "render-lane", 0)
    }

    /// `threads` workers called `{name}-{i}`, realtime at `priority` (0:
    /// the audio callback's) — the render ahead's own pool runs below the
    /// callback.
    pub(crate) fn named(threads: usize, name: &str, priority: i32) -> Self {
        Self::spawn(threads, name, Some(priority))
    }

    /// `threads` workers at normal priority, for offline renders: they
    /// yield while they wait, so they share the machine.
    pub(crate) fn offline(threads: usize) -> Self {
        Self::spawn(threads, "render-offline", None)
    }

    fn spawn(threads: usize, name: &str, priority: Option<i32>) -> Self {
        let shared = Arc::new(Shared {
            state: AtomicU64::new(0),
            done: AtomicUsize::new(0),
            job: UnsafeCell::new(None),
            quit: AtomicBool::new(false),
        });
        let workers = (0..threads)
            .filter_map(|i| {
                let shared = shared.clone();
                std::thread::Builder::new()
                    .name(format!("{name}-{i}"))
                    .spawn(move || worker(&shared, priority))
                    .ok()
            })
            .collect();
        Self {
            shared,
            workers,
            epoch: std::cell::Cell::new(0),
            realtime: priority.is_some(),
        }
    }

    /// Whether its workers are the audio callback's (they never yield).
    pub(crate) fn realtime(&self) -> bool {
        self.realtime
    }

    /// Worker threads (not counting the render thread).
    pub(crate) fn threads(&self) -> usize {
        self.workers.len()
    }

    /// Run `job(0..count)` across the workers and this thread; returns when
    /// every job has finished. Realtime-safe: no lock, no allocation.
    pub(crate) fn run(&self, count: usize, job: &(dyn Fn(usize) + Sync)) {
        self.run_with_main(count, job, &|| {});
    }

    /// [`run`](Self::run), with `main` run on this thread first, while the
    /// workers take the jobs; then this thread takes what jobs are left.
    pub(crate) fn run_with_main(
        &self,
        count: usize,
        job: &(dyn Fn(usize) + Sync),
        main: &dyn Fn(),
    ) {
        let count = count.min(u16::MAX as usize);
        if count == 0 {
            main();
            return;
        }
        let epoch = self.epoch.get().wrapping_add(1);
        self.epoch.set(epoch);
        // SAFETY: no claim can succeed now (the last batch is finished), so
        // no worker reads `job`. The pointer's lifetime is erased; this call
        // outlives every use (it waits for every job below).
        unsafe {
            let f: JobFn = std::mem::transmute::<&(dyn Fn(usize) + Sync), JobFn>(job);
            *self.shared.job.get() = Some(f);
        }
        self.shared.done.store(0, Ordering::Relaxed);
        self.shared
            .state
            .store(pack(epoch, 0, count as u16), Ordering::Release);
        for w in &self.workers {
            w.thread().unpark();
        }
        // Every job must still be waited out if `main` panics.
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(main));
        self.shared.work(epoch);
        while self.shared.done.load(Ordering::Acquire) < count {
            std::hint::spin_loop();
        }
        if let Err(panic) = unwound {
            std::panic::resume_unwind(panic);
        }
    }
}

impl Drop for LanePool {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        for w in &self.workers {
            w.thread().unpark();
        }
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker(shared: &Shared, priority: Option<i32>) {
    #[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
    let mut seat = priority.map(audio::io::rt_workers::WorkerSeat::at);
    #[cfg(not(all(feature = "audio", not(target_arch = "wasm32"))))]
    let _ = priority;
    let mut seen = unpack(shared.state.load(Ordering::Acquire)).0;
    let mut idle_since = Instant::now();
    loop {
        if shared.quit.load(Ordering::Acquire) {
            return;
        }
        let epoch = unpack(shared.state.load(Ordering::Acquire)).0;
        if epoch != seen {
            seen = epoch;
            #[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
            if let Some(seat) = seat.as_mut() {
                seat.sync();
            }
            shared.work(epoch);
            idle_since = Instant::now();
            continue;
        }
        if idle_since.elapsed() < SPIN {
            std::hint::spin_loop();
        } else {
            // An unpark between the check above and here is not lost: the
            // park returns at once.
            std::thread::park_timeout(Duration::from_millis(100));
        }
    }
}

/// Worker count for a realtime renderer: `FTS_RENDER_THREADS` if set (0
/// renders serially), else a few of the performance cores — the render
/// thread is one more, and the UI and the sample streamer need the rest.
/// Workers for a render ahead's pool (`FTS_RENDER_AHEAD_THREADS`, else
/// half the cores, at most 15): it renders the whole song off the
/// callback's deadline, so a mix of a few thousand plugins wants every
/// core it can have — one render-ahead thread at a full core played
/// Unbreakable at a tenth of realtime.
pub(crate) fn ahead_threads() -> usize {
    if let Some(n) = std::env::var("FTS_RENDER_AHEAD_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return n.min(15);
    }
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    (cores / 2).clamp(1, 15)
}

pub(crate) fn default_threads() -> usize {
    if let Some(n) = std::env::var("FTS_RENDER_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return n.min(15);
    }
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    (cores / 3).clamp(0, 3)
}

/// Workers for an offline render: `FTS_OFFLINE_RENDER_THREADS` if set (0
/// renders on the calling thread alone), else the machine's cores but two
/// (one for the render thread, one for everything else), at most 63.
pub(crate) fn offline_threads() -> usize {
    if let Some(n) = std::env::var("FTS_OFFLINE_RENDER_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return n.min(63);
    }
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    cores.saturating_sub(3).min(63)
}

/// How many offline pools run at once (`FTS_OFFLINE_RENDER_POOLS`, else 2):
/// a render past them renders on its own thread, rather than every render
/// at once spinning on every core.
fn most_offline_pools() -> usize {
    std::env::var("FTS_OFFLINE_RENDER_POOLS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(2)
}

/// Offline pools not in use, kept for the next render (threads are not
/// spawned per render), and how many are out.
static OFFLINE_POOLS: std::sync::Mutex<(Vec<LanePool>, usize)> =
    std::sync::Mutex::new((Vec::new(), 0));

/// An offline render's pool, borrowed from the process's for as long as
/// the render lives.
pub(crate) struct OfflinePool(Option<LanePool>);

impl OfflinePool {
    /// A pool of `threads` workers, if one may run now (`None`: render on
    /// the calling thread). `bounded`: only while fewer than
    /// `FTS_OFFLINE_RENDER_POOLS` are out (a render that asked for its
    /// threads by number gets them regardless).
    pub(crate) fn take(threads: usize, bounded: bool) -> Option<Self> {
        if threads == 0 {
            return None;
        }
        let mut pools = OFFLINE_POOLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if bounded && pools.1 >= most_offline_pools() {
            return None;
        }
        pools.1 += 1;
        let kept = pools
            .0
            .iter()
            .position(|p| p.threads() == threads)
            .map(|i| pools.0.swap_remove(i));
        drop(pools);
        Some(Self(Some(
            kept.unwrap_or_else(|| LanePool::offline(threads)),
        )))
    }

    pub(crate) fn pool(&self) -> &LanePool {
        self.0.as_ref().expect("a pool until dropped")
    }
}

impl Drop for OfflinePool {
    fn drop(&mut self) {
        let mut pools = OFFLINE_POOLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pools.1 = pools.1.saturating_sub(1);
        let pool = self.0.take();
        let spare = match pool {
            Some(pool) if pools.0.len() < 4 => {
                pools.0.push(pool);
                None
            }
            other => other,
        };
        drop(pools);
        // Its threads are joined outside the lock.
        drop(spare);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn every_job_runs_once_per_batch() {
        let pool = LanePool::new(3);
        let hits: Vec<AtomicU32> = (0..40).map(|_| AtomicU32::new(0)).collect();
        for _ in 0..2_000 {
            pool.run(hits.len(), &|i| {
                hits[i].fetch_add(1, Ordering::Relaxed);
            });
        }
        assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 2_000));
    }

    #[test]
    fn a_batch_sees_what_was_written_before_it_and_is_seen_after() {
        let pool = LanePool::new(2);
        let mut data = vec![0u64; 16];
        for round in 1..500u64 {
            let ptr = data.as_mut_ptr() as usize;
            pool.run(16, &move |i| {
                // SAFETY: each job its own slot.
                let slot = unsafe { &mut *(ptr as *mut u64).add(i) };
                assert_eq!(*slot, round - 1);
                *slot = round;
            });
            assert!(data.iter().all(|&v| v == round));
        }
    }
}
