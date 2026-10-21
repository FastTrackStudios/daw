//! Realtime **worker threads** for a render that splits a block across
//! cores. The audio callback's own thread is realtime and in the device's
//! audio workgroup; a worker that renders part of its block must be too, or
//! the scheduler treats it as ordinary work — it runs late under load, on an
//! efficiency core, and the block misses its deadline waiting for it.
//!
//! On macOS a worker takes a realtime time-constraint policy sized to the
//! IO period and joins the IO thread's workgroup
//! (`kAudioDevicePropertyIOThreadOSWorkgroup`), which the CoreAudio backend
//! publishes here when a device starts. On Linux a worker goes `SCHED_FIFO`
//! at the audio thread's priority ([`super::realtime`]) on its first batch:
//! nothing hands it the callback's priority, and an ordinary worker
//! pre-empted mid-job by a build or a browser held the callback, which
//! spins until every job it handed out is done, past its deadline.

use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

/// The running device's IO workgroup (an `os_workgroup_t`, retained and
/// never released — a worker may still be joined to an old one).
static IO_WORKGROUP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The running device's IO period, nanoseconds (0: none running).
static IO_PERIOD_NS: AtomicU64 = AtomicU64::new(0);

/// A device started: its IO workgroup (may be null) and period.
pub(crate) fn publish(workgroup: *mut std::ffi::c_void, period_ns: u64) {
    IO_PERIOD_NS.store(period_ns, Ordering::Relaxed);
    if !workgroup.is_null() {
        IO_WORKGROUP.store(workgroup, Ordering::Release);
    }
}

/// A worker's membership of the IO workgroup. Call [`WorkerSeat::sync`]
/// before each batch of work: it joins the current device's workgroup the
/// first time, and moves across when the device changes. Leaves on drop.
#[derive(Default)]
pub struct WorkerSeat {
    #[cfg(target_os = "macos")]
    joined: Option<(usize, macos::JoinToken)>,
    #[cfg(target_os = "linux")]
    realtime: bool,
    /// The priority it asks for on Linux; 0: the audio callback's.
    priority: i32,
    period_ns: u64,
}

impl WorkerSeat {
    /// A seat at `priority` rather than the callback's — a worker of the
    /// render ahead ([`super::realtime::AHEAD_PRIORITY`]).
    #[must_use]
    pub fn at(priority: i32) -> Self {
        // Not `..Self::default()`: the seat leaves its workgroup on drop
        // (macOS), and a type with `Drop` cannot be built by moving fields
        // out of another value (E0509).
        let mut seat = Self::default();
        seat.priority = priority;
        seat
    }

    /// Follow the running device: realtime policy for its period, member of
    /// its workgroup. Cheap when nothing changed (two atomic loads).
    pub fn sync(&mut self) {
        #[cfg(target_os = "linux")]
        if !self.realtime {
            // Once, taken or not: refused, it said why, and asking again
            // every block would only say it again.
            self.realtime = true;
            let priority = if self.priority > 0 {
                self.priority
            } else {
                super::realtime::AUDIO_PRIORITY
            };
            let _ = super::realtime::promote_current_thread_at(priority);
        }
        let period = IO_PERIOD_NS.load(Ordering::Relaxed);
        if period != 0 && period != self.period_ns {
            self.period_ns = period;
            #[cfg(target_os = "macos")]
            macos::set_realtime(period);
        }
        #[cfg(target_os = "macos")]
        {
            let wg = IO_WORKGROUP.load(Ordering::Acquire);
            if wg.is_null() || self.joined.as_ref().is_some_and(|j| j.0 == wg as usize) {
                return;
            }
            if let Some((old, token)) = self.joined.take() {
                macos::leave(old as *mut _, token);
            }
            self.joined = macos::join(wg).map(|t| (wg as usize, t));
        }
    }
}

impl Drop for WorkerSeat {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some((wg, token)) = self.joined.take() {
            macos::leave(wg as *mut _, token);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    /// `os_workgroup_join_token_s`: a 4-byte signature and 36 opaque bytes.
    #[repr(C)]
    pub struct JoinToken {
        sig: u32,
        opaque: [u8; 36],
    }

    #[repr(C)]
    struct TimeConstraint {
        period: u32,
        computation: u32,
        constraint: u32,
        preemptible: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    const THREAD_TIME_CONSTRAINT_POLICY: u32 = 2;
    const THREAD_TIME_CONSTRAINT_POLICY_COUNT: u32 = 4;

    unsafe extern "C" {
        fn os_workgroup_join(wg: *mut c_void, token: *mut JoinToken) -> i32;
        fn os_workgroup_leave(wg: *mut c_void, token: *mut JoinToken);
        fn mach_thread_self() -> u32;
        fn mach_port_deallocate(task: u32, name: u32) -> i32;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
        fn thread_policy_set(
            thread: u32,
            flavor: u32,
            policy: *const TimeConstraint,
            count: u32,
        ) -> i32;
        static mach_task_self_: u32;
    }

    /// Give this thread a realtime time constraint for an IO period of
    /// `period_ns`: up to half the period of computation, done within it —
    /// what CoreAudio gives its own IO thread.
    pub fn set_realtime(period_ns: u64) {
        let mut tb = Timebase::default();
        // SAFETY: plain out-parameter.
        unsafe { mach_timebase_info(&mut tb) };
        if tb.numer == 0 {
            return;
        }
        let abs = |ns: u64| {
            (ns * u64::from(tb.denom) / u64::from(tb.numer)).min(u64::from(u32::MAX)) as u32
        };
        let policy = TimeConstraint {
            period: abs(period_ns),
            computation: abs(period_ns / 2),
            constraint: abs(period_ns),
            preemptible: 1,
        };
        // SAFETY: this thread's own port, a policy struct of the declared
        // flavour and count; the port right is released after.
        unsafe {
            let me = mach_thread_self();
            let status = thread_policy_set(
                me,
                THREAD_TIME_CONSTRAINT_POLICY,
                &policy,
                THREAD_TIME_CONSTRAINT_POLICY_COUNT,
            );
            mach_port_deallocate(mach_task_self_, me);
            if status != 0 {
                tracing::warn!(status, "render worker: realtime policy refused");
            }
        }
    }

    pub fn join(wg: *mut c_void) -> Option<JoinToken> {
        let mut token = JoinToken {
            sig: 0,
            opaque: [0; 36],
        };
        // SAFETY: `wg` is a retained, never-released workgroup; the token is
        // kept until the matching leave, on this same thread.
        let status = unsafe { os_workgroup_join(wg, &mut token) };
        if status == 0 {
            Some(token)
        } else {
            tracing::warn!(status, "render worker: could not join the audio workgroup");
            None
        }
    }

    pub fn leave(wg: *mut c_void, mut token: JoinToken) {
        // SAFETY: joined on this thread with this token.
        unsafe { os_workgroup_leave(wg, &mut token) };
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    /// A worker seated for its first batch runs at the audio thread's
    /// policy where the user may (the `audio` group), as it was otherwise.
    #[test]
    fn a_seated_worker_is_as_realtime_as_the_callback() {
        let (callback, worker) = std::thread::spawn(|| {
            let callback = super::super::realtime::promote_current_thread();
            let worker = std::thread::spawn(|| {
                let mut seat = super::WorkerSeat::default();
                seat.sync();
                seat.sync();
                // SAFETY: reads the calling thread's policy.
                (unsafe { libc::sched_getscheduler(0) }) & !libc::SCHED_RESET_ON_FORK
            })
            .join()
            .expect("the worker");
            (callback, worker)
        })
        .join()
        .expect("the callback");
        let expected = if callback {
            libc::SCHED_FIFO
        } else {
            libc::SCHED_OTHER
        };
        assert_eq!(worker, expected);
    }
}
