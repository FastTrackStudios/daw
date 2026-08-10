//! Turning the vocal chain's gain decisions into editable automation.
//!
//! The chain applies gain internally and emits nothing, so its analysis
//! is good and invisible: you cannot see what the gate decided, disagree
//! with it, or keep the ride while bypassing the gate. This module is
//! the output side — each stage emits a **sparse, shaped curve** that
//! sums with the others into the item's one volume envelope.
//!
//! ## Why decimation is load-bearing, not an optimisation
//!
//! Analysis is block-rate: the classifier's default block is 10 ms, so
//! 100 points per second per curve. A five-minute vocal is ~30,000
//! points per curve and ~120,000 across four — roughly 2.4 MB per item
//! as text. Without decimation the curves cannot live in the item at
//! all. It therefore happens **at generation**, not at storage.
//!
//! ## One analysis pass, four independent curves
//!
//! The stages share the classifier, block classes and adaptive silence
//! floor, and never share gain curves. That is what makes their
//! independence affordable: the rider still knows where silence *is*, so
//! it does not ride room tone up between lines, while remaining ignorant
//! of what the gate decided. One pass rather than four also matters
//! across a whole song.

use crate::classify::{BlockClass, ClassifyConfig, Classifier, adaptive_silence_db};

/// One breakpoint on a generated envelope.
///
/// dB rather than linear, because that is the space the ear, the
/// tolerance and the composite sum all work in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvPoint {
    /// Seconds from the start of the item.
    pub t_s: f64,
    /// Gain in dB. 0 is unity.
    pub db: f64,
    /// Whether the curve holds this value until the next point rather
    /// than ramping to it.
    ///
    /// A gate is on/off with an attack and a release; forcing that
    /// through linear interpolation is what makes a decimated curve need
    /// far more points than the shape it describes.
    pub hold: bool,
}

impl EnvPoint {
    pub fn new(t_s: f64, db: f64) -> Self {
        Self {
            t_s,
            db,
            hold: false,
        }
    }
}

/// What every stage reads, computed once.
pub struct Analysis {
    /// Class of each block, in order.
    pub classes: Vec<BlockClass>,
    /// RMS of each block, dBFS.
    pub rms_db: Vec<f64>,
    /// The adaptive silence floor for this material.
    pub silence_db: f64,
    /// Samples per block.
    pub block_len: usize,
    pub sample_rate: f64,
}

impl Analysis {
    /// Seconds at the centre of a block.
    pub fn time_of(&self, block: usize) -> f64 {
        (block as f64 + 0.5) * self.block_len as f64 / self.sample_rate.max(1.0)
    }

    pub fn blocks(&self) -> usize {
        self.classes.len()
    }
}

/// Run the one shared pass over an item's audio.
pub fn analyze(samples: &[f64], sample_rate: f64, cfg: ClassifyConfig) -> Analysis {
    let mut classifier = Classifier::new(sample_rate, cfg);
    let block_len = classifier.block_samples().max(1);

    let mut features = Vec::new();
    let mut rms_db = Vec::new();
    let mut rms_lin = Vec::new();
    for chunk in samples.chunks(block_len) {
        let f = classifier.analyze_block(chunk);
        rms_db.push(f.rms_db);
        rms_lin.push(f.rms);
        features.push(f);
    }

    let silence_db = adaptive_silence_db(&rms_lin);
    let classes = features
        .iter()
        .map(|f| classifier.classify(f, silence_db))
        .collect();

    Analysis {
        classes,
        rms_db,
        silence_db,
        block_len,
        sample_rate,
    }
}

/// Reduce a dense per-block series to breakpoints.
///
/// Ramer–Douglas–Peucker in dB space, with `forced` indices kept
/// regardless. Pure RDP rounds a gate's sharp edge; pure event-driven
/// placement misses a ride that drifts across a held note with no event
/// to hang a point on. Seeding RDP with the event boundaries gets both.
pub fn decimate(dense: &[EnvPoint], tolerance_db: f64, forced: &[usize]) -> Vec<EnvPoint> {
    if dense.len() <= 2 {
        return dense.to_vec();
    }
    let mut keep = vec![false; dense.len()];
    keep[0] = true;
    keep[dense.len() - 1] = true;
    for &i in forced {
        if i < keep.len() {
            keep[i] = true;
        }
    }

    // Split at every forced point so RDP never smooths across one.
    let anchors: Vec<usize> = (0..dense.len()).filter(|&i| keep[i]).collect();
    for pair in anchors.windows(2) {
        rdp(dense, pair[0], pair[1], tolerance_db.max(1e-9), &mut keep);
    }

    dense
        .iter()
        .zip(keep.iter())
        .filter_map(|(p, &k)| k.then_some(*p))
        .collect()
}

fn rdp(pts: &[EnvPoint], lo: usize, hi: usize, tol: f64, keep: &mut [bool]) {
    if hi <= lo + 1 {
        return;
    }
    let (a, b) = (pts[lo], pts[hi]);
    let span = b.t_s - a.t_s;

    let mut worst = 0.0_f64;
    let mut worst_i = lo;
    for (i, p) in pts.iter().enumerate().take(hi).skip(lo + 1) {
        let projected = if span.abs() < 1e-12 {
            a.db
        } else {
            a.db + (b.db - a.db) * ((p.t_s - a.t_s) / span)
        };
        let err = (p.db - projected).abs();
        if err > worst {
            worst = err;
            worst_i = i;
        }
    }
    if worst > tol {
        keep[worst_i] = true;
        rdp(pts, lo, worst_i, tol, keep);
        rdp(pts, worst_i, hi, tol, keep);
    }
}

/// Block indices where the gate's decision changes.
///
/// Forced into the decimation so an attack and a release survive it —
/// the edges are the whole shape of a gate.
fn transitions(open: &[bool]) -> Vec<usize> {
    // Both sides of each edge, so the step between them is real rather
    // than an artefact of where the blocks fell.
    edges(open)
}

/// Generate the gate's envelope from a shared analysis pass.
///
/// Emits the gain the gate *would* apply, per block, then decimates.
/// The stage's own smoothing is reproduced here rather than re-running
/// the sample-rate gate, because an envelope is read at block rate and
/// the difference is inaudible against the tolerance.
pub fn gate_envelope(analysis: &Analysis, threshold_db: f64, floor_db: f64, tolerance_db: f64) -> Vec<EnvPoint> {
    if analysis.blocks() == 0 {
        return Vec::new();
    }
    let open: Vec<bool> = analysis
        .rms_db
        .iter()
        .map(|&db| db >= threshold_db)
        .collect();

    let dense: Vec<EnvPoint> = open
        .iter()
        .enumerate()
        .map(|(i, &is_open)| {
            let db = if is_open { 0.0 } else { floor_db };
            EnvPoint {
                t_s: analysis.time_of(i),
                db,
                // A gate holds its state and steps; it does not ramp
                // between blocks.
                hold: true,
            }
        })
        .collect();

    decimate(&dense, tolerance_db, &transitions(&open))
}

/// A span of blocks with one gain, rather than a point curve.
///
/// Sibilance is a **discrete event**: two orders of magnitude fewer
/// entries than a curve at block rate, directly editable ("this ess,
/// 3 dB"), and the natural thing to hand a plugin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainSpan {
    pub from_s: f64,
    pub to_s: f64,
    /// Gain in dB across the span. Negative reduces.
    pub db: f64,
}

/// Generate the breath envelope.
///
/// A breath is quiet, and not bright enough to be an "ess" — the two are
/// separated by the classifier rather than by re-running a detector, so
/// this stage and the sibilance stage cannot disagree about which is
/// which.
pub fn breath_envelope(
    analysis: &Analysis,
    reduction_db: f64,
    max_level_db: f64,
    min_level_db: f64,
    tolerance_db: f64,
) -> Vec<EnvPoint> {
    if analysis.blocks() == 0 {
        return Vec::new();
    }
    let ducked: Vec<bool> = analysis
        .rms_db
        .iter()
        .zip(&analysis.classes)
        .map(|(&db, &class)| {
            class != BlockClass::Tonal && db < max_level_db && db > min_level_db
        })
        .collect();

    let dense: Vec<EnvPoint> = ducked
        .iter()
        .enumerate()
        .map(|(i, &d)| EnvPoint {
            t_s: analysis.time_of(i),
            db: if d { -reduction_db.abs() } else { 0.0 },
            // Breaths duck in and out over a slew, so they ramp.
            hold: false,
        })
        .collect();

    decimate(&dense, tolerance_db, &edges(&ducked))
}

/// Detect sibilant spans and the gain to apply over each.
///
/// Spans rather than a point curve, and **not** a split-band de-esser: a
/// volume envelope is broadband, so folding a band-split reduction into
/// it would duck the vowel body along with the "ess". True multiband
/// de-essing stays available by feeding the item into a plugin — a
/// different tool, rather than an envelope pretending to be one.
pub fn sibilance_spans(analysis: &Analysis, reduction_db: f64) -> Vec<GainSpan> {
    let mut spans: Vec<GainSpan> = Vec::new();
    let mut run: Option<usize> = None;

    for i in 0..analysis.blocks() {
        let sibilant = analysis.classes[i] == BlockClass::Consonant
            && analysis.rms_db[i] > analysis.silence_db;
        match (sibilant, run) {
            (true, None) => run = Some(i),
            (false, Some(start)) => {
                spans.push(GainSpan {
                    from_s: analysis.time_of(start),
                    to_s: analysis.time_of(i),
                    db: -reduction_db.abs(),
                });
                run = None;
            }
            _ => {}
        }
    }
    if let Some(start) = run {
        spans.push(GainSpan {
            from_s: analysis.time_of(start),
            to_s: analysis.time_of(analysis.blocks().saturating_sub(1)),
            db: -reduction_db.abs(),
        });
    }
    spans
}

/// Generate the ride envelope: how loud each phrase should be.
///
/// Rides **tonal** blocks toward the target and holds elsewhere. It
/// consults the shared silence floor, so it does not ride room tone up
/// between lines — but it never consults the gate's *output*, which is
/// what keeps the two independently bypassable.
pub fn ride_envelope(
    analysis: &Analysis,
    target_db: f64,
    amount: f64,
    max_gain_db: f64,
    max_cut_db: f64,
    tolerance_db: f64,
) -> Vec<EnvPoint> {
    if analysis.blocks() == 0 {
        return Vec::new();
    }
    let mut held = 0.0_f64;
    let dense: Vec<EnvPoint> = (0..analysis.blocks())
        .map(|i| {
            let ridable = analysis.classes[i] == BlockClass::Tonal
                && analysis.rms_db[i] > analysis.silence_db;
            if ridable {
                let correction = (target_db - analysis.rms_db[i]) * amount.clamp(0.0, 1.0);
                held = correction.clamp(-max_cut_db.abs(), max_gain_db.abs());
            }
            // Between phrases the ride holds its last value rather than
            // returning to unity, so a phrase does not swell at its edges.
            EnvPoint {
                t_s: analysis.time_of(i),
                db: held,
                hold: false,
            }
        })
        .collect();

    decimate(&dense, tolerance_db, &[])
}

/// Block indices either side of every change in a boolean series.
fn edges(flags: &[bool]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 1..flags.len() {
        if flags[i] != flags[i - 1] {
            out.push(i - 1);
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize, from: f64, to: f64) -> Vec<EnvPoint> {
        (0..n)
            .map(|i| {
                let x = i as f64 / (n - 1) as f64;
                EnvPoint::new(i as f64 * 0.01, from + (to - from) * x)
            })
            .collect()
    }

    #[test]
    fn a_straight_line_reduces_to_its_endpoints() {
        let got = decimate(&ramp(100, -6.0, 0.0), 0.25, &[]);
        assert_eq!(got.len(), 2, "nothing in between says anything new");
    }

    #[test]
    fn a_slow_drift_across_a_held_note_is_not_flattened() {
        // 6 dB over the span, with no event anywhere: pure event-driven
        // placement would emit nothing between the ends.
        let dense = ramp(200, 0.0, -6.0);
        let got = decimate(&dense, 0.25, &[]);
        assert!(got.len() >= 2);
        // The reconstruction must stay within tolerance everywhere.
        for p in &dense {
            let approx = sample_at(&got, p.t_s);
            assert!(
                (approx - p.db).abs() <= 0.25 + 1e-9,
                "drift lost at {}: {} vs {}",
                p.t_s,
                approx,
                p.db
            );
        }
    }

    #[test]
    fn a_forced_point_survives_decimation() {
        let dense = ramp(100, 0.0, 0.0);
        let got = decimate(&dense, 0.25, &[50]);
        assert!(
            got.iter().any(|p| (p.t_s - dense[50].t_s).abs() < 1e-9),
            "a boundary the classifier found is not RDP's to discard"
        );
    }

    #[test]
    fn a_tighter_tolerance_yields_more_points_never_fewer() {
        let dense = ramp(200, 0.0, -12.0);
        let coarse = decimate(&dense, 1.0, &[]).len();
        let fine = decimate(&dense, 0.05, &[]).len();
        assert!(fine >= coarse, "{fine} < {coarse}");
    }

    #[test]
    fn a_gate_edge_keeps_both_sides() {
        let mut dense: Vec<EnvPoint> = Vec::new();
        let mut open = vec![];
        for i in 0..100 {
            let is_open = i >= 50;
            open.push(is_open);
            dense.push(EnvPoint {
                t_s: i as f64 * 0.01,
                db: if is_open { 0.0 } else { -60.0 },
                hold: true,
            });
        }
        let got = decimate(&dense, 0.25, &transitions(&open));
        let has = |t: f64| got.iter().any(|p| (p.t_s - t).abs() < 1e-9);
        assert!(has(0.49), "the last closed block");
        assert!(has(0.50), "and the first open one");
    }

    fn sample_at(pts: &[EnvPoint], t: f64) -> f64 {
        match pts.iter().position(|p| p.t_s >= t) {
            None => pts.last().map(|p| p.db).unwrap_or(0.0),
            Some(0) => pts[0].db,
            Some(i) => {
                let (a, b) = (pts[i - 1], pts[i]);
                let span = b.t_s - a.t_s;
                if span.abs() < 1e-12 {
                    b.db
                } else {
                    a.db + (b.db - a.db) * ((t - a.t_s) / span)
                }
            }
        }
    }
}
