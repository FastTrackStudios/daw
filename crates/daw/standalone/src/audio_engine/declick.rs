//! The declick around a jump of the playhead while playing (a locate, a
//! seek), and the stale start of play after a stop: what the callback's
//! render ([`DualRender::render`](super::ahead::DualRender::render)) does
//! to each block, rendered whole or ahead.
//!
//! What is heard is the song's latency (delay compensation, `render::pdc`)
//! behind what is rendered. After a jump, that latency's worth of the old
//! place still comes out of the delays and the latent plugins — the old
//! place playing on, every track aligned — and the new place is heard from
//! the jump plus the latency: the *splice*. The fade goes there: the old
//! place faded out over the [`DECLICK_FRAMES`] before the splice, the new
//! one faded in over those after. A latency shorter than the fade leaves
//! too little of the old place to fade out, so the old place is rendered
//! on for the rest, the new one from as far past its target (Ardour's
//! `DeclickToLocate`, which also plays on to fade out): the new place is
//! heard when it would have been, its first frames under the fade.
//!
//! After a stop, the delays and the latent plugins still hold where it
//! stopped, and the callback rendered nothing since. Play mutes the
//! latency's worth of that and fades in from the splice — silent where
//! Ardour's latency pre-roll is silent.
//!
//! A loop's wrap is not a jump (the transport foresees it). Lock- and
//! allocation-free: the callback runs it every block.

use crate::transport_engine::{BlockPlan, BlockSegment};

/// How long a declick's fade lasts, each way, frames (Ardour's
/// `FADE_LEN`).
pub(crate) const DECLICK_FRAMES: usize = 128;

/// One declick: the gain is `from` until `down`, falls to nothing at
/// `splice`, and is back at one [`DECLICK_FRAMES`] later. Frames heard.
#[derive(Clone, Copy, Debug)]
struct Fade {
    down: u64,
    splice: u64,
    from: f32,
}

impl Fade {
    /// The gain at heard frame `t`; `None` once the fade is over.
    fn gain(&self, t: u64) -> Option<f32> {
        if t < self.down {
            Some(self.from)
        } else if t < self.splice {
            let left = (self.splice - t) as f32 / (self.splice - self.down) as f32;
            Some(self.from * left)
        } else if t < self.splice + DECLICK_FRAMES as u64 {
            Some((t - self.splice) as f32 / DECLICK_FRAMES as f32)
        } else {
            None
        }
    }
}

/// What a block does to the playhead.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    /// Plays on (or does not play).
    On,
    /// Jumps at buffer frame `at`, from `from` on the timeline at `rate`.
    Jump { at: usize, from: f64, rate: f64 },
    /// Starts playing at buffer frame `at`, after a stop.
    Start { at: usize },
}

/// The callback's declick state, block to block.
#[derive(Debug, Default)]
pub(crate) struct Declick {
    /// Frames heard so far: the clock the fades are on.
    heard: u64,
    /// Where this block was heard from.
    block: u64,
    /// Where the next block plays on from (timeline, rate), when it does.
    next: Option<(f64, f64)>,
    fade: Option<Fade>,
}

impl Declick {
    /// Nothing played (stopped): play starts again from silence.
    pub(crate) fn rest(&mut self) {
        self.next = None;
        self.fade = None;
    }

    /// The block to render for `plan` — the plan itself, or one that plays
    /// the old place on past a jump to fade it out — with `latency` the
    /// song's (frames heard behind rendered) and `wrap` the loop the
    /// transport wraps at.
    pub(crate) fn plan(
        &mut self,
        plan: &BlockPlan,
        latency: usize,
        wrap: Option<(f64, f64)>,
    ) -> BlockPlan {
        let segs = plan.segments();
        let n: usize = segs.iter().map(|s| s.frames).sum();
        let moved = match segs {
            [s] if s.playing => match self.next {
                None => Move::Start { at: 0 },
                Some((from, rate)) if (s.start_samples - from).abs() > 0.5 => {
                    Move::Jump { at: 0, from, rate }
                }
                Some(_) => Move::On,
            },
            [a, b] if b.playing && a.playing => Move::Jump {
                at: b.offset,
                from: (a.frames as f64).mul_add(a.rate, a.start_samples),
                rate: a.rate,
            },
            [a, b] if b.playing && !a.playing => Move::Start { at: b.offset },
            _ => Move::On,
        };
        self.next = segs.last().filter(|s| s.playing).map(|s| {
            let end = (s.frames as f64).mul_add(s.rate, s.start_samples);
            (wrapped(end, wrap), s.rate)
        });
        self.block = self.heard;
        self.heard += n as u64;
        let t = self.block;
        match moved {
            Move::Jump { at, from, rate } => {
                // The old place rendered on for what the latency does not
                // leave of it to fade.
                let on = DECLICK_FRAMES.saturating_sub(latency).min(n - at);
                let splice = t + (at + on + latency) as u64;
                let down = splice
                    .saturating_sub(DECLICK_FRAMES as u64)
                    .max(t + at as u64);
                let from_gain = self.gain_at(t + at as u64);
                self.fade = Some(Fade {
                    down,
                    splice,
                    from: from_gain,
                });
                if on == 0 {
                    return *plan;
                }
                let new = segs[segs.len() - 1];
                let old = match segs {
                    [a, _] => BlockSegment {
                        frames: at + on,
                        ..*a
                    },
                    _ => BlockSegment {
                        offset: 0,
                        frames: on,
                        start_samples: from,
                        rate,
                        playing: true,
                    },
                };
                if at + on == n {
                    BlockPlan::single(old)
                } else {
                    BlockPlan::split(
                        old,
                        BlockSegment {
                            offset: at + on,
                            frames: n - at - on,
                            start_samples: (on as f64).mul_add(new.rate, new.start_samples),
                            ..new
                        },
                    )
                }
            }
            Move::Start { at } if latency > 0 => {
                let begin = t + at as u64;
                self.fade = Some(Fade {
                    down: begin,
                    splice: begin + latency as u64,
                    from: 0.0,
                });
                *plan
            }
            _ => *plan,
        }
    }

    /// The gain a fade under way has at heard frame `t` (one without).
    fn gain_at(&self, t: u64) -> f32 {
        self.fade.and_then(|f| f.gain(t)).unwrap_or(1.0)
    }

    /// The block rendered for the last [`plan`](Self::plan), interleaved
    /// stereo: faded where a fade is.
    pub(crate) fn apply(&mut self, samples: &mut [f32]) {
        let Some(fade) = self.fade else {
            return;
        };
        for (f, frame) in samples.chunks_exact_mut(2).enumerate() {
            match fade.gain(self.block + f as u64) {
                Some(g) => {
                    frame[0] *= g;
                    frame[1] *= g;
                }
                None => {
                    self.fade = None;
                    return;
                }
            }
        }
    }
}

/// `at` on the timeline, wrapped as the transport wraps it at the loop
/// `wrap` (start, end).
fn wrapped(at: f64, wrap: Option<(f64, f64)>) -> f64 {
    match wrap {
        Some((start, end)) if end > start && at >= end => {
            start + (at - start).rem_euclid(end - start)
        }
        _ => at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(offset: usize, frames: usize, start: f64) -> BlockSegment {
        BlockSegment {
            offset,
            frames,
            start_samples: start,
            rate: 1.0,
            playing: true,
        }
    }

    #[test]
    fn playing_on_and_a_loop_wrap_are_not_jumps() {
        let mut d = Declick::default();
        let latency = 0;
        // A loop of 0..200: the transport wraps the third block to 56.
        let wrap = Some((0.0, 200.0));
        // Play starts with no latency: nothing to fade.
        for start in [0.0, 128.0, 56.0, 184.0] {
            let p = BlockPlan::single(seg(0, 128, start));
            assert_eq!(d.plan(&p, latency, wrap), p, "from {start}");
            assert!(d.fade.is_none(), "from {start}");
        }
    }

    #[test]
    fn a_jump_with_little_latency_plays_the_old_place_on_to_fade_it() {
        let mut d = Declick::default();
        let _ = d.plan(&BlockPlan::single(seg(0, 256, 0.0)), 28, None);
        let jumped = d.plan(&BlockPlan::single(seg(0, 256, 10_000.0)), 28, None);
        assert_eq!(
            jumped.segments(),
            &[seg(0, 100, 256.0), seg(100, 156, 10_100.0)],
            "the old place for the fade the latency leaves; the new one as far in"
        );
        let mut out = vec![1.0; 512];
        d.apply(&mut out);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert_eq!(left[0], 1.0);
        assert_eq!(left[128], 0.0, "silent at the splice");
        assert!(left[64] > 0.4 && left[64] < 0.6);
        assert_eq!(left[255], 127.0 / 128.0);
    }

    #[test]
    fn a_jump_with_latency_enough_fades_where_it_is_heard() {
        let mut d = Declick::default();
        let _ = d.plan(&BlockPlan::single(seg(0, 128, 0.0)), 500, None);
        // Play started muted for the latency: past it, the gain is one.
        let mut out = vec![1.0; 256];
        d.apply(&mut out);
        assert_eq!(out[0], 0.0);
        for b in 1..6 {
            let p = BlockPlan::single(seg(0, 128, (b * 128) as f64));
            let _ = d.plan(&p, 500, None);
            d.apply(&mut vec![1.0; 256]);
        }
        let p = BlockPlan::single(seg(0, 128, 50_000.0));
        assert_eq!(d.plan(&p, 500, None), p, "rendered as planned");
        let fade = d.fade.expect("a fade");
        let jump = 6 * 128;
        assert_eq!(
            (fade.down, fade.splice),
            (jump + 500 - 128, jump + 500),
            "where the jump is heard"
        );
    }
}
