//! Quantizing audio to the project grid.
//!
//! Same shape as the alignment paths — detect, choose targets, produce a
//! time map — with the targets coming from the grid instead of from
//! another take. See `spec/grid-quantize.md`.
//!
//! The output is a [`Plan`]: a list of transients and where each should
//! end up. What is done with it is the mode's business —
//! [`Plan::alignment`] warps, [`Plan::splits`] cuts — and both are built
//! from the same decisions, so the two modes can never disagree about
//! where a hit belongs.

use crate::align::Alignment;
use crate::detect::Transient;

/// How transients are matched to grid divisions.
#[derive(Clone, Copy, Debug)]
pub struct QuantizeConfig {
    /// Seconds between grid divisions.
    pub grid_secs: f64,
    /// Where the grid starts, in seconds from the take's start.
    ///
    /// Takes do not begin on a division, and assuming they do puts every
    /// hit out by the remainder.
    pub grid_offset_secs: f64,
    /// Half-width of the window each division scans, in seconds. `None`
    /// turns grid scan off: every transient snaps to its own nearest
    /// division instead.
    pub tolerance_secs: Option<f64>,
    /// `0.0` leaves the take alone, `1.0` puts every hit on its
    /// division.
    pub strength: f64,
}

impl Default for QuantizeConfig {
    fn default() -> Self {
        Self {
            // 1/16 at 120 bpm.
            grid_secs: 0.125,
            grid_offset_secs: 0.0,
            tolerance_secs: Some(0.05),
            strength: 1.0,
        }
    }
}

/// One transient and where it is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Move {
    /// Where the transient is now, in seconds.
    pub from: f64,
    /// Where it should be after quantizing, in seconds. Strength is
    /// already applied — this is the final position, not the division.
    pub to: f64,
    /// The division it was matched to, for display. Unaffected by
    /// strength.
    pub division: f64,
}

impl Move {
    pub fn shift(&self) -> f64 {
        self.to - self.from
    }
}

/// What quantizing will do, before anything is written.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub moves: Vec<Move>,
    /// Transients that were detected and deliberately left alone: no
    /// division claimed them.
    ///
    /// Kept rather than dropped because "why did that hit not move" is
    /// the first question a user asks, and the answer is here.
    pub unmatched: Vec<f64>,
}

/// Match transients to grid divisions.
///
/// With a tolerance set, **each division takes at most one transient**,
/// the loudest inside its window. That one rule does most of the work: a
/// buzz roll cannot produce eight hits fighting over one division, a
/// ghost note between divisions is left alone rather than dragged onto a
/// beat it was never near, and a division with nothing near it stays
/// empty — silence is not quantized onto.
///
/// Without one, every transient snaps to its own nearest division, which
/// is right for material that is not on a grid and wrong for a kit.
pub fn plan(transients: &[Transient], cfg: QuantizeConfig) -> Plan {
    if transients.is_empty() || cfg.grid_secs <= 0.0 {
        return Plan::default();
    }
    let strength = cfg.strength.clamp(0.0, 1.0);
    let division_of = |t: f64| {
        let n = ((t - cfg.grid_offset_secs) / cfg.grid_secs).round();
        cfg.grid_offset_secs + n * cfg.grid_secs
    };

    let mut moves = Vec::new();
    let mut matched = vec![false; transients.len()];

    match cfg.tolerance_secs {
        Some(tolerance) => {
            // Walk divisions, not transients. Every division from the
            // first hit to the last, so a division with nothing near it
            // is visited and correctly produces nothing.
            let first = transients.first().expect("non-empty").at;
            let last = transients.last().expect("non-empty").at;
            let n0 = ((first - cfg.grid_offset_secs) / cfg.grid_secs).floor() as i64;
            let n1 = ((last - cfg.grid_offset_secs) / cfg.grid_secs).ceil() as i64;

            for n in n0..=n1 {
                let division = cfg.grid_offset_secs + n as f64 * cfg.grid_secs;
                // The loudest transient in the window, not the nearest.
                // A ghost note a hair closer to the division than the
                // backbeat must not be the one that gets quantized —
                // same reasoning as the spacing contest in `onsets`.
                let winner = transients
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| !matched[*i] && (t.at - division).abs() <= tolerance)
                    .max_by(|a, b| {
                        a.1.loudness
                            .partial_cmp(&b.1.loudness)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                if let Some((i, t)) = winner {
                    matched[i] = true;
                    moves.push(Move {
                        from: t.at,
                        to: t.at + (division - t.at) * strength,
                        division,
                    });
                }
            }
        }
        None => {
            for (i, t) in transients.iter().enumerate() {
                let division = division_of(t.at);
                matched[i] = true;
                moves.push(Move {
                    from: t.at,
                    to: t.at + (division - t.at) * strength,
                    division,
                });
            }
        }
    }

    moves.sort_by(|a, b| {
        a.from
            .partial_cmp(&b.from)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Two hits quantized past each other would need audio to run
    // backwards. Drop the later one rather than reorder: it is the one
    // whose division was already taken by something louder.
    let mut ordered: Vec<Move> = Vec::with_capacity(moves.len());
    let mut dropped: Vec<f64> = Vec::new();
    for m in moves {
        if ordered.last().is_some_and(|last| m.to <= last.to) {
            dropped.push(m.from);
            continue;
        }
        ordered.push(m);
    }

    let mut unmatched: Vec<f64> = transients
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched[*i])
        .map(|(_, t)| t.at)
        .collect();
    unmatched.extend(dropped);
    unmatched.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    Plan {
        moves: ordered,
        unmatched,
    }
}

impl Plan {
    /// The plan as a time map, for WARP mode.
    ///
    /// `frames` and `frame_rate` describe the map the caller needs, not
    /// the analysis: this is what the renderer walks, so it has to cover
    /// the take end to end at whatever resolution the caller is working
    /// in.
    ///
    /// Between moved transients the map is linear, which spreads the
    /// correction across the decay of one hit and the space before the
    /// next — the part with no features of its own to preserve. Outside
    /// the first and last it holds constant rather than decaying to
    /// zero, so a lead-in arrives with the hit it leads into.
    pub fn alignment(&self, frames: usize, frame_rate: f64) -> Option<Alignment> {
        if self.moves.is_empty() || frames == 0 || frame_rate <= 0.0 {
            return None;
        }
        let mut anchors: Vec<(f64, f64)> = Vec::with_capacity(self.moves.len() + 2);
        let first = &self.moves[0];
        anchors.push((0.0, first.shift() * frame_rate));
        for m in &self.moves {
            anchors.push((m.from * frame_rate, m.shift() * frame_rate));
        }
        let last = self.moves.last().expect("non-empty");
        anchors.push(((frames - 1) as f64, last.shift() * frame_rate));
        anchors.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        anchors.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);

        let mut map = Vec::with_capacity(frames);
        let mut seg = 0usize;
        for i in 0..frames {
            let x = i as f64;
            while seg + 1 < anchors.len() && anchors[seg + 1].0 < x {
                seg += 1;
            }
            let (x0, d0) = anchors[seg];
            let (x1, d1) = anchors[(seg + 1).min(anchors.len() - 1)];
            let offset = if (x1 - x0).abs() < 1e-9 {
                d0
            } else {
                let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                d0 + (d1 - d0) * t
            };
            map.push((x + offset).max(0.0));
        }
        for i in 1..map.len() {
            if map[i] < map[i - 1] {
                map[i] = map[i - 1];
            }
        }
        Some(Alignment { map, frame_rate })
    }

    /// The plan as a list of pieces to cut and move, for SPLIT mode.
    ///
    /// `take_secs` is the item's length; the last piece runs to it.
    pub fn splits(&self, take_secs: f64, cfg: SplitConfig) -> Vec<Piece> {
        if self.moves.is_empty() {
            return Vec::new();
        }
        let pad = cfg.leading_pad_secs.max(0.0);
        let mut pieces = Vec::with_capacity(self.moves.len() + 1);

        // The audio before the first transient is a piece too, and it
        // does not move: nothing in it was detected, so nothing about it
        // is late.
        let first_cut = (self.moves[0].from - pad).max(0.0);
        if first_cut > 0.0 {
            pieces.push(Piece {
                cut: 0.0,
                end: first_cut,
                shift: 0.0,
                transient: None,
            });
        }

        for (i, m) in self.moves.iter().enumerate() {
            // The cut goes *before* the transient; the shift is measured
            // at the transient. Conflating the two is the classic way to
            // get this wrong — the audio then arrives `pad` early and
            // every hit is flammed against the rest of the kit.
            let cut = (m.from - pad).max(0.0);
            let end = match self.moves.get(i + 1) {
                Some(next) => (next.from - pad).max(cut),
                None => take_secs.max(cut),
            };
            pieces.push(Piece {
                cut,
                end,
                shift: m.shift(),
                transient: Some(m.from),
            });
        }
        pieces
    }
}

/// How SPLIT mode cuts.
#[derive(Clone, Copy, Debug)]
pub struct SplitConfig {
    /// Cut this far *before* each transient, without moving the snap
    /// point.
    ///
    /// A cut exactly on an attack clips the front of it. A cut 5–10 ms
    /// early leaves the attack whole and puts the join in the previous
    /// hit's decay, where a crossfade is inaudible.
    pub leading_pad_secs: f64,
    /// Crossfade length at each join. Moving pieces leaves a gap or an
    /// overlap at every cut, and a hard edge there is a click.
    pub crossfade_secs: f64,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            leading_pad_secs: 0.007,
            crossfade_secs: 0.007,
        }
    }
}

/// One piece of a split item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    /// Where the piece is cut from the source, in seconds from the
    /// take's start.
    pub cut: f64,
    /// Where it ends, in seconds from the take's start.
    pub end: f64,
    /// How far it moves. Positive is later.
    pub shift: f64,
    /// The transient this piece carries, if any — the lead-in piece has
    /// none.
    pub transient: Option<f64>,
}

impl Piece {
    pub fn len(&self) -> f64 {
        (self.end - self.cut).max(0.0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= 0.0
    }

    /// Where the piece lands.
    pub fn placed(&self) -> f64 {
        self.cut + self.shift
    }
}
