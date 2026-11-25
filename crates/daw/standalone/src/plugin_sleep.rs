//! **A plugin asleep while it has nothing to do**: CLAP's process status
//! and tail, honoured by the renderer, live and offline alike (they share
//! the scheduler, so one rule holds for both).
//!
//! After every call the plugin says what it needs (its
//! [`ProcessStatus`]); before every block the renderer asks the plugin's
//! [`SleepState`] whether to call it at all. A plugin falls asleep only on
//! a block whose input was exact zeros (the bus never marked dirty, or
//! scanned to zero) and whose output was quiet, under one of:
//!
//! - [`ProcessStatus::Sleep`]: at once.
//! - [`ProcessStatus::Tail`]: once its input has been silent for its tail
//!   (`clap_plugin_tail`) plus its latency; never with an infinite one.
//! - [`ProcessStatus::ContinueIfNotQuiet`]: at once, but only from a plugin
//!   that declares [`EXT_SLEEP`] ("my quiet is honest": it says so only
//!   once nothing more can come, its own gaps held — Signal). Every other
//!   plugin's is taken as `Continue`: nice-plug says it of every block,
//!   delays with 2 s between echoes and generators that play from the
//!   transport alone (a guide's clicks and cues) among them, and nothing
//!   tells their gaps from their end.
//! - [`ProcessStatus::Continue`] (every backend's default): never, so a
//!   plugin that never asks keeps running exactly as before.
//!
//! `FTS_PLUGIN_SLEEP=0` in the environment turns it off (every plugin is
//! called every block, as before), for an A/B of a render.
//!
//! **Quiet** is every output sample at or under [`SILENCE`], -150 dBFS:
//! a NAM capture's bias, a filter's asymptotic decay and denormals never
//! reach exact zero. What a plugin put out under that level when it fell
//! asleep is the most a render with sleep differs from one without, at any
//! sample; the equivalence checks accept exactly that and no more.
//!
//! An asleep plugin is woken (called again, its clock where the song is)
//! by a non-zero input sample or sidechain, a note or note expression, a
//! parameter value or automation that moved ([`SleepState::skip`]'s key),
//! a control thread's touch, an open editor, or its own `request_process`.
//!
//! Realtime-safe: atomics only (written under the plugin's lock, so
//! `Relaxed` throughout), nothing allocated, nothing waited on.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// -150 dBFS: the loudest a sample may be and still be "quiet".
pub const SILENCE: f32 = 3.162_277_7e-8;

/// The extension a plugin answers (with any non-null pointer, by
/// convention a `u32` version) to say its `CONTINUE_IF_NOT_QUIET` is
/// honest: it may sleep the block it says so.
pub const EXT_SLEEP: &std::ffi::CStr = c"com.fasttrackstudio.sleep/1";

/// Whether `plugin` answers [`EXT_SLEEP`].
///
/// # Safety
///
/// `plugin` is a valid, initialized `clap_plugin`, on its main thread.
#[cfg(feature = "clap-host")]
#[allow(unsafe_code)]
#[must_use]
pub unsafe fn honest_quiet(plugin: *const clap_sys::plugin::clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    // SAFETY: the caller's.
    let Some(get) = (unsafe { (*plugin).get_extension }) else {
        return false;
    };
    // SAFETY: as above; the id is a valid C string.
    !unsafe { get(plugin, EXT_SLEEP.as_ptr()) }.is_null()
}

/// 0 not read yet, 1 on, 2 off (`FTS_PLUGIN_SLEEP=0`).
static ENABLED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Read `FTS_PLUGIN_SLEEP` once: on a control thread, where a cell is made.
fn read_enabled() {
    if ENABLED.load(Relaxed) == 0 {
        let off = std::env::var("FTS_PLUGIN_SLEEP").is_ok_and(|v| v.trim() == "0");
        ENABLED.store(if off { 2 } else { 1 }, Relaxed);
    }
}

/// Whether plugins may sleep in this process.
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Relaxed) != 2
}

/// What a plugin said of its last block (CLAP's `clap_process_status`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Call it every block (CLAP_PROCESS_CONTINUE).
    #[default]
    Continue,
    /// Call it unless its output is quiet (CLAP_PROCESS_CONTINUE_IF_NOT_QUIET).
    ContinueIfNotQuiet,
    /// Ringing out: ask its tail (CLAP_PROCESS_TAIL).
    Tail,
    /// Nothing to do until an event or input (CLAP_PROCESS_SLEEP).
    Sleep,
}

#[cfg(feature = "clap-host")]
impl From<clack_host::process::ProcessStatus> for ProcessStatus {
    fn from(s: clack_host::process::ProcessStatus) -> Self {
        use clack_host::process::ProcessStatus as Clap;
        match s {
            Clap::Continue => Self::Continue,
            Clap::ContinueIfNotQuiet => Self::ContinueIfNotQuiet,
            Clap::Tail => Self::Tail,
            Clap::Sleep => Self::Sleep,
        }
    }
}

/// How long a plugin rings after its input stops (`clap_plugin_tail`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    /// This many samples.
    Finite(u32),
    /// Forever: never put to sleep on its tail.
    Infinite,
}

/// What a block said about one plugin's audio, for its [`SleepState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    /// Its input (and sidechain) were exact zeros.
    pub input_silent: bool,
    /// It was sent notes or note expressions.
    pub events: bool,
    /// Its output was at or under [`SILENCE`] throughout.
    pub output_quiet: bool,
    pub frames: u32,
}

/// Whether a plugin that has heard silence for `silent_in` frames, and
/// silence while quiet for `quiet` frames, falls asleep after this block.
/// The rule of the module's docs.
#[must_use]
pub fn falls_asleep(
    status: ProcessStatus,
    heard: Heard,
    silent_in: u64,
    quiet: u64,
    tail: impl FnOnce() -> Tail,
    latency: u32,
) -> bool {
    if !heard.input_silent || heard.events || !heard.output_quiet {
        return false;
    }
    let latency = u64::from(latency);
    match status {
        ProcessStatus::Continue => false,
        ProcessStatus::Sleep => true,
        ProcessStatus::Tail => match tail() {
            Tail::Finite(t) => silent_in >= u64::from(t) + latency,
            Tail::Infinite => false,
        },
        ProcessStatus::ContinueIfNotQuiet => quiet >= latency,
    }
}

/// Whether `l` and `r` are exact zeros (`-0.0` too), and whether they are
/// quiet ([`SILENCE`]). A NaN is neither.
#[must_use]
pub fn silence(l: &[f32], r: &[f32]) -> (bool, bool) {
    let (mut zero, mut quiet) = (true, true);
    for s in l.iter().chain(r) {
        zero &= *s == 0.0;
        quiet &= s.abs() <= SILENCE;
    }
    (zero, quiet)
}

/// One plugin instance's sleep, beside it in its cell
/// ([`crate::plugin::PluginCell::sleep`]).
#[derive(Debug)]
pub struct SleepState {
    asleep: AtomicBool,
    /// Frames since its input was last non-silent (or it was sent a note).
    silent_in: AtomicU64,
    /// Frames since its input was non-silent or its output loud.
    quiet: AtomicU64,
    /// What it was sent when it fell asleep ([`SleepState::skip`]'s key).
    key: AtomicU64,
    /// Blocks it ran and their time, µs: what a skipped block saved.
    calls: AtomicU64,
    call_us: AtomicU64,
    /// Blocks it was not called for.
    skipped: AtomicU64,
}

impl Default for SleepState {
    /// Made on a control thread, with its cell: where `FTS_PLUGIN_SLEEP`
    /// is read.
    fn default() -> Self {
        read_enabled();
        Self {
            asleep: AtomicBool::new(false),
            silent_in: AtomicU64::new(0),
            quiet: AtomicU64::new(0),
            key: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            call_us: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
        }
    }
}

/// What [`SleepState::skip`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Call it.
    Run,
    /// Not called: its output is silence. About this many µs saved (its
    /// average awake block).
    Asleep { saved_us: u32 },
}

impl SleepState {
    /// Before a block: whether to call the plugin. An asleep one stays so
    /// while its input is silent, it is sent no note, and `key` (everything
    /// else that wakes it: its parameter values and automation, a control
    /// thread's touch, its editor, its `request_process`) is the one it
    /// fell asleep with.
    pub fn skip(&self, input_silent: bool, events: bool, key: u64) -> Skip {
        if !self.asleep.load(Relaxed) {
            return Skip::Run;
        }
        if input_silent && !events && key == self.key.load(Relaxed) {
            self.skipped.fetch_add(1, Relaxed);
            return Skip::Asleep {
                saved_us: self.average_us(),
            };
        }
        self.asleep.store(false, Relaxed);
        self.silent_in.store(0, Relaxed);
        self.quiet.store(0, Relaxed);
        Skip::Run
    }

    /// After a block it ran (`us` long): its counters moved, and asleep if
    /// [`falls_asleep`] says so, keyed by `key`. Whether it fell asleep.
    #[allow(clippy::too_many_arguments)]
    pub fn ran(
        &self,
        status: ProcessStatus,
        heard: Heard,
        tail: impl FnOnce() -> Tail,
        latency: u32,
        key: u64,
        us: u32,
    ) -> bool {
        self.calls.fetch_add(1, Relaxed);
        self.call_us.fetch_add(u64::from(us), Relaxed);
        let frames = u64::from(heard.frames);
        let silent_in = if heard.input_silent && !heard.events {
            self.silent_in.load(Relaxed).saturating_add(frames)
        } else {
            0
        };
        let quiet = if heard.input_silent && !heard.events && heard.output_quiet {
            self.quiet.load(Relaxed).saturating_add(frames)
        } else {
            0
        };
        self.silent_in.store(silent_in, Relaxed);
        self.quiet.store(quiet, Relaxed);
        let asleep = enabled() && falls_asleep(status, heard, silent_in, quiet, tail, latency);
        if asleep {
            self.key.store(key, Relaxed);
            self.asleep.store(true, Relaxed);
        }
        asleep
    }

    /// Asleep now.
    #[must_use]
    pub fn is_asleep(&self) -> bool {
        self.asleep.load(Relaxed)
    }

    /// Blocks not called for, so far.
    #[must_use]
    pub fn skipped(&self) -> u64 {
        self.skipped.load(Relaxed)
    }

    fn average_us(&self) -> u32 {
        let calls = self.calls.load(Relaxed);
        if calls == 0 {
            return 0;
        }
        u32::try_from(self.call_us.load(Relaxed) / calls).unwrap_or(u32::MAX)
    }
}

/// FNV-1a over `words`, from `seed`: a [`SleepState::skip`] key.
#[must_use]
pub fn key_of(seed: u64, words: impl IntoIterator<Item = u64>) -> u64 {
    let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
    for w in words {
        h ^= w;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(input_silent: bool, output_quiet: bool) -> Heard {
        Heard {
            input_silent,
            events: false,
            output_quiet,
            frames: 512,
        }
    }

    #[test]
    fn continue_never_sleeps_and_sleep_sleeps_only_on_silence_in_and_quiet_out() {
        let fin = || Tail::Finite(0);
        assert!(!falls_asleep(
            ProcessStatus::Continue,
            heard(true, true),
            u64::MAX,
            u64::MAX,
            fin,
            0
        ));
        assert!(falls_asleep(
            ProcessStatus::Sleep,
            heard(true, true),
            0,
            0,
            fin,
            0
        ));
        assert!(!falls_asleep(
            ProcessStatus::Sleep,
            heard(false, true),
            0,
            0,
            fin,
            0
        ));
        assert!(!falls_asleep(
            ProcessStatus::Sleep,
            heard(true, false),
            0,
            0,
            fin,
            0
        ));
        let note = Heard {
            events: true,
            ..heard(true, true)
        };
        assert!(!falls_asleep(ProcessStatus::Sleep, note, 0, 0, fin, 0));
    }

    #[test]
    fn a_tail_runs_out_past_its_length_and_latency_and_an_infinite_one_never() {
        let tail = || Tail::Finite(48_000);
        let h = heard(true, true);
        assert!(!falls_asleep(
            ProcessStatus::Tail,
            h,
            48_000 + 63,
            0,
            tail,
            64
        ));
        assert!(falls_asleep(
            ProcessStatus::Tail,
            h,
            48_000 + 64,
            0,
            tail,
            64
        ));
        assert!(!falls_asleep(
            ProcessStatus::Tail,
            h,
            u64::MAX,
            0,
            || Tail::Infinite,
            0
        ));
    }

    #[test]
    fn an_honest_quiet_sleeps_once_quiet_past_its_latency() {
        let fin = || Tail::Finite(0);
        let h = heard(true, true);
        assert!(!falls_asleep(
            ProcessStatus::ContinueIfNotQuiet,
            h,
            512,
            511,
            fin,
            512
        ));
        assert!(falls_asleep(
            ProcessStatus::ContinueIfNotQuiet,
            h,
            512,
            512,
            fin,
            512
        ));
        assert!(!falls_asleep(
            ProcessStatus::ContinueIfNotQuiet,
            heard(true, false),
            512,
            0,
            fin,
            0
        ));
    }

    #[test]
    fn silence_is_exact_zero_and_quiet_is_under_minus_150_dbfs_and_nan_is_neither() {
        assert_eq!(silence(&[0.0, -0.0], &[0.0]), (true, true));
        assert_eq!(silence(&[0.0, 3e-8], &[0.0]), (false, true));
        assert_eq!(silence(&[0.0, 4e-8], &[0.0]), (false, false));
        assert_eq!(silence(&[f32::NAN], &[0.0]), (false, false));
    }

    #[test]
    fn asleep_until_input_a_note_or_the_key_moves() {
        let s = SleepState::default();
        let fin = || Tail::Finite(0);
        assert_eq!(s.skip(true, false, 7), Skip::Run);
        assert!(s.ran(ProcessStatus::Sleep, heard(true, true), fin, 0, 7, 100));
        assert!(matches!(
            s.skip(true, false, 7),
            Skip::Asleep { saved_us: 100 }
        ));
        assert!(matches!(s.skip(true, false, 7), Skip::Asleep { .. }));
        assert_eq!(s.skipped(), 2);
        // A parameter moved: woken.
        assert_eq!(s.skip(true, false, 8), Skip::Run);
        assert!(!s.is_asleep());
        assert!(s.ran(ProcessStatus::Sleep, heard(true, true), fin, 0, 8, 100));
        assert_eq!(s.skip(true, true, 8), Skip::Run, "a note wakes it");
        assert!(s.ran(ProcessStatus::Sleep, heard(true, true), fin, 0, 8, 100));
        assert_eq!(s.skip(false, false, 8), Skip::Run, "input wakes it");
    }

    #[test]
    fn a_tail_counts_from_the_last_non_silent_input() {
        let s = SleepState::default();
        let tail = || Tail::Finite(1024);
        // Input, then silence: 512, 1024 frames in — asleep at the second.
        assert!(!s.ran(ProcessStatus::Tail, heard(false, false), tail, 0, 0, 1));
        assert!(!s.ran(ProcessStatus::Tail, heard(true, true), tail, 0, 0, 1));
        assert!(s.ran(ProcessStatus::Tail, heard(true, true), tail, 0, 0, 1));
    }
}
