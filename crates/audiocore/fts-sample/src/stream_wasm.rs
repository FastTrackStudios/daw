//! The wasm **streamer queue** — the browser's stand-in for the native
//! streamer thread pool (W13 of `crates/signal/docs/browser-keys-rig.md`).
//!
//! Same contract as the native pool in [`super::stream`]: the audio thread
//! marks which chunks it wants and returns IMMEDIATELY; decoding happens
//! somewhere else and the decoded chunks appear in memory the audio thread
//! already reads. What differs is only how "somewhere else" is spelled.
//!
//! `wasm32-unknown-unknown` has no `std::thread::spawn` even with atomics,
//! so the workers are Web Workers the PAGE spawns, each instantiating the
//! same module over the SAME shared `WebAssembly.Memory`. Because the heap
//! is shared, a worker can operate on an `Arc<StreamedSample>` the audio
//! thread created — it only needs the pointer. So the queue carries raw
//! pointers to leaked `Arc`s, and a worker reclaims each one after filling.
//!
//! Everything here is lock-free by necessity, not preference: `atomic.wait`
//! TRAPS on the audio thread (and on the main thread), so the producer side
//! may never block. Producers push into a fixed ring with a CAS.
//!
//! The PARKING is deliberately not done here. Rust's `memory_atomic_wait32`
//! /`notify` intrinsics are nightly-only (`stdarch_wasm_atomic_wait`), and
//! this crate compiles on STABLE for every native target — a `#![feature]`
//! attribute would break all of them to serve one wasm build. So the worker
//! parks in JS instead: [`wake_addr`] hands out the byte address of a word
//! in this heap, the worker calls `Atomics.wait` on it with a short timeout,
//! and then calls [`drain`]. A timed wait needs no notify at all, which also
//! means the audio thread never has to call out to JS to wake anyone — it
//! just pushes and returns.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use super::stream::StreamedSample;

/// Ring capacity — power of two so the index mask is a bitand. A backlog
/// this deep already means the decoders are hopelessly behind; further
/// pushes are DROPPED rather than blocking the audio thread, and the sample
/// re-requests on its next read anyway (the wanted bitmask is idempotent).
const RING: usize = 1024;
const MASK: usize = RING - 1;

/// Slots hold `Arc::into_raw` pointers (0 = empty).
static SLOTS: [AtomicUsize; RING] = [const { AtomicUsize::new(0) }; RING];
/// Producer / consumer cursors. Monotonic; indices are `& MASK`.
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);
/// Futex word workers park on. Bumped on every push.
static WAKE: AtomicU32 = AtomicU32::new(0);
/// Pushes dropped because the ring was full — a decoders-are-behind signal
/// worth surfacing, not a silent failure.
static DROPPED: AtomicUsize = AtomicUsize::new(0);

/// Queue `sample` for chunk filling. **Audio-thread safe**: a CAS and a
/// notify, no allocation, no blocking, and it never fails in a way the
/// caller must handle (a full ring drops, and the request survives in the
/// sample's own wanted bitmask).
pub fn enqueue(sample: &Arc<StreamedSample>) {
    let head = HEAD.load(Ordering::Relaxed);
    let tail = TAIL.load(Ordering::Acquire);
    if head.wrapping_sub(tail) >= RING {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    // Claim a slot.
    let idx = match HEAD.compare_exchange_weak(
        head,
        head.wrapping_add(1),
        Ordering::AcqRel,
        Ordering::Relaxed,
    ) {
        Ok(_) => head & MASK,
        Err(_) => {
            // Contended: another producer took it. One retry is enough —
            // this is the audio thread, and a missed enqueue costs a chunk
            // of latency, never correctness.
            let head = HEAD.fetch_add(1, Ordering::AcqRel);
            head & MASK
        }
    };
    let ptr = Arc::into_raw(Arc::clone(sample)) as usize;
    let prev = SLOTS[idx].swap(ptr, Ordering::Release);
    if prev != 0 {
        // Overwrote an unconsumed slot (ring wrapped past a stalled
        // worker): drop that reference rather than leak it.
        unsafe { drop(Arc::from_raw(prev as *const StreamedSample)) };
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
    wake_one();
}

/// Take one queued sample, or `None` when the ring is empty.
fn dequeue() -> Option<Arc<StreamedSample>> {
    loop {
        let tail = TAIL.load(Ordering::Relaxed);
        if tail == HEAD.load(Ordering::Acquire) {
            return None;
        }
        if TAIL
            .compare_exchange_weak(
                tail,
                tail.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            continue;
        }
        let ptr = SLOTS[tail & MASK].swap(0, Ordering::AcqRel);
        if ptr == 0 {
            // Producer claimed the index but has not stored yet; treat as
            // empty and let the next wake pick it up.
            continue;
        }
        return Some(unsafe { Arc::from_raw(ptr as *const StreamedSample) });
    }
}

/// Publish that there is work. Workers observe this word (they park on it
/// with a timeout in JS), so no notify intrinsic is needed — and the audio
/// thread stays free of any call into JS.
fn wake_one() {
    WAKE.fetch_add(1, Ordering::Release);
}

/// Byte address of the word workers park on, for `Atomics.wait` from JS.
/// Valid only within this module's shared memory.
pub fn wake_addr() -> u32 {
    WAKE.as_ptr() as u32
}

/// The word's current value — the `expected` argument for `Atomics.wait`,
/// so a worker never sleeps through work queued between its check and its
/// wait (the standard futex race guard).
pub fn wake_value() -> u32 {
    WAKE.load(Ordering::Acquire)
}

/// Drain everything currently queued, decoding each sample's wanted chunks.
/// Returns how many samples were filled. Safe to call from a worker only.
pub fn drain() -> usize {
    let mut n = 0;
    while let Some(sample) = dequeue() {
        sample.fill_wanted_off_thread();
        n += 1;
    }
    n
}

/// Samples dropped because the ring was full — decoders falling behind.
pub fn dropped() -> usize {
    DROPPED.load(Ordering::Relaxed)
}

/// Queue depth right now (diagnostic).
pub fn depth() -> usize {
    HEAD.load(Ordering::Relaxed).wrapping_sub(TAIL.load(Ordering::Relaxed))
}
