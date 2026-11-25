//! Plugin delay compensation: every sum in the routing graph — a folder's
//! children, a bus's sends, master — hears its inputs aligned, however
//! much latency each one's plugins report.
//!
//! Each track's chain latency is the sum of what its running plugins
//! report ([`PluginInstance::latency`](crate::plugin::PluginInstance::latency)),
//! measured as its chain runs and kept in a [`ChainLatencies`] table — one
//! per song, shared by a dual render's two renderers so both plan alike.
//! At the start of a block, [`plan`] walks the graph in its processing
//! order: a track's input arrives at the latest of what feeds it, its
//! output that plus its chain's latency, and every edge into a sum (its
//! parent or master, each send) is delayed by the difference between
//! that sum's input latency and what it taps. A track's own items, when
//! something feeding it arrives late, are played that much later too.
//! What reaches master is late by [`Plan::master`] samples: the song's
//! latency, which an offline render takes off the front.
//!
//! The plan uses the latencies measured by the block before — a latency
//! that changes (a state loaded, a plugin switched on) is followed a block
//! later. Delay lines allocate only when an edge's delay changes, and
//! change the way Ardour's do (`DelayLine::run`): longer, what is in the
//! line plays out, its end faded, then silence and the input faded in;
//! shorter, the old line crossfades into the new — never a cut.

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

use super::snapshot::TrackSnapshot;

/// The most tracks a song's latencies are kept for; a track past it is
/// treated as having none.
const MOST_TRACKS: usize = 4_096;

/// Each track's chain latency, samples, by track index: written by the
/// render that runs the chain, read by every render's [`plan`]. Lock-free.
pub(crate) struct ChainLatencies(Box<[AtomicU32]>);

impl Default for ChainLatencies {
    fn default() -> Self {
        Self((0..MOST_TRACKS).map(|_| AtomicU32::new(0)).collect())
    }
}

impl ChainLatencies {
    pub(crate) fn get(&self, track: usize) -> u32 {
        self.0.get(track).map_or(0, |a| a.load(Relaxed))
    }

    pub(crate) fn set(&self, track: usize, samples: u32) {
        if let Some(a) = self.0.get(track) {
            a.store(samples, Relaxed);
        }
    }
}

/// One block's delay compensation: each track's input and output latency
/// and the delay on every edge out of it.
#[derive(Default)]
pub(crate) struct Plan {
    /// When a track's input arrives, samples late.
    pub(crate) input: Vec<u32>,
    /// When its output does: its input plus its chain.
    pub(crate) output: Vec<u32>,
    /// Per track, the delay on its edge to its parent (or master): edge 0;
    /// then one per send, in its send order.
    pub(crate) edges: Vec<Vec<u32>>,
    /// Per track, the delay its output takes to master directly (a soloed
    /// track heard with no way there of its own).
    pub(crate) to_master: Vec<u32>,
    /// What reaches master is this late.
    pub(crate) master: u32,
}

impl Plan {
    /// Whether anything in the song is delayed at all.
    pub(crate) fn any(&self) -> bool {
        self.master > 0 || self.input.iter().any(|&l| l > 0)
    }
}

/// Where a send taps its track: its input (pre-FX) or its output.
fn send_tap(send: &super::snapshot::SendSnapshot, input: u32, output: u32) -> u32 {
    if send.mode == daw_proto::routing::SendMode::PreFx && send.from_pair == 0 {
        input
    } else {
        output
    }
}

/// Plan the block: `latency(ti)` each track's chain latency, `order` the
/// snapshot's processing order (a track after all that feed it).
pub(crate) fn plan(
    tracks: &[TrackSnapshot],
    order: &[usize],
    latency: impl Fn(usize) -> u32,
    out: &mut Plan,
) {
    let n = tracks.len();
    out.input.clear();
    out.input.resize(n, 0);
    out.output.clear();
    out.output.resize(n, 0);
    out.to_master.clear();
    out.to_master.resize(n, 0);
    out.edges.resize_with(n, Vec::new);
    out.master = 0;
    // When each sum's inputs arrive: the latest of them.
    for &ti in order {
        let t = &tracks[ti];
        // A track played back from a recording is as late as the recording
        // was: what feeds it isn't heard (`TrackSnapshot::replayed`).
        let output = if t.replayed {
            latency(ti)
        } else {
            out.input[ti].saturating_add(latency(ti))
        };
        out.output[ti] = output;
        match t.parent_idx.filter(|_| t.parent_send) {
            Some(p) => out.input[p] = out.input[p].max(output),
            None => out.master = out.master.max(output),
        }
        for s in t.sends.iter().filter(|s| !s.muted) {
            if let Some(d) = s.dest_idx.filter(|&d| d < n) {
                let tap = send_tap(s, out.input[ti], output);
                out.input[d] = out.input[d].max(tap);
            }
        }
    }
    // Every edge waits for the latest of its sum's inputs.
    for (ti, t) in tracks.iter().enumerate() {
        let (input, output) = (out.input[ti], out.output[ti]);
        let edges = &mut out.edges[ti];
        edges.clear();
        edges.push(match t.parent_idx.filter(|_| t.parent_send) {
            Some(p) => out.input[p].saturating_sub(output),
            None => out.master.saturating_sub(output),
        });
        for s in &t.sends {
            edges.push(match s.dest_idx.filter(|&d| d < n && !s.muted) {
                Some(d) => out.input[d].saturating_sub(send_tap(s, input, output)),
                None => 0,
            });
        }
        out.to_master[ti] = out.master.saturating_sub(output);
    }
}

/// How long a delay that changes fades, frames (Ardour's `FADE_LEN`).
const FADE: usize = 128;

/// A stereo delay line (interleaved), as long as its edge's delay.
#[derive(Default)]
pub(crate) struct Delay {
    buf: Vec<f32>,
    pos: usize,
    /// Frames of signal still inside it: it must keep running (on silence)
    /// until they are out, even on a block its track has nothing.
    hot: usize,
}

impl Delay {
    /// Whether signal is still inside it.
    pub(crate) fn hot(&self) -> bool {
        self.hot > 0
    }

    /// Whether it must run even at no delay: it holds a line (the delay it
    /// last ran at), to fade out of when the delay goes.
    pub(crate) fn engaged(&self) -> bool {
        self.hot > 0 || !self.buf.is_empty()
    }

    /// `src`'s first `frames` frames, `delay` frames late, into `out`.
    /// A delay that changed fades from the old line to the new
    /// ([`change`](Self::change)); it allocates then, and only then.
    pub(crate) fn run(&mut self, delay: usize, src: &[f32], out: &mut [f32], frames: usize) {
        let len = frames * 2;
        let was = self.buf.len() / 2;
        if delay != was {
            self.change(was, delay, src, out, frames);
            return;
        }
        if delay == 0 {
            out[..len].copy_from_slice(&src[..len]);
            return;
        }
        let silent = src[..len].iter().all(|s| *s == 0.0);
        if silent && self.hot == 0 {
            out[..len].fill(0.0);
            return;
        }
        // Out comes what went in `delay` frames ago, in comes `src`: a run
        // of frames at a time up to the ring's end.
        let mut done = 0;
        while done < len {
            let at = self.pos * 2;
            let run = (delay * 2 - at).min(len - done);
            out[done..done + run].copy_from_slice(&self.buf[at..at + run]);
            self.buf[at..at + run].copy_from_slice(&src[done..done + run]);
            done += run;
            self.pos += run / 2;
            if self.pos == delay {
                self.pos = 0;
            }
        }
        self.hot = if silent {
            self.hot.saturating_sub(frames)
        } else {
            delay
        };
    }

    /// The block the delay changes in, from `was` frames to `delay`.
    /// Longer: what is in the line plays out (with the input's first
    /// frames, if the line is shorter than a fade), its end faded out,
    /// then silence, then the input faded in. Shorter: the old line
    /// crossfades into the new over the block's first frames, the frames
    /// between them never heard.
    fn change(&mut self, was: usize, delay: usize, src: &[f32], out: &mut [f32], frames: usize) {
        let fade = FADE.min(frames);
        // The old line, oldest frame first.
        let mut line = Vec::with_capacity(delay.max(was) * 2);
        line.extend_from_slice(&self.buf[self.pos * 2..]);
        line.extend_from_slice(&self.buf[..self.pos * 2]);
        let ramp = |k: usize, of: usize| (k + 1) as f32 / (of + 1) as f32;
        if delay > was {
            let add = fade.saturating_sub(was).min(delay - was);
            line.extend_from_slice(&src[..add * 2]);
            let n = line.len() / 2;
            let out_len = n.min(fade);
            for k in 0..out_len {
                let g = 1.0 - ramp(k, out_len);
                let f = n - out_len + k;
                line[f * 2] *= g;
                line[f * 2 + 1] *= g;
            }
            line.resize(delay * 2, 0.0);
            self.buf = line;
            self.pos = 0;
            for f in 0..frames {
                let g = if f < fade { ramp(f, fade) } else { 1.0 };
                let at = self.pos * 2;
                out[f * 2] = self.buf[at];
                out[f * 2 + 1] = self.buf[at + 1];
                self.buf[at] = src[f * 2] * g;
                self.buf[at + 1] = src[f * 2 + 1] * g;
                self.pos = (self.pos + 1) % delay;
            }
            self.hot = delay;
            return;
        }
        // Shorter: what the old line would have played over the fade.
        let mut from = [0.0_f32; FADE * 2];
        for f in 0..fade {
            let (l, r) = if f < was {
                (line[f * 2], line[f * 2 + 1])
            } else {
                (src[(f - was) * 2], src[(f - was) * 2 + 1])
            };
            from[f * 2] = l;
            from[f * 2 + 1] = r;
        }
        if delay == 0 {
            out[..frames * 2].copy_from_slice(&src[..frames * 2]);
            self.buf = Vec::new();
            self.pos = 0;
            self.hot = 0;
        } else {
            // The new line: the old one's newest `delay` frames.
            self.buf = line.split_off((was - delay) * 2);
            self.pos = 0;
            for f in 0..frames {
                let at = self.pos * 2;
                out[f * 2] = self.buf[at];
                out[f * 2 + 1] = self.buf[at + 1];
                self.buf[at] = src[f * 2];
                self.buf[at + 1] = src[f * 2 + 1];
                self.pos = (self.pos + 1) % delay;
            }
            self.hot = delay;
        }
        for f in 0..fade {
            let g = ramp(f, fade);
            out[f * 2] = (1.0 - g) * from[f * 2] + g * out[f * 2];
            out[f * 2 + 1] = (1.0 - g) * from[f * 2 + 1] + g * out[f * 2 + 1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line already at `delay` (run once on silence): a line's first run
    /// is a change from none.
    fn line(delay: usize) -> Delay {
        let mut d = Delay::default();
        d.run(delay, &[0.0; 8], &mut [0.0; 8], 4);
        d
    }

    #[test]
    fn a_delay_line_delays_by_its_length_across_blocks() {
        let mut d = line(6);
        let mut out = vec![0.0; 8];
        let impulse = [1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        d.run(6, &impulse, &mut out, 4);
        assert!(out.iter().all(|s| *s == 0.0));
        assert!(d.hot());
        d.run(6, &[0.0; 8], &mut out, 4);
        // Frame 6 overall = frame 2 of the second block.
        assert_eq!(&out[4..6], &[1.0, -1.0]);
        assert_eq!(out.iter().filter(|s| **s != 0.0).count(), 2);
    }

    #[test]
    fn a_delay_line_is_a_delay_whatever_its_length_against_the_block() {
        for delay in [1, 3, 4, 5, 64, 100] {
            let mut d = line(delay);
            let mut out = vec![0.0; 8];
            let mut heard = Vec::new();
            let signal: Vec<f32> = (1..=48).map(|i| i as f32).collect();
            for block in 0..12 {
                let src: Vec<f32> = signal
                    .chunks(2)
                    .cycle()
                    .skip(block * 4)
                    .take(4)
                    .flatten()
                    .copied()
                    .collect();
                d.run(delay, &src, &mut out, 4);
                heard.extend(out.chunks(2).map(|f| f[0]));
            }
            let sent: Vec<f32> = signal.chunks(2).cycle().take(48).map(|f| f[0]).collect();
            for (f, h) in heard.iter().enumerate() {
                let want = f.checked_sub(delay).map_or(0.0, |s| sent[s]);
                assert_eq!(*h, want, "delay {delay}, frame {f}");
            }
        }
    }

    #[test]
    fn a_line_with_nothing_in_it_goes_cold() {
        let mut d = Delay::default();
        let mut out = vec![0.0; 8];
        d.run(3, &[0.5; 8], &mut out, 4);
        d.run(3, &[0.0; 8], &mut out, 4);
        assert!(!d.hot());
    }
}
