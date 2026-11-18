//! Each project behind its own lock: an edit to one song holds up no
//! reader or editor of another.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use daw_proto::{Daw, ProjectInfo};

use super::Standalone;

fn info(guid: &str) -> ProjectInfo {
    ProjectInfo {
        guid: guid.into(),
        name: guid.into(),
        path: String::new(),
    }
}

/// How long an edit to the other song holds its lock, per call.
const HOLD: Duration = Duration::from_millis(5);

/// The editor's pause between edits, outside the lock.
const GAP: Duration = Duration::from_micros(200);

/// The longest a read of one song may wait while the other is edited: a
/// read is a map lookup and an uncontended lock, so anything near `HOLD`
/// means it waited on the edit.
const BUDGET: Duration = if cfg!(debug_assertions) {
    Duration::from_millis(5)
} else {
    Duration::from_millis(1)
};

/// Run `f` on another thread for as long as `probe` runs here, starting
/// once `f`'s thread is under way.
fn while_running(daw: &Standalone, f: impl Fn(&Standalone) + Send + 'static, probe: impl FnOnce()) {
    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Barrier::new(2));
    let editor = {
        let (daw, stop, started) = (daw.clone(), Arc::clone(&stop), Arc::clone(&started));
        thread::spawn(move || {
            started.wait();
            while !stop.load(Ordering::Relaxed) {
                f(&daw);
                // A breath between edits, as any editor takes: a lock
                // retaken the instant it is let go starves its own
                // readers, which is not what this measures.
                thread::sleep(GAP);
            }
        })
    };
    started.wait();
    probe();
    stop.store(true, Ordering::Relaxed);
    editor.join().expect("editor thread");
}

/// The worst time `op` took, called over and over for `window` — long
/// enough to span many of the editor's holds.
fn worst_for(window: Duration, mut op: impl FnMut()) -> Duration {
    let until = Instant::now() + window;
    let mut worst = Duration::ZERO;
    while Instant::now() < until {
        let t = Instant::now();
        op();
        worst = worst.max(t.elapsed());
    }
    worst
}

/// Wait (at most `limit`) for `cond`.
fn wait_for(limit: Duration, cond: impl Fn() -> bool) -> bool {
    let until = Instant::now() + limit;
    while !cond() {
        if Instant::now() >= until {
            return false;
        }
        thread::yield_now();
    }
    true
}

/// Song A is edited in a tight loop, each call holding A's lock ~5 ms;
/// song B's readers and editors never wait on it.
#[test]
fn an_edit_to_one_song_never_blocks_another() {
    let daw = Standalone::new();
    daw.seed_project(info("a"));
    daw.seed_project(info("b"));
    // How many holds of A the editor has entered.
    let holds = Arc::new(AtomicUsize::new(0));
    let window = Duration::from_millis(50);
    let mut b_reads = Duration::ZERO;
    let mut b_edits = Duration::ZERO;
    let mut a_waited = false;
    let editor_holds = Arc::clone(&holds);
    while_running(
        &daw,
        move |daw| {
            daw.write_project("a", |p| {
                editor_holds.fetch_add(1, Ordering::Relaxed);
                p.revision = p.revision.wrapping_add(1);
                thread::sleep(HOLD);
            });
        },
        || {
            // Measure only once the editor is at it, however busy the box.
            assert!(
                wait_for(Duration::from_secs(10), || holds.load(Ordering::Relaxed)
                    > 0),
                "the editor never took a"
            );
            b_reads = worst_for(window, || {
                daw.read_project("b", |p| p.tracks.len()).expect("b");
            });
            b_edits = worst_for(window, || {
                daw.write_project("b", |p| p.master_volume = 0.5)
                    .expect("b");
            });
            // The listing reads each project's published info: no wait.
            let listing = worst_for(window, || assert_eq!(daw.projects().len(), 2));
            assert!(listing < BUDGET, "listing projects waited {listing:?}");
            // The renderer's revision probe never waits either.
            let revision = worst_for(window, || {
                daw.try_project_revision("a").expect("a's revision");
            });
            assert!(revision < BUDGET, "a's revision waited {revision:?}");
            // And A's own readers do wait: the editor really holds it.
            a_waited = wait_for(Duration::from_secs(10), || {
                let t = Instant::now();
                daw.read_project("a", |_| ()).expect("a");
                t.elapsed() >= HOLD / 2
            });
        },
    );
    assert!(
        holds.load(Ordering::Relaxed) >= 4,
        "the editor held a only {} times: the window was not contended",
        holds.load(Ordering::Relaxed)
    );
    assert!(b_reads < BUDGET, "a read of b waited {b_reads:?}");
    assert!(b_edits < BUDGET, "an edit of b waited {b_edits:?}");
    assert!(
        a_waited,
        "a's readers never waited: the editor did not hold a"
    );
}

/// A project's revision and info, published at the end of every write,
/// read as the project holds them.
#[test]
fn a_write_publishes_revision_and_info() {
    let daw = Standalone::new();
    daw.seed_project(info("a"));
    let before = daw.try_project_revision("a").expect("a");
    daw.write_project("a", |p| p.info.name = "renamed".into());
    assert_eq!(
        daw.try_project_revision("a"),
        daw.read_project("a", |p| p.revision)
    );
    assert_ne!(daw.try_project_revision("a"), Some(before));
    assert_eq!(daw.projects()[0].name, "renamed");
}
