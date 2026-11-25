//! **The render scheduler** — the order a block's tracks are rendered in,
//! on one thread or on many, the same for a realtime render and an offline
//! one.
//!
//! Every track of a block is one node ([`super::node`]): its media, the
//! sums that come into it, its FX chain, its fader, and what it hands on
//! (its parent's or master's share, each send's). A node runs once every
//! track that feeds it — a child into its folder, a send's source, a key
//! into its sidechain — is done; the sums are pulled by the node that
//! receives them, in the order the serial topo loop added them (sources in
//! the snapshot's processing order, a source's sends before its parent
//! share). So a render on any number of threads is the serial render,
//! sample for sample.
//!
//! The nodes are claimed in one fixed order: the longest path to the end of
//! the block first (each track's cost the time it took last block), which
//! is also an order in which everything comes after what feeds it. A worker
//! that claims a node whose inputs are not done yet spins until they are.
//! Nodes with a plugin that may not run beside other tracks
//! ([`PluginInstance::parallel_safe`](crate::plugin::PluginInstance::parallel_safe))
//! run on the render thread, in the processing order, and the render thread
//! only ever runs a node that is ready: the earliest unfinished node in the
//! claim order is always ready, so the render never deadlocks.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::snapshot::TrackSnapshot;

/// Per-index access to a buffer from several threads, each index reached
/// by one thread at a time (the scheduler's contract).
pub(super) struct Cells<T> {
    ptr: *mut T,
    len: usize,
}

// SAFETY: an index is touched by one thread at a time (a node writes only
// its own track's slots, and reads another's only after that one is done,
// through the `done` flags' release/acquire).
unsafe impl<T: Send> Sync for Cells<T> {}
unsafe impl<T: Send> Send for Cells<T> {}

impl<T> Cells<T> {
    pub(super) fn new(v: &mut [T]) -> Self {
        Self {
            ptr: v.as_mut_ptr(),
            len: v.len(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// # Safety
    /// No other reference to element `i` is alive.
    #[allow(clippy::mut_from_ref)]
    pub(super) unsafe fn at(&self, i: usize) -> &mut T {
        assert!(i < self.len, "cell {i} of {}", self.len);
        // SAFETY: in bounds; the caller's contract.
        unsafe { &mut *self.ptr.add(i) }
    }

    /// # Safety
    /// No mutable reference to element `i` is alive.
    pub(super) unsafe fn get(&self, i: usize) -> &T {
        assert!(i < self.len, "cell {i} of {}", self.len);
        // SAFETY: in bounds; the caller's contract.
        unsafe { &*self.ptr.add(i) }
    }
}

/// One edge into a track's sum: `src`'s hand-on number `edge` (0 its parent
/// or master share, `k + 1` its send `k`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InEdge {
    pub(super) src: u32,
    pub(super) edge: u32,
}

/// Every edge out of track `s`, into any other track: `(dest, edge)`.
fn edges_out(tracks: &[TrackSnapshot], s: usize, mut f: impl FnMut(usize, u32)) {
    let n = tracks.len();
    let t = &tracks[s];
    if let Some(p) = t.parent_idx.filter(|&p| p < n && p != s) {
        f(p, 0);
    }
    for (si, snd) in t.sends.iter().enumerate() {
        if let Some(d) = snd.dest_idx.filter(|&d| d < n && d != s) {
            f(d, si as u32 + 1);
        }
    }
}

/// A node's work: `(track, slot)`. Shared by the threads of a pool on
/// native; a browser render has one thread.
#[cfg(not(target_arch = "wasm32"))]
pub(super) type NodeFn<'a> = dyn Fn(usize, usize) + Sync + 'a;
#[cfg(target_arch = "wasm32")]
pub(super) type NodeFn<'a> = dyn Fn(usize, usize) + 'a;

/// A track not rendered this block (another part's).
pub(super) const NOT_A_NODE: u32 = u32::MAX;

/// One block's schedule. Kept across blocks: it allocates only as the song
/// grows.
#[derive(Default)]
pub(super) struct Schedule {
    /// Each track's place in the processing order, [`NOT_A_NODE`] when it is
    /// not rendered this block.
    pub(super) pos: Vec<u32>,
    /// Each track's edges in (`ins[in_start[t]..in_start[t + 1]]`), in the
    /// order the serial loop summed them; for a node only those from
    /// earlier in the processing order (a later one, a feedback loop's, is
    /// lost there too).
    in_start: Vec<u32>,
    ins: Vec<InEdge>,
    /// Fill cursors while building `ins`.
    fill: Vec<u32>,
    /// Each node's longest path to the block's end, cost-weighted.
    reach: Vec<u64>,
    /// Nodes that run on the render thread only.
    pub(super) pinned: Vec<bool>,
    /// The claim order: every node.
    order: Vec<u32>,
    /// Of `order`, the nodes any thread may run, and the render thread's own.
    general: Vec<u32>,
    pinned_order: Vec<u32>,
    /// Per track: finished this block.
    done: Vec<AtomicBool>,
}

impl Schedule {
    /// Build the block's schedule over `tracks`, `order` the snapshot's
    /// processing order and `node(ti)` whether a track is rendered here.
    /// `cost(ti)` is what it took last block, `pinned(ti)` whether it must
    /// run on the render thread.
    pub(super) fn build(
        &mut self,
        tracks: &[TrackSnapshot],
        order: &[usize],
        node: impl Fn(usize) -> bool,
        cost: impl Fn(usize) -> u64,
        pinned: impl Fn(usize) -> bool,
    ) {
        let n = tracks.len();
        self.pos.clear();
        self.pos.resize(n, NOT_A_NODE);
        for (p, &ti) in order.iter().enumerate() {
            if ti < n && node(ti) {
                self.pos[ti] = p as u32;
            }
        }
        let pos = &self.pos;
        let forward = |s: usize, d: usize| pos[s] != NOT_A_NODE && pos[s] < pos[d];
        self.fill.clear();
        self.fill.resize(n, 0);
        for s in 0..n {
            if pos[s] == NOT_A_NODE {
                continue;
            }
            let fill = &mut self.fill;
            edges_out(tracks, s, |d, _| {
                if forward(s, d) {
                    fill[d] += 1;
                }
            });
        }
        self.in_start.clear();
        self.in_start.push(0);
        for d in 0..n {
            let at = self.in_start[d] + self.fill[d];
            self.in_start.push(at);
            self.fill[d] = self.in_start[d];
        }
        self.ins.clear();
        self.ins
            .resize(self.in_start[n] as usize, InEdge { src: 0, edge: 0 });
        for s in 0..n {
            if pos[s] == NOT_A_NODE {
                continue;
            }
            let (fill, ins) = (&mut self.fill, &mut self.ins);
            edges_out(tracks, s, |d, edge| {
                if forward(s, d) {
                    ins[fill[d] as usize] = InEdge {
                        src: s as u32,
                        edge,
                    };
                    fill[d] += 1;
                }
            });
        }
        // The serial loop's order: by source, and a source's sends before
        // its parent share.
        for d in 0..n {
            let span = self.in_start[d] as usize..self.in_start[d + 1] as usize;
            self.ins[span].sort_unstable_by_key(|e| {
                (
                    pos[e.src as usize],
                    if e.edge == 0 { u32::MAX } else { e.edge },
                )
            });
        }
        // Each node's reach: its cost and the most of what it feeds. The
        // render thread's nodes are chained in their processing order too,
        // so the claim order keeps them in it.
        self.pinned.clear();
        self.pinned
            .extend((0..n).map(|ti| pos[ti] != NOT_A_NODE && pinned(ti)));
        self.reach.clear();
        self.reach.resize(n, 0);
        for ti in 0..n {
            if pos[ti] != NOT_A_NODE {
                self.reach[ti] = cost(ti).saturating_add(1);
            }
        }
        let mut next_pinned: Option<usize> = None;
        for &d in order.iter().rev() {
            if d >= n || pos[d] == NOT_A_NODE {
                continue;
            }
            let reach = self.reach[d];
            for e in &self.ins[self.in_start[d] as usize..self.in_start[d + 1] as usize] {
                let s = e.src as usize;
                let via = cost(s).saturating_add(1).saturating_add(reach);
                self.reach[s] = self.reach[s].max(via);
            }
            if self.pinned[d] {
                if let Some(after) = next_pinned {
                    let via = cost(d).saturating_add(1).saturating_add(self.reach[after]);
                    self.reach[d] = self.reach[d].max(via);
                }
                next_pinned = Some(d);
            }
        }
        self.order.clear();
        self.order.extend(
            order
                .iter()
                .filter(|&&ti| ti < n && pos[ti] != NOT_A_NODE)
                .map(|&ti| ti as u32),
        );
        let reach = &self.reach;
        self.order
            .sort_unstable_by_key(|&ti| (std::cmp::Reverse(reach[ti as usize]), pos[ti as usize]));
        self.general.clear();
        self.pinned_order.clear();
        for &ti in &self.order {
            if self.pinned[ti as usize] {
                self.pinned_order.push(ti);
            } else {
                self.general.push(ti);
            }
        }
        if self.done.len() < n {
            self.done.resize_with(n, AtomicBool::default);
        }
        for d in &self.done[..n] {
            d.store(false, Ordering::Relaxed);
        }
    }

    /// Track `ti`'s edges in, in summing order.
    pub(super) fn ins(&self, ti: usize) -> &[InEdge] {
        match (self.in_start.get(ti), self.in_start.get(ti + 1)) {
            (Some(&a), Some(&b)) => &self.ins[a as usize..b as usize],
            _ => &[],
        }
    }

    /// The nodes in claim order.
    pub(super) fn nodes(&self) -> &[u32] {
        &self.order
    }

    fn ready(&self, ti: usize) -> bool {
        self.ins(ti)
            .iter()
            .all(|e| self.done[e.src as usize].load(Ordering::Acquire))
    }

    fn wait(&self, ti: usize, yielding: bool) {
        for e in self.ins(ti) {
            let mut spins = 0u32;
            while !self.done[e.src as usize].load(Ordering::Acquire) {
                spins = spins.wrapping_add(1);
                if yielding && spins.is_multiple_of(4_096) {
                    std::thread::yield_now();
                } else {
                    std::hint::spin_loop();
                }
            }
        }
    }

    fn finish(&self, ti: usize, node: &NodeFn<'_>, slot: usize) {
        // A node catches its plugins' panics itself; anything else must
        // still mark it done, or what it feeds waits forever.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| node(ti, slot)));
        self.done[ti].store(true, Ordering::Release);
    }

    /// Every node on this thread, in claim order (slot 0).
    pub(super) fn run_serial(&self, node: &NodeFn<'_>) {
        for &ti in &self.order {
            self.finish(ti as usize, node, 0);
        }
    }

    /// Every node across `pool` and this thread: `node(track, slot)`, the
    /// slot a worker's own (0..workers), the render thread's `workers`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn run_parallel(
        &self,
        pool: &super::parallel::LanePool,
        node: &NodeFn<'_>,
        context: crate::plugin::RenderContext,
        stopped: bool,
    ) {
        let cursor = AtomicUsize::new(0);
        let general = &self.general;
        let yielding = !pool.realtime();
        let workers = pool.threads();
        let job = |slot: usize| {
            // A worker renders inside the render thread's block: its frame,
            // its clock, whether the transport rolls.
            let _frame = (crate::plugin::RenderFrameGuard::current() != context)
                .then(|| crate::plugin::RenderFrameGuard::adopt(context));
            let _stopped = (stopped && !crate::plugin::rendering_stopped())
                .then(crate::plugin::StoppedRender::enter);
            loop {
                let i = cursor.fetch_add(1, Ordering::AcqRel);
                let Some(&ti) = general.get(i) else {
                    return;
                };
                self.wait(ti as usize, yielding);
                self.finish(ti as usize, node, slot);
            }
        };
        let main = || {
            let mut pin = 0;
            let mut spins = 0u32;
            loop {
                if let Some(&ti) = self.pinned_order.get(pin)
                    && self.ready(ti as usize)
                {
                    self.finish(ti as usize, node, workers);
                    pin += 1;
                    continue;
                }
                let c = cursor.load(Ordering::Acquire);
                if let Some(&ti) = general.get(c)
                    && self.ready(ti as usize)
                    && cursor
                        .compare_exchange(c, c + 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    self.finish(ti as usize, node, workers);
                    continue;
                }
                if pin >= self.pinned_order.len() && c >= general.len() {
                    return;
                }
                spins = spins.wrapping_add(1);
                if yielding && spins.is_multiple_of(4_096) {
                    std::thread::yield_now();
                } else {
                    std::hint::spin_loop();
                }
            }
        };
        pool.run_with_main(workers, &job, &main);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_hand_out_one_index_each() {
        let mut v = vec![1, 2, 3];
        let c = Cells::new(&mut v);
        // SAFETY: one reference per index.
        unsafe {
            *c.at(1) += 10;
            assert_eq!(*c.get(1), 12);
        }
        assert_eq!(c.len(), 3);
    }
}
