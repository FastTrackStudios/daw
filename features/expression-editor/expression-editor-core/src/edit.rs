//! Edits and undo.
//!
//! Every mutation goes through [`Edit`] so the UI stays a pure view and
//! the host (MIDI writer, audio render job) sees a describable change
//! rather than a mutated document it has to diff.
//!
//! History is snapshot-based and bounded. Inverse-operation undo would
//! be leaner, but a freehand pen stroke's inverse is the entire prior
//! curve anyway, and zone/target changes have to be captured alongside
//! note and expression changes in a single step — which snapshots get
//! right for free.

use crate::blob;
use crate::doc::{Curve, ExpressionDoc, Lane, Note, NoteId, Point, Target};
use crate::shape::Shape;

/// One describable change to the document.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Replace `[t0, t1]` of a lane with drawn points (pen, curve).
    /// Points outside the interval survive untouched.
    DrawLane {
        note: NoteId,
        lane: Lane,
        t0: f64,
        t1: f64,
        points: Vec<Point>,
    },
    /// Erase a lane over `[t0, t1]`.
    EraseLane {
        note: NoteId,
        lane: Lane,
        t0: f64,
        t1: f64,
    },
    /// Restyle `[t0, t1]` between its existing endpoints.
    ReshapeLane {
        note: NoteId,
        lane: Lane,
        t0: f64,
        t1: f64,
        shape: Shape,
        samples: usize,
    },
    /// Scale a lane about its effective center — the alt-drag gesture.
    /// `factor` below zero inverts.
    ScaleLane {
        note: NoteId,
        lane: Lane,
        t0: f64,
        t1: f64,
        factor: f64,
    },
    /// Scale drift and vibrato independently (the Melodyne sliders).
    /// Rewrites the pitch curve from its decomposition.
    ReblendPitch {
        note: NoteId,
        t0: f64,
        t1: f64,
        drift_amount: f64,
        modulation_amount: f64,
    },
    /// Transpose notes. The pitch gesture moves rigidly with the row.
    Transpose { notes: Vec<NoteId>, semitones: i32 },
    /// Move notes in time, carrying their owned expression.
    MoveTime { notes: Vec<NoteId>, delta: f64 },
    /// Resize a note; owned expression stretches to the new bounds.
    Resize { note: NoteId, start: f64, end: f64 },
    /// Set a note's sounding pitch offset from its row — how a
    /// microtonal target is stored.
    SetPitchOffset { note: NoteId, semitones: f64 },
    AddNote(Box<Note>),
    DeleteNotes(Vec<NoteId>),
    SplitNote { note: NoteId, t: f64 },
    /// Q zones.
    AddZoneSplit { note: NoteId, t: f64 },
    RemoveZoneSplit { note: NoteId, t: f64, tolerance: f64 },
    MoveZoneSplit { note: NoteId, from: f64, to: f64 },
    SetTarget { note: NoteId, target: Target },
    /// Reassign MPE member channels so overlapping notes never share
    /// one.
    AssignChannels { notes: Vec<NoteId>, seed: u64 },
    SetBendRange(f64),
}

/// How many samples a reshape or reblend writes per second of audio.
/// Dense enough that instruments which ignore sparse expression still
/// track the gesture.
const DEFAULT_SAMPLES: usize = 64;

impl Edit {
    /// Apply to `doc`. Returns false if the edit could not be applied
    /// (missing note, degenerate span) — the caller should then not
    /// push a history entry.
    pub fn apply(&self, doc: &mut ExpressionDoc) -> bool {
        let units_per_second = doc.time_base.units_per_second(120.0);
        match self {
            Edit::DrawLane {
                note,
                lane,
                t0,
                t1,
                points,
            } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (lo, hi) = ordered(*t0, *t1);
                n.lane_mut(*lane).splice(lo, hi, points);
                extend_to_note_edges(n, *lane);
                true
            }
            Edit::EraseLane {
                note,
                lane,
                t0,
                t1,
            } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (lo, hi) = ordered(*t0, *t1);
                n.lane_mut(*lane).remove_range(lo, hi) > 0
            }
            Edit::ReshapeLane {
                note,
                lane,
                t0,
                t1,
                shape,
                samples,
            } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (lo, hi) = ordered(*t0, *t1);
                let default = lane.default_value();
                n.lane_mut(*lane)
                    .reshape(lo, hi, *shape, (*samples).max(2), default);
                true
            }
            Edit::ScaleLane {
                note,
                lane,
                t0,
                t1,
                factor,
            } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (lo, hi) = ordered(*t0, *t1);
                let default = lane.default_value();
                let pivot =
                    blob::effective_center(n.lane(*lane), lo, hi, DEFAULT_SAMPLES, default);
                n.lane_mut(*lane).scale_about(lo, hi, pivot, *factor);
                true
            }
            Edit::ReblendPitch {
                note,
                t0,
                t1,
                drift_amount,
                modulation_amount,
            } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (lo, hi) = ordered(*t0, *t1);
                if hi - lo <= 0.0 {
                    return false;
                }
                let d = blob::decompose(
                    &n.pitch,
                    lo,
                    hi,
                    DEFAULT_SAMPLES,
                    units_per_second,
                    0.0,
                );
                let rebuilt = d.recompose(d.center, *drift_amount, *modulation_amount);
                n.pitch.splice(lo, hi, rebuilt.points());
                true
            }
            Edit::Transpose { notes, semitones } => {
                let mut any = false;
                for id in notes {
                    if let Some(n) = doc.note_mut(*id) {
                        n.row = (n.row + semitones).clamp(0, 127);
                        any = true;
                    }
                }
                any
            }
            Edit::MoveTime { notes, delta } => {
                let mut any = false;
                for id in notes {
                    if let Some(n) = doc.note_mut(*id) {
                        let (s, e) = (n.start, n.end);
                        n.start += delta;
                        n.end += delta;
                        for s2 in n.splits.iter_mut() {
                            *s2 += delta;
                        }
                        for lane in Lane::ALL {
                            n.lane_mut(lane).shift_time(s, e, *delta);
                        }
                        any = true;
                    }
                }
                any
            }
            Edit::Resize { note, start, end } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (new_s, new_e) = ordered(*start, *end);
                if new_e - new_s <= 0.0 {
                    return false;
                }
                let (old_s, old_e) = (n.start, n.end);
                if (old_e - old_s).abs() < 1e-9 {
                    return false;
                }
                let scale = (new_e - new_s) / (old_e - old_s);
                for s in n.splits.iter_mut() {
                    *s = new_s + (*s - old_s) * scale;
                }
                for lane in Lane::ALL {
                    n.lane_mut(lane).remap_time(old_s, old_e, new_s, new_e);
                }
                n.start = new_s;
                n.end = new_e;
                true
            }
            Edit::SetPitchOffset { note, semitones } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let (s, e) = (n.start, n.end);
                if n.pitch.is_empty() {
                    n.pitch.set(s, *semitones);
                    n.pitch.set(e, *semitones);
                } else {
                    // Move the whole gesture rigidly — a microtonal
                    // target shifts the note without reshaping how it
                    // was sung or drawn.
                    let current =
                        blob::effective_center(&n.pitch, s, e, DEFAULT_SAMPLES, 0.0);
                    n.pitch.offset(s, e, *semitones - current);
                }
                true
            }
            Edit::AddNote(note) => {
                doc.push((**note).clone());
                true
            }
            Edit::DeleteNotes(ids) => {
                let before = doc.notes.len();
                doc.notes.retain(|n| !ids.contains(&n.id));
                doc.notes.len() != before
            }
            Edit::SplitNote { note, t } => split_note(doc, *note, *t),
            Edit::AddZoneSplit { note, t } => doc
                .note_mut(*note)
                .map(|n| n.add_split(*t))
                .unwrap_or(false),
            Edit::RemoveZoneSplit {
                note,
                t,
                tolerance,
            } => doc
                .note_mut(*note)
                .map(|n| n.remove_split_near(*t, *tolerance))
                .unwrap_or(false),
            Edit::MoveZoneSplit { note, from, to } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                let Some(i) = n
                    .splits
                    .iter()
                    .position(|s| (s - from).abs() < 1e-6)
                else {
                    return false;
                };
                // Keep splits ordered and strictly interior; a boundary
                // dragged past its neighbour would silently reorder the
                // zones under the pointer.
                let lo = n.splits.get(i.wrapping_sub(1)).copied().unwrap_or(n.start);
                let hi = n.splits.get(i + 1).copied().unwrap_or(n.end);
                let eps = (n.end - n.start) * 1e-4;
                n.splits[i] = to.clamp(lo + eps, hi - eps);
                true
            }
            Edit::SetTarget { note, target } => {
                let Some(n) = doc.note_mut(*note) else {
                    return false;
                };
                n.target = match *target {
                    Target::Zone(i) if i >= n.zone_count() => Target::WholeNote,
                    t => t,
                };
                true
            }
            Edit::AssignChannels { notes, seed } => assign_channels(doc, notes, *seed),
            Edit::SetBendRange(r) => {
                doc.bend_range = r.max(1.0);
                true
            }
        }
    }
}

fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Hold a lane's first and last authored values out to the note edges.
///
/// Without this a gesture drawn in the middle of a note leaves a gap
/// where the lane falls back to its default — audible as a jump to
/// center on either side of the edit.
fn extend_to_note_edges(note: &mut Note, lane: Lane) {
    let (s, e) = (note.start, note.end);
    let curve = note.lane_mut(lane);
    let Some((first, last)) = curve.bounds() else {
        return;
    };
    if first > s {
        let v = curve.sample(first, lane.default_value());
        curve.set(s, v);
    }
    if last < e {
        let v = curve.sample(last, lane.default_value());
        curve.set(e, v);
    }
}

fn split_note(doc: &mut ExpressionDoc, id: NoteId, t: f64) -> bool {
    let Some(src) = doc.note(id).cloned() else {
        return false;
    };
    if t <= src.start || t >= src.end {
        return false;
    }
    let new_id = doc.mint_id();
    let mut right = src.clone();
    right.id = new_id;
    right.start = t;
    right.splits.retain(|&s| s > t);
    right.target = Target::WholeNote;
    for lane in Lane::ALL {
        let held = right.lane(lane).sample(t, lane.default_value());
        let curve = right.lane_mut(lane);
        curve.remove_range(f64::NEG_INFINITY, t);
        curve.set(t, held);
    }

    let Some(left) = doc.note_mut(id) else {
        return false;
    };
    left.end = t;
    left.splits.retain(|&s| s < t);
    left.target = Target::WholeNote;
    for lane in Lane::ALL {
        let held = left.lane(lane).sample(t, lane.default_value());
        let curve = left.lane_mut(lane);
        curve.remove_range(t, f64::INFINITY);
        curve.set(t, held);
    }
    doc.push(right);
    true
}

/// Give each note an MPE member channel (2..=16) such that no two
/// overlapping, touching, or immediately consecutive notes share one.
///
/// Channel 1 stays free as the MPE master. The consecutive-note rule
/// matters as much as the overlap rule: reusing a channel the instant a
/// note ends means the incoming note's setup expression lands while the
/// outgoing note's release is still sounding.
fn assign_channels(doc: &mut ExpressionDoc, ids: &[NoteId], seed: u64) -> bool {
    let mut order: Vec<usize> = doc
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| ids.contains(&n.id))
        .map(|(i, _)| i)
        .collect();
    if order.is_empty() {
        return false;
    }
    order.sort_by(|&a, &b| {
        doc.notes[a]
            .start
            .partial_cmp(&doc.notes[b].start)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    // Deterministic per-seed rotation: same input, same layout, but a
    // fresh seed reshuffles when an instrument dislikes a given spread.
    let mut rng = seed | 1;
    let mut assigned: Vec<(f64, f64, u8)> = Vec::new();
    for &i in &order {
        let (start, end) = (doc.notes[i].start, doc.notes[i].end);
        let mut taken = [false; 17];
        for &(s, e, ch) in &assigned {
            // Touching counts as conflicting.
            if s <= end && start <= e {
                taken[ch as usize] = true;
            }
        }
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let offset = (rng >> 33) as usize % 15;
        let chosen = (0..15)
            .map(|k| 2 + ((offset + k) % 15) as u8)
            .find(|&ch| !taken[ch as usize])
            .unwrap_or(2);
        doc.notes[i].channel = Some(chosen);
        assigned.push((start, end, chosen));
    }
    doc.mark_ambiguity();
    true
}

/// Bounded snapshot undo.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    past: Vec<ExpressionDoc>,
    future: Vec<ExpressionDoc>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Apply `edit`, recording an undo step only if it changed
    /// anything.
    pub fn apply(&mut self, doc: &mut ExpressionDoc, edit: &Edit) -> bool {
        let snapshot = doc.clone();
        if !edit.apply(doc) {
            return false;
        }
        self.push_snapshot(snapshot);
        true
    }

    /// Record the pre-gesture state directly — for drags that stream
    /// many edits but should collapse into one undo step.
    pub fn begin_gesture(&mut self, doc: &ExpressionDoc) {
        self.push_snapshot(doc.clone());
    }

    fn push_snapshot(&mut self, snapshot: ExpressionDoc) {
        self.past.push(snapshot);
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        self.future.clear();
    }

    pub fn undo(&mut self, doc: &mut ExpressionDoc) -> bool {
        let Some(prev) = self.past.pop() else {
            return false;
        };
        self.future.push(core::mem::replace(doc, prev));
        true
    }

    pub fn redo(&mut self, doc: &mut ExpressionDoc) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        self.past.push(core::mem::replace(doc, next));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(10)
    }
}

/// Build the point list a freehand stroke commits, at one point per
/// document-time step (screen-pixel resolution at the current zoom).
pub fn stroke_points(samples: &[(f64, f64)], lane: Lane) -> Vec<Point> {
    let mut curve = Curve::new();
    for &(t, v) in samples {
        curve.set(t, lane.clamp(v));
    }
    curve.points().to_vec()
}
