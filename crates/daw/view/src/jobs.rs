//! What is being done off the window's thread, and how far along: reading
//! waveforms, opening and saving the project file — anything the window
//! should not wait on but the person should be able to see.
//!
//! A piece of work holds a [`Job`] while it runs and moves it on as it
//! goes; dropping it ends it. The bottom bar shows what is running
//! ([`running`]).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

struct Inner {
    id: u64,
    label: String,
    done: AtomicUsize,
    total: AtomicUsize,
}

/// One piece of work, running while held.
#[derive(Clone)]
pub struct Job(Arc<Inner>);

/// A job running now, as [`running`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    /// Its own, for as long as the process runs — what tells two jobs of
    /// the same label apart (a set's songs open four at once, each an
    /// "Opening", each reading its "Waveforms").
    pub id: u64,
    pub label: String,
    pub done: usize,
    /// Steps in all; 0 for a job that does not know.
    pub total: usize,
}

fn all() -> &'static Mutex<Vec<Weak<Inner>>> {
    static JOBS: Mutex<Vec<Weak<Inner>>> = Mutex::new(Vec::new());
    &JOBS
}

/// Where this process's job ids start: its pid, high. A window shows its
/// engine's jobs beside its own, and two processes' ids must not meet —
/// while one process (iOS, a test) lists each job once, by its one id.
fn base() -> u64 {
    // A page is one process with no pid to ask for.
    #[cfg(target_arch = "wasm32")]
    return 0;
    #[cfg(not(target_arch = "wasm32"))]
    return u64::from(std::process::id()) << 32;
}

/// Start a piece of work named `label`, `total` steps long (0 for one
/// that does not know).
#[must_use]
pub fn start(label: &str, total: usize) -> Job {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let inner = Arc::new(Inner {
        id: base() + NEXT.fetch_add(1, Ordering::Relaxed),
        label: label.to_owned(),
        done: AtomicUsize::new(0),
        total: AtomicUsize::new(total),
    });
    if let Ok(mut jobs) = all().lock() {
        jobs.retain(|j| j.strong_count() > 0);
        jobs.push(Arc::downgrade(&inner));
    }
    Job(inner)
}

impl Job {
    /// `n` more steps done.
    pub fn advance(&self, n: usize) {
        self.0.done.fetch_add(n, Ordering::Relaxed);
    }
}

/// What is running now, in the order it started.
#[must_use]
pub fn running() -> Vec<Running> {
    all().lock().map_or_else(
        |_| Vec::new(),
        |jobs| {
            jobs.iter()
                .filter_map(Weak::upgrade)
                .map(|j| Running {
                    id: j.id,
                    label: j.label.clone(),
                    done: j.done.load(Ordering::Relaxed),
                    total: j.total.load(Ordering::Relaxed),
                })
                .collect()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{running, start};

    /// The bottom bar keys a job's row by its id: songs of a set open in
    /// parallel, each its own "Opening" — keyed by label, four of them
    /// were four siblings with one key, and the window panicked.
    #[test]
    fn jobs_of_one_label_are_told_apart() {
        let jobs: Vec<_> = (0..4).map(|_| start("Opening", 0)).collect();
        let ids: std::collections::HashSet<u64> = running()
            .into_iter()
            .filter(|j| j.label == "Opening")
            .map(|j| j.id)
            .collect();
        assert_eq!(ids.len(), jobs.len());
        drop(jobs);
        assert!(running().iter().all(|j| j.label != "Opening"));
    }
}
