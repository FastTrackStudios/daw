//! Adapter between the expression editor's document and `tune_dsp`'s
//! analyzed pitch model.
//!
//! The audio counterpart of `expression-editor-daw`: that one converts
//! a MIDI take, this converts an analysis, and both produce the same
//! [`ExpressionDoc`]. That is the entire reason one editor can be both
//! an MPE editor and a Melodyne competitor — the surface never learns
//! which domain it is serving.
//!
//! ## Where the two models line up, and where they do not
//!
//! They agree on the important thing. `tune_dsp::NoteBlob` stores
//! `center + drift·amount + modulation·amount`; the editor stores an
//! integer row plus a curve of semitones relative to it. Those are the
//! same decomposition with the origin in a different place, so the
//! conversion is arithmetic rather than interpretation.
//!
//! They disagree on three things, each handled once here:
//!
//! - **Time.** A blob is indexed by analysis *frame*; the document is
//!   in `TimeBase::Frames`, so `t` and frame index are the same number.
//!   Keeping them identical is deliberate — a scaling factor between
//!   them would have to be undone at every edit.
//! - **Pitch origin.** A blob's `center_midi` is absolute and
//!   fractional. The document splits that into a rounded integer `row`
//!   plus the fractional remainder folded into the curve, so a note sung
//!   30 cents flat sits on its literal row with the flatness in the
//!   curve — which is also how microtonal MIDI is drawn.
//! - **Per-note trims.** `formant_shift` and `gain_db` have no MIDI
//!   equivalent, so they ride in the document's `Timbre` and `Pressure`
//!   lanes as flat curves. They are round-tripped exactly rather than
//!   re-derived, because a trim the user set is not something to infer.
//!
//! [`to_doc`] and [`apply_to`] are pure functions over snapshots, so
//! the whole path is testable with no audio, no DSP and no UI.

use expression_editor_core::doc::{ExpressionDoc, Lane, Note, NoteId, TimeBase};
use tune_dsp::model::{NoteBlob, PitchDoc};

pub mod spans;

pub use spans::{unvoiced_spans, Span};

/// How a per-note trim is stored in a lane.
///
/// The lanes are 0..1 normalized, and both trims are signed, so each
/// needs a range and a centre. These are display ranges, not limits on
/// what the DSP will accept — they only decide how far a drag travels.
pub const FORMANT_RANGE: f64 = 12.0;
/// Gain trim range in dB, ± this value.
pub const GAIN_RANGE_DB: f64 = 24.0;

/// Samples used to find a note's pitch centre on write-back. Dense
/// enough that a vibrato cycle cannot bias the median, cheap enough to
/// run for every note on every write.
const CENTER_SAMPLES: usize = 128;

/// The decomposition's drift/vibrato split is a *frequency* split, so
/// it needs to know how document time maps to seconds. In
/// `TimeBase::Frames` that is exact and this value is unused; it is
/// only a fallback for a tick-based document, which the audio path does
/// not produce.
const REFERENCE_BPM: f64 = 120.0;

/// Encode a signed value into a 0..1 lane position.
fn to_lane(v: f64, range: f64) -> f64 {
    (0.5 + v / (range * 2.0)).clamp(0.0, 1.0)
}

/// Inverse of [`to_lane`].
fn from_lane(v: f64, range: f64) -> f64 {
    (v - 0.5) * range * 2.0
}

/// Convert an analysis into an editable document.
///
/// `frame_rate` is `sample_rate / hop` — the rate the blobs' frame
/// indices tick at.
pub fn to_doc(pitch: &PitchDoc, frame_rate: f64) -> ExpressionDoc {
    let end = pitch
        .blobs
        .iter()
        .map(|b| b.end_frame as f64)
        .fold(0.0_f64, f64::max)
        .max(frame_rate);
    let mut doc = ExpressionDoc::new(TimeBase::Frames { frame_rate }, 0.0, end);

    for (i, blob) in pitch.blobs.iter().enumerate() {
        doc.push(blob_to_note(NoteId(i as u64 + 1), blob));
    }
    doc
}

/// One blob as an editor note.
pub fn blob_to_note(id: NoteId, blob: &NoteBlob) -> Note {
    let start = blob.start_frame as f64;
    let end = blob.end_frame as f64;
    // The row is the *rounded* centre and the remainder goes into the
    // curve. A sung note 30 cents flat then draws on its literal row
    // with the flatness visible as curve offset, rather than sitting
    // between two rows where nothing lines up.
    let row = blob.center_midi.round();
    let offset = blob.center_midi - row;

    let mut note = Note::new(id, start, end.max(start + 1.0), row as i32);
    note.channel = None;
    note.weight = blob.rms;

    // The sounding contour, sampled per frame: exactly what
    // `NoteBlob::target_midi` reports, expressed relative to the row.
    for frame in blob.start_frame..=blob.end_frame {
        let Some(midi) = blob.target_midi(frame) else {
            continue;
        };
        note.pitch.set(frame as f64, midi - row);
    }
    // A blob with no frames still has a centre, and a note with an
    // empty curve would read as "exactly on the row" rather than as
    // however flat it was sung.
    if note.pitch.is_empty() {
        note.pitch.set(start, offset);
        note.pitch.set(end, offset);
    }

    set_flat(&mut note, Lane::Timbre, to_lane(blob.formant_shift, FORMANT_RANGE));
    set_flat(&mut note, Lane::Pressure, to_lane(blob.gain_db, GAIN_RANGE_DB));
    note
}

/// Hold a lane at one value across the note.
///
/// Two points rather than one: the curve holds its endpoint value
/// outside the authored range, but a single point gives the UI nothing
/// to draw a span from.
fn set_flat(note: &mut Note, lane: Lane, value: f64) {
    let (s, e) = (note.start, note.end);
    let curve = note.lane_mut(lane);
    curve.set(s, value);
    curve.set(e, value);
}

/// Write a document's edits back onto the analysis.
///
/// Only the fields the editor owns are touched. `drift` and
/// `modulation` — what was actually *sung* — are never rewritten, so a
/// re-analysis can replace them without discarding the user's edits.
/// That is the same reason `NoteBlob` keeps `analyzed_center_midi`.
///
/// Returns how many blobs matched a note. Notes with no counterpart are
/// skipped rather than appended: creating audio for a note that was
/// never sung is not something this adapter can do.
pub fn apply_to(doc: &ExpressionDoc, pitch: &mut PitchDoc) -> usize {
    let mut applied = 0;
    for (i, blob) in pitch.blobs.iter_mut().enumerate() {
        let Some(note) = doc.note(NoteId(i as u64 + 1)) else {
            continue;
        };
        // The centre is *not* the curve at the midpoint: that reading
        // includes whatever drift and vibrato are passing through at
        // the time, so a note with a scoop would write back a centre
        // pulled toward the scoop. It is the median of the contour —
        // the same "where the curve actually dwells" the core uses for
        // zone scaling, so the surface and the write-back agree.
        let default = Lane::Pitch.default_value();
        let center = expression_editor_core::blob::decompose(
            &note.pitch,
            note.start,
            note.end,
            CENTER_SAMPLES,
            doc.time_base.units_per_second(REFERENCE_BPM),
            default,
        )
        .center;
        blob.center_midi = note.row as f64 + center;

        let mid = (note.start + note.end) * 0.5;
        blob.formant_shift = from_lane(
            note.timbre.sample(mid, Lane::Timbre.default_value()),
            FORMANT_RANGE,
        );
        blob.gain_db = from_lane(
            note.pressure.sample(mid, Lane::Pressure.default_value()),
            GAIN_RANGE_DB,
        );
        applied += 1;
    }
    applied
}

/// Push the drift and vibrato blend back onto the analysis.
///
/// Separate from [`apply_to`] because these two are the *only* fields
/// where the editor's curve and the blob's decomposition can disagree:
/// the editor may have redrawn the contour freehand, at which point
/// scaling the analyzed drift is no longer what is on screen. Callers
/// that have only moved the sliders want this; callers that have drawn
/// want [`apply_to`] alone.
pub fn apply_blend(blob: &mut NoteBlob, drift_amount: f64, modulation_amount: f64) {
    blob.drift_amount = drift_amount.max(0.0);
    blob.modulation_amount = modulation_amount.max(0.0);
}
