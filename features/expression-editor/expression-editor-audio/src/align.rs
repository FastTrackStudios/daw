//! Aligning one performance to another.
//!
//! The VocAlign job: a doubled vocal, a stacked harmony, or a second
//! take that should sit exactly on the first. Pick a **reference**,
//! pick a **dub**, and the dub is retimed to match.
//!
//! Clean-room. mpl's `Align takes` establishes the *shape* of the
//! feature — reference, dub, and a non-destructive time map written
//! into the dub — and that repository carries no licence, so nothing
//! comes from it but the description. The matching below is ours, and
//! it can afford to be: [`crate::Analysis`] already gives per-frame
//! pitch, level and voiced state, which is a richer feature set than a
//! script reaching through ReaScript can cheaply get.
//!
//! ## Why this is the timing feature, not a new one
//!
//! Alignment produces a map from dub time to reference time. That is
//! exactly what a timing drag produces, and it is written the same way
//! — as `WarpMarker`s that `render_world_warped` honours. So an
//! alignment lands in the editor as ordinary timing edits the user can
//! then adjust by hand, rather than as an opaque process.
//!
//! ## The matching
//!
//! Banded dynamic time warping over per-frame features. Three choices
//! carry most of the quality:
//!
//! - **Energy is the primary feature.** Two takes of the same line
//!   agree far more reliably on *when the syllables are* than on
//!   pitch — a harmony part is deliberately at a different pitch, and
//!   aligning it to the lead must still work.
//! - **Silence is cheap to stretch.** Frames where neither take is
//!   sounding cost almost nothing to insert or delete, so the slack
//!   between phrases absorbs the difference and the phrases themselves
//!   are left close to their own natural rhythm. Without this a
//!   long breath in one take drags the whole alignment.
//! - **The band is a hard limit on how far material may move.** A
//!   global DTW is free to align the end of one phrase to the start of
//!   another; a band says "the same moment is within a second or so",
//!   which is true of two takes of the same part and false of
//!   everything the algorithm would otherwise be tempted by.

use tune_dsp::model::WarpMarker;

use crate::Analysis;

/// How the two takes are matched.
#[derive(Clone, Copy, Debug)]
pub struct AlignConfig {
    /// Widest correction, in seconds — a hard cap on how far any
    /// material may move, enforced on the finished map.
    ///
    /// Two takes of the same part agree to within a beat or so; letting
    /// the match wander further finds better *scores* and worse
    /// alignments. Note this is a limit on the *result*, not just on
    /// the search: the search band is around the length-scaled
    /// diagonal, so that two takes of different total length can match
    /// at all, and a shift within that band can still be larger than
    /// the user asked to allow.
    pub max_shift_secs: f64,
    /// Preference for not moving, per band-width of correction.
    ///
    /// Small, and it earns its place in exactly the situation where
    /// nothing else has an opinion: silence pairs at zero cost, so
    /// without a tiebreak the path through a gap is arbitrary and the
    /// map wanders. With no evidence, do nothing.
    pub diagonal_bias: f64,
    /// Weight on the level difference. The primary cue.
    pub energy_weight: f64,
    /// Weight on pitch distance, applied only where both takes are
    /// voiced.
    ///
    /// Low by default so a harmony aligns to a lead: the parts are
    /// deliberately at different pitches and only their timing should
    /// match.
    pub pitch_weight: f64,
    /// Penalty for matching a voiced frame to an unvoiced one.
    ///
    /// The strongest single cue in a vocal is where the consonants
    /// are, and this is what makes them line up.
    pub voicing_penalty: f64,
    /// Below this normalized level a frame counts as silence, and
    /// stretching it is nearly free.
    pub silence_level: f64,
    /// How strongly the result is applied. 1.0 lands exactly on the
    /// reference; less leaves some of the dub's own feel.
    pub strength: f64,
}

impl Default for AlignConfig {
    fn default() -> Self {
        Self {
            max_shift_secs: 0.5,
            energy_weight: 1.0,
            pitch_weight: 0.15,
            voicing_penalty: 0.6,
            diagonal_bias: 0.02,
            silence_level: 0.02,
            strength: 1.0,
        }
    }
}

/// One frame's alignable features.
#[derive(Clone, Copy, Debug)]
struct Feature {
    /// Level in a log scale, normalized 0..1 across the take.
    energy: f64,
    /// Pitch relative to the take's own median, in semitones. `None`
    /// when unvoiced.
    pitch: Option<f64>,
    /// Below the silence threshold.
    silent: bool,
}

fn features(a: &Analysis, silence_level: f64) -> Vec<Feature> {
    let frames = &a.frames.frames;
    if frames.is_empty() {
        return Vec::new();
    }
    // Log level, normalized against the take's own peak: alignment is
    // about shape, and two takes recorded at different levels describe
    // the same performance.
    let peak = frames.iter().map(|f| f.rms).fold(0.0_f64, f64::max);
    let floor = 1e-6;
    let to_db = |v: f64| (v.max(floor) / peak.max(floor)).log10() * 20.0;
    // -60 dB maps to 0, 0 dB to 1.
    let norm = |db: f64| ((db + 60.0) / 60.0).clamp(0.0, 1.0);

    let mut pitches: Vec<f64> = frames
        .iter()
        .filter_map(|f| f.f0_hz.map(tune_dsp::hz_to_midi))
        .collect();
    pitches.sort_by(f64::total_cmp);
    let median = pitches.get(pitches.len() / 2).copied();

    frames
        .iter()
        .map(|f| {
            let energy = norm(to_db(f.rms));
            Feature {
                energy,
                pitch: match (f.f0_hz, median) {
                    (Some(hz), Some(m)) => Some(tune_dsp::hz_to_midi(hz) - m),
                    _ => None,
                },
                silent: energy < silence_level,
            }
        })
        .collect()
}

/// Cost of matching a dub frame to a reference frame.
fn cost(d: Feature, r: Feature, cfg: &AlignConfig) -> f64 {
    // Two silent frames are the same moment as far as anything can
    // tell, so pairing them is free and the slack collects here.
    if d.silent && r.silent {
        return 0.0;
    }
    let mut c = cfg.energy_weight * (d.energy - r.energy).abs();
    match (d.pitch, r.pitch) {
        (Some(a), Some(b)) => c += cfg.pitch_weight * (a - b).abs(),
        // One voiced, one not: a consonant against a vowel. This is the
        // cue that lines syllables up.
        (Some(_), None) | (None, Some(_)) => c += cfg.voicing_penalty,
        (None, None) => {}
    }
    c
}

/// The alignment, as a monotonic map over dub frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Alignment {
    /// For each dub frame, the reference frame it should land on.
    /// Same length as the dub's frame count.
    pub map: Vec<f64>,
    /// Frames per second both takes were analysed at.
    pub frame_rate: f64,
}

impl Alignment {
    /// Largest correction anywhere, in seconds — how far the dub had to
    /// move. A useful readout, and a sanity check: a value near the
    /// configured maximum usually means the takes are not the same
    /// part.
    pub fn max_shift_secs(&self) -> f64 {
        self.map
            .iter()
            .enumerate()
            .map(|(i, &r)| (r - i as f64).abs())
            .fold(0.0, f64::max)
            / self.frame_rate.max(1e-9)
    }

    /// The map as warp markers for the dub.
    ///
    /// One per frame would be far more than the renderer needs, so
    /// markers are thinned to every `stride` frames, always keeping the
    /// ends — the map is piecewise linear between markers anyway, and a
    /// marker per frame makes the interpolation exact at the cost of
    /// making the result impossible to edit afterwards.
    pub fn markers(&self, hop: usize, stride: usize) -> Vec<WarpMarker> {
        if self.map.is_empty() {
            return Vec::new();
        }
        let hop = hop.max(1) as f64;
        let stride = stride.max(1);
        let last = self.map.len() - 1;
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let source = i as f64;
            out.push(WarpMarker {
                sample: source * hop,
                d_time: (self.map[i] - source) * hop,
                pitch_bend: 0.0,
            });
            if i == last {
                break;
            }
            i = (i + stride).min(last);
        }
        out
    }
}

/// Align `dub` to `reference`.
///
/// Returns `None` when either take has no frames — there is nothing to
/// match, and a caller should say so rather than apply an empty map.
pub fn align(reference: &Analysis, dub: &Analysis, cfg: AlignConfig) -> Option<Alignment> {
    let r = features(reference, cfg.silence_level);
    let d = features(dub, cfg.silence_level);
    if r.is_empty() || d.is_empty() {
        return None;
    }
    // Both takes are analysed by the same code at the same rate; if a
    // caller ever mixes rates, the band below would be wrong in one of
    // them, so this is the assumption made explicit.
    let frame_rate = dub.frame_rate;
    let band = ((cfg.max_shift_secs * frame_rate).round() as usize).max(1);

    let path = dtw(&d, &r, band, &cfg);
    let strength = cfg.strength.clamp(0.0, 1.0);
    let cap = cfg.max_shift_secs * frame_rate;
    let mut map: Vec<f64> = path
        .into_iter()
        .enumerate()
        .map(|(i, target)| {
            let src = i as f64;
            // Clamp the correction, not the destination: the promise
            // `max_shift_secs` makes is about how far material moves.
            let shift = (target - src).clamp(-cap, cap) * strength;
            src + shift
        })
        .collect();
    // Clamping can flatten a rising map into a backward step at the
    // boundary, and a time map that goes backwards renders as a
    // stutter.
    for k in 1..map.len() {
        if map[k] < map[k - 1] {
            map[k] = map[k - 1];
        }
    }

    Some(Alignment { map, frame_rate })
}

/// Banded DTW returning, for each dub frame, the reference frame it
/// matched.
///
/// The band is around the diagonal *scaled to the two lengths*, not the
/// literal diagonal: two takes of the same line usually differ a little
/// in total length, and a band around `i` would drift off the true path
/// by the end of a long take even when every syllable is within a
/// fraction of a second of its partner.
fn dtw(d: &[Feature], r: &[Feature], band: usize, cfg: &AlignConfig) -> Vec<f64> {
    let (n, m) = (d.len(), r.len());
    let scale = m as f64 / n as f64;

    // Rolling two rows: the full matrix for a five-minute take would be
    // gigabytes, and only the back-pointers are needed afterwards.
    let width = band * 2 + 1;
    let centre = |i: usize| (i as f64 * scale).round() as isize;
    let idx = |i: usize, j: isize| -> Option<usize> {
        let off = j - centre(i) + band as isize;
        (off >= 0 && (off as usize) < width).then_some(off as usize)
    };

    let inf = f64::INFINITY;
    let mut prev = vec![inf; width];
    let mut cur = vec![inf; width];
    // Back-pointers: one byte per cell, 0 = diagonal, 1 = from the same
    // reference frame (dub stretched), 2 = from the same dub frame
    // (reference skipped).
    let mut back = vec![0u8; n * width];

    for i in 0..n {
        cur.iter_mut().for_each(|c| *c = inf);
        let lo = (centre(i) - band as isize).max(0);
        let hi = (centre(i) + band as isize).min(m as isize - 1);
        for j in lo..=hi {
            let Some(o) = idx(i, j) else { continue };
            // The tiebreak: identical frames and silent frames both
            // cost nothing to pair, so without this the path through
            // them is arbitrary.
            //
            // Measured against *not moving*, not against the search
            // centre. The centre is the length-scaled diagonal, which
            // spreads a difference in total length evenly across the
            // take — so biasing toward it would argue for a uniform
            // compression when the truth is that the dub simply came in
            // late, and the correction belongs at the front.
            let drift = (j - i as isize).unsigned_abs() as f64 / band as f64;
            let c = cost(d[i], r[j as usize], cfg) + cfg.diagonal_bias * drift;
            if i == 0 {
                // The first dub frame may start anywhere in the band,
                // so a dub that begins late is not forced to stretch
                // its opening silence to reach the reference.
                cur[o] = c;
                back[o] = 0;
                continue;
            }
            // Three predecessors, and their costs.
            let diag = idx(i - 1, j - 1).map(|p| prev[p]).unwrap_or(inf);
            let up = idx(i - 1, j).map(|p| prev[p]).unwrap_or(inf);
            let left = if j > lo {
                idx(i, j - 1).map(|p| cur[p]).unwrap_or(inf)
            } else {
                inf
            };
            let (best, dir) = if diag <= up && diag <= left {
                (diag, 0u8)
            } else if up <= left {
                (up, 1u8)
            } else {
                (left, 2u8)
            };
            if best.is_finite() {
                cur[o] = best + c;
                back[i * width + o] = dir;
            }
        }
        core::mem::swap(&mut prev, &mut cur);
    }

    // Walk back from the cheapest end cell.
    let last = n - 1;
    let lo = (centre(last) - band as isize).max(0);
    let hi = (centre(last) + band as isize).min(m as isize - 1);
    let mut best_j = hi;
    let mut best = inf;
    for j in lo..=hi {
        if let Some(o) = idx(last, j).filter(|&o| prev[o] < best) {
            best = prev[o];
            best_j = j;
        }
    }

    let mut map = vec![0.0; n];
    let mut i = last;
    let mut j = best_j;
    loop {
        map[i] = j as f64;
        if i == 0 {
            break;
        }
        let o = idx(i, j).unwrap_or(0);
        match back[i * width + o] {
            0 => {
                i -= 1;
                j = (j - 1).max(0);
            }
            1 => i -= 1,
            _ => {
                // Several reference frames for one dub frame: the map
                // keeps the last, and the frames in between are covered
                // by the piecewise-linear interpolation between
                // markers.
                j = (j - 1).max(0);
            }
        }
    }
    // Enforce monotonicity: back-pointer walks can produce a flat or
    // fractionally backward step at band edges, and a time map that
    // goes backwards renders as a stutter.
    for k in 1..map.len() {
        if map[k] < map[k - 1] {
            map[k] = map[k - 1];
        }
    }
    map
}
