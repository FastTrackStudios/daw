//! Tune DSP — pitch detection, note extraction, and correction.
//!
//! `tune` is the pitch-editing FX: a Melodyne/Auto-Tune-style engine built as a
//! pipeline over the existing pitch-shift core [`pitch_dsp`]:
//!
//! 1. [`detect`] — YIN monophonic f0 estimation → a per-frame pitch track.
//! 2. [`note`] — group frames into editable [`note::Note`]s (median pitch).
//! 3. [`correct`] — snap each note to a [`correct::Scale`] → per-note shift
//!    ratios.
//! 4. Resynthesis — feed those ratios to [`pitch_dsp`] (PSOLA/WSOLA), so the
//!    actual formant-aware shifting is *not* reinvented here.
//!
//! This crate is the monophonic foundation. Polyphonic detection + a note-graph
//! editing surface (the full Melodyne competitor) build on top of these types.
//! DSP style matches the sibling fx crates: plain `std`, `f64`, allocation at
//! construction only.

pub mod correct;
pub mod detect;
pub mod dna;
pub mod note;
pub mod tracker;

pub use correct::{correct_notes, CorrectConfig, NoteCorrection, Scale};
pub use dna::{DnaConfig, DnaEngine, NoteSpan, SeparatedNote};
pub use tracker::{spans_from_frames, track_notes, TrackConfig, TrackedNote};
pub use detect::{hz_to_midi, midi_to_hz, PitchFrame, YinConfig, YinDetector};
pub use note::{segment_notes, Note, NoteConfig};

/// Re-export the shared pitch-shift core the correction stage drives.
pub use pitch_dsp as shifter;

/// One-shot analysis config bundling the three stages.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyzeConfig {
    /// YIN detection config.
    pub yin: YinConfig,
    /// Note-segmentation config.
    pub note: NoteConfig,
}

/// Result of analysing a monophonic buffer.
#[derive(Clone, Debug)]
pub struct TuneAnalysis {
    /// Per-frame pitch track.
    pub frames: Vec<PitchFrame>,
    /// Extracted notes.
    pub notes: Vec<Note>,
    /// Hop size (samples) between analysis frames.
    pub hop: usize,
    /// Sample rate the analysis ran at.
    pub sample_rate: f64,
}

/// Analyse a mono buffer into a pitch track and notes.
///
/// Frames are taken every `hop` samples (defaults to a quarter window) using an
/// overlapping YIN window. This is the offline/edit-time path; a realtime
/// note-locked corrector is the next layer.
pub fn analyze(samples: &[f64], sample_rate: f64, cfg: AnalyzeConfig) -> TuneAnalysis {
    let mut yin = YinDetector::new(sample_rate, cfg.yin);
    let window = yin.window();
    let hop = (window / 4).max(1);

    let mut frames = Vec::new();
    let mut pos = 0;
    while pos + window <= samples.len() {
        frames.push(yin.detect(&samples[pos..pos + window]));
        pos += hop;
    }
    let notes = segment_notes(&frames, cfg.note);

    TuneAnalysis {
        frames,
        notes,
        hop,
        sample_rate,
    }
}
