//! How long play takes to be heard: from the transport told to play to the
//! first block a live engine plays that is not silence.
//!
//! [`arm`] is called where play is asked for; every live engine's render
//! ([`super::ahead::DualRender::render`]) hands its block to [`observe`],
//! which costs one atomic load a block while nothing waits to be heard.
//! The first block above [`THRESHOLD`] while the transport rolls stamps the
//! time; [`watch`] waits for that stamp on a thread of its own and closes
//! the `transport.play` span with `play.first_audio_ms` — the audio thread
//! never touches tracing. The span also says how many plugin runs were
//! bypassed while it waited: refused as too big a block
//! (`play.plugin_too_large`) or the plugin busy (`play.plugin_busy`).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A sample above this (−100 dBFS) is heard.
pub const THRESHOLD: f32 = 1e-5;

/// How long a play is watched for its first audio before it is called
/// silent.
const WATCH_FOR: Duration = Duration::from_secs(30);

/// When the waiting play was asked for, ns since [`epoch`]; 0 when none
/// waits.
static ARMED_AT: AtomicU64 = AtomicU64::new(0);
/// When the latest play was asked for, ns since [`epoch`]: what its first
/// audio is timed from (a play while one is watched re-arms it).
static ASKED_AT: AtomicU64 = AtomicU64::new(0);
/// When it was first heard, ns since [`epoch`]; 0 until it is.
static HEARD_AT: AtomicU64 = AtomicU64::new(0);
/// A watcher is running.
static WATCHING: AtomicBool = AtomicBool::new(false);

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_ns() -> u64 {
    // At least 1: 0 means "none".
    u64::try_from(epoch().elapsed().as_nanos())
        .unwrap_or(u64::MAX)
        .max(1)
}

/// Play was asked for now: the next audible block is its first audio.
pub fn arm() {
    let at = now_ns();
    HEARD_AT.store(0, Ordering::Relaxed);
    ASKED_AT.store(at, Ordering::Release);
    ARMED_AT.store(at, Ordering::Release);
}

/// A live engine's block, interleaved, rendered while `rolling` says the
/// transport plays through it. Realtime-safe: an atomic load, and only
/// while a play waits, a scan of the block.
pub fn observe(samples: &[f32], rolling: bool) {
    if !rolling || ARMED_AT.load(Ordering::Acquire) == 0 {
        return;
    }
    if samples.iter().any(|s| s.abs() > THRESHOLD) {
        HEARD_AT.store(now_ns(), Ordering::Release);
        ARMED_AT.store(0, Ordering::Release);
    }
}

/// What a play's wait came to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Heard {
    /// Heard this long after play was asked for.
    After(Duration),
    /// The transport stopped first, or nothing was heard in time.
    Not,
}

/// Wait for the armed play to be heard while `still_playing` says it
/// plays, polling every `poll`, at most `limit`.
fn wait(still_playing: &dyn Fn() -> bool, poll: Duration, limit: Duration) -> Heard {
    let began = Instant::now();
    loop {
        let heard = HEARD_AT.load(Ordering::Acquire);
        if heard != 0 {
            let asked = ASKED_AT.load(Ordering::Acquire);
            return Heard::After(Duration::from_nanos(heard.saturating_sub(asked)));
        }
        if began.elapsed() > limit || !still_playing() {
            ARMED_AT.store(0, Ordering::Release);
            return Heard::Not;
        }
        std::thread::sleep(poll);
    }
}

/// [`arm`], then on a thread of its own wait for the first audio and close
/// the `transport.play` span with it: `play.first_audio_ms`, `play.heard`.
/// One play is watched at a time; a play while one is watched re-arms it.
pub fn watch(still_playing: Box<dyn Fn() -> bool + Send>) {
    arm();
    if WATCHING.swap(true, Ordering::AcqRel) {
        return;
    }
    let span = tracing::info_span!(
        "transport.play",
        play.first_audio_ms = tracing::field::Empty,
        play.heard = tracing::field::Empty,
        play.plugin_too_large = tracing::field::Empty,
        play.plugin_busy = tracing::field::Empty,
    );
    let too_large = super::render::plugin_too_large_skips();
    let busy = super::render::plugin_busy_skips();
    let spawned = std::thread::Builder::new()
        .name("play-heard".into())
        .spawn(move || {
            let _in = span.enter();
            let heard = wait(&*still_playing, Duration::from_millis(1), WATCH_FOR);
            match heard {
                Heard::After(d) => {
                    span.record("play.first_audio_ms", d.as_secs_f64() * 1e3);
                    span.record("play.heard", true);
                }
                Heard::Not => {
                    span.record("play.heard", false);
                }
            }
            span.record(
                "play.plugin_too_large",
                super::render::plugin_too_large_skips().saturating_sub(too_large),
            );
            span.record(
                "play.plugin_busy",
                super::render::plugin_busy_skips().saturating_sub(busy),
            );
            if super::render::plugin_too_large_skips() > too_large {
                tracing::warn!(
                    play.plugin_too_large = super::render::plugin_too_large_skips() - too_large,
                    "plugins bypassed: a block bigger than they were activated for"
                );
            }
            WATCHING.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        WATCHING.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_play_is_heard_at_its_first_loud_block_while_rolling() {
        arm();
        observe(&[0.0; 64], true);
        observe(&[0.5; 64], false);
        assert_eq!(HEARD_AT.load(Ordering::Acquire), 0);
        observe(&[0.0, 0.0, 0.25, 0.0], true);
        let heard = wait(&|| true, Duration::from_millis(1), WATCH_FOR);
        assert!(matches!(heard, Heard::After(_)));
        // Disarmed: a later block stamps nothing.
        let at = HEARD_AT.load(Ordering::Acquire);
        observe(&[0.5; 4], true);
        assert_eq!(HEARD_AT.load(Ordering::Acquire), at);
    }
}
