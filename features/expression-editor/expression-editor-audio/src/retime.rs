//! Timing edits as stretch markers rather than as rendered audio.
//!
//! A note that has been moved or stretched needs the host to play its
//! source content at a different time. That can be done by rendering
//! new audio, or by telling the host where the content goes — and the
//! second is better in every way that matters: the media is untouched,
//! the edit is reversible, and it stays adjustable in the host
//! afterwards rather than being baked in.
//!
//! ## The interaction that has to be right
//!
//! Pitch edits *do* need a render, and a render is produced from the
//! analysis. If that render also applied the warp, the timing would be
//! applied **twice** — once baked into the audio and again by the
//! markers on top. So the render is always pitch-only, and timing lives
//! exclusively in the markers. [`crate::Analysis::commit`] keeps them
//! separate for exactly this reason.

use daw::service::StretchMarker;
use expression_editor_core::{ExpressionDoc, NoteId};
use tune_dsp::model::PitchDoc;

/// Where the take sits, which decides how document time converts.
///
/// The editor works in analysis frames from the start of the take's
/// audio. A marker is in take playback time and source time, and
/// getting from one to the other needs the item's own placement — see
/// [`StretchMarker`] for why those are three different numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TakePlacement {
    /// Item position on the timeline, seconds.
    pub item_position: f64,
    /// Take start offset into the source, seconds.
    pub start_offset: f64,
    /// Take play rate.
    pub play_rate: f64,
    /// Analysis frames per second.
    pub frame_rate: f64,
}

impl TakePlacement {
    /// The simple case: item at zero, no offset, unity rate.
    pub fn unit(frame_rate: f64) -> Self {
        Self {
            item_position: 0.0,
            start_offset: 0.0,
            play_rate: 1.0,
            frame_rate,
        }
    }

    fn secs(&self, frames: f64) -> f64 {
        frames / self.frame_rate.max(1e-9)
    }
}

/// Stretch markers for the timing edits in `doc`, against `pitch`.
///
/// One pair per moved note: where its start now plays, and where its
/// end now plays. Everything between two markers is stretched to fit,
/// so a note that was moved *and* lengthened needs both.
///
/// Returns empty when nothing moved, which is the common case and the
/// one worth being cheap: an untouched take must not acquire a warp
/// just for being opened.
pub fn stretch_markers(
    doc: &ExpressionDoc,
    pitch: &PitchDoc,
    placement: TakePlacement,
) -> Vec<StretchMarker> {
    let mut out = Vec::new();
    let mut moved = false;

    for (i, blob) in pitch.blobs.iter().enumerate() {
        let Some(note) = doc.note(NoteId(i as u64 + 1)) else {
            continue;
        };
        let analyzed = (blob.start_frame as f64, blob.end_frame as f64);
        if (note.start - analyzed.0).abs() > 1e-9 || (note.end - analyzed.1).abs() > 1e-9 {
            moved = true;
        }
        for (now, was) in [(note.start, analyzed.0), (note.end, analyzed.1)] {
            // `at_project_time` wants project times, and both of ours
            // are relative to the take's audio — so the item position
            // is added back on the way in and taken off again inside.
            out.push(StretchMarker::at_project_time(
                placement.item_position + placement.secs(now),
                placement.item_position + placement.secs(was),
                placement.item_position,
                placement.start_offset,
                placement.play_rate,
            ));
        }
    }

    if !moved {
        return Vec::new();
    }
    // Sorted and deduplicated: abutting notes share an edge, and a map
    // with a repeated knot has an undefined slope there.
    out.sort_by(|a, b| a.position.total_cmp(&b.position));
    out.dedup_by(|a, b| (a.position - b.position).abs() < 1e-9);
    out
}

/// Whether the user changed anything that only a resynthesis can carry.
///
/// Compared **document to document**, not against the analysis: by the
/// time a write-back runs, `commit` has already pushed the edits into
/// the blobs, so asking the analysis whether it differs from the
/// document would always answer no. The baseline is what the take
/// looked like when it was opened or last written, which is the thing
/// the question is actually about.
///
/// Timing is deliberately *not* here. A moved note is carried by the
/// host as stretch markers, losslessly, and rendering for it would cost
/// a generation of quality for nothing.
pub fn pitch_changed(baseline: &ExpressionDoc, current: &ExpressionDoc) -> bool {
    if baseline.notes.len() != current.notes.len() {
        // A note added or removed changes what sounds.
        return true;
    }
    for was in &baseline.notes {
        let Some(now) = current.note(was.id) else {
            return true;
        };
        if now.row != was.row || now.pitch != was.pitch {
            return true;
        }
        // Formant and gain are resynthesis parameters too: both are
        // read by the renderer and neither has a host-side equivalent.
        if now.timbre != was.timbre || now.pressure != was.pressure {
            return true;
        }
    }
    false
}
