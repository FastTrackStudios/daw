//! Realtime scheduling for the audio threads.
//!
//! An audio callback has a deadline every block (2.7 ms at 128 frames,
//! 48 kHz). Scheduled like any other thread (`SCHED_OTHER`), it is
//! pre-empted whenever the machine is busy — a build, a browser — and a
//! few milliseconds off the CPU mid-block is a click or a dropout however
//! fast the render is. DAWs and PipeWire's own data loop run `SCHED_FIFO`;
//! the stream threads cpal makes for us (`pw_out`, `pw_in`) did not.

/// The `SCHED_FIFO` priority the audio threads ask for: above every
/// ordinary thread, below PipeWire's data loop and a PTP daemon (80+),
/// which must never wait on us.
pub const AUDIO_PRIORITY: i32 = 60;

/// The priority of the threads that render ahead of the playhead (a
/// dual-buffer engine): above every ordinary thread, so a busy machine
/// does not starve them, and below the audio callback, which they must
/// never hold up.
pub const AHEAD_PRIORITY: i32 = 50;

/// Make the calling thread realtime (`SCHED_FIFO` at [`AUDIO_PRIORITY`],
/// reset on fork). Call it from the audio callback's own thread — once,
/// on its first block. `true` when it took; refused (no `RLIMIT_RTPRIO`,
/// no `audio` group), it says so once and the thread stays as it was.
#[must_use]
pub fn promote_current_thread() -> bool {
    promote_current_thread_at(AUDIO_PRIORITY)
}

/// [`promote_current_thread`] at `priority` ([`AHEAD_PRIORITY`] for a
/// thread rendering ahead).
#[must_use]
pub fn promote_current_thread_at(priority: i32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let param = libc::sched_param {
            sched_priority: priority,
        };
        // SAFETY: pid 0 is the calling thread; `param` outlives the call,
        // which only reads it.
        let result = unsafe {
            libc::sched_setscheduler(
                0,
                libc::SCHED_FIFO | libc::SCHED_RESET_ON_FORK,
                &raw const param,
            )
        };
        if result == 0 {
            tracing::info!(priority, "daw-audio-io: audio thread is realtime");
            true
        } else {
            let error = std::io::Error::last_os_error();
            tracing::warn!(%error, "daw-audio-io: audio thread could not go realtime; it may be pre-empted (clicks under load)");
            false
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = priority;
        false
    }
}
