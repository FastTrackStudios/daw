//! Editor modes.
//!
//! One surface serves several products, and each wants a different set
//! of controls. Rather than showing every control always and letting
//! the user ignore four fifths of it, the mode decides what is on
//! screen — MPE channel controls only appear in MPE, technique controls
//! only on a string roll, lyric controls only for vocals.
//!
//! A mode is a *preset*, not a lock: it sets the row space, the mouse
//! map, the visible lanes and the strip, and the user can then change
//! any of them. Switching modes re-applies the preset.

use crate::mouse::MouseMap;
use crate::rows::{DrumMap, RowSpace, SliceBands, StringTuning};
use crate::{Lane, StripLane};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// Plain MIDI: notes and velocity, nothing else.
    #[default]
    Midi,
    /// Per-note pitch bend, pressure and timbre.
    Mpe,
    /// Named kit lanes, triangle heads, paint-on-drag.
    Drums,
    /// String roll with frets and techniques.
    Guitar,
    /// Pitch rows with lyric syllables.
    Vocals,
    /// Analyzed audio — the Melodyne surface. Same components, notes
    /// sourced from pitch tracking instead of MIDI.
    Audio,
    /// Analysed audio with no pitch to speak of: drums, percussion,
    /// noise. Hits on spectral bands rather than notes on a scale.
    ///
    /// Separate from [`Mode::Audio`] because a pitch contour taken from
    /// unpitched material is noise — it yields notes that flicker
    /// between octaves and a blob that wanders the grid. Editing what
    /// is actually there (when each hit lands, how hard) beats editing
    /// a pitch that was never in the recording.
    Percussive,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::Midi,
        Mode::Mpe,
        Mode::Drums,
        Mode::Guitar,
        Mode::Vocals,
        Mode::Audio,
        Mode::Percussive,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Mode::Midi => "MIDI",
            Mode::Mpe => "MPE",
            Mode::Drums => "Drums",
            Mode::Guitar => "Guitar",
            Mode::Vocals => "Vocals",
            Mode::Audio => "Audio",
            Mode::Percussive => "Percussive",
        }
    }

    /// Whether this mode's notes came from analysing a recording.
    pub fn is_analysed_audio(&self) -> bool {
        matches!(self, Mode::Audio | Mode::Percussive)
    }

    /// Whether notes draw as hits on a waveform rather than as a roll.
    pub fn draws_slices(&self) -> bool {
        matches!(self, Mode::Percussive)
    }

    /// Whether a note in this mode has a pitch worth editing at all.
    ///
    /// The one question that decides whether pitch drawing, the tuning
    /// grid and transpose are offered. A slice answers no, and hiding
    /// those beats offering an edit that cannot land: percussive audio
    /// is written as stretch markers and envelope points, never as a
    /// re-render, so there is nothing for a pitch change to write to.
    pub fn has_pitch(&self) -> bool {
        !matches!(self, Mode::Percussive)
    }

    /// Whether the per-note expression lanes are meaningful.
    ///
    /// Plain MIDI has no per-note pressure or timbre — showing those
    /// lane buttons would offer an edit the format cannot carry.
    pub fn has_expression_lanes(&self) -> bool {
        matches!(self, Mode::Mpe | Mode::Audio)
    }

    /// Whether MPE channel management applies.
    pub fn has_mpe_channels(&self) -> bool {
        matches!(self, Mode::Mpe)
    }

    /// Whether guitar technique controls apply.
    pub fn has_techniques(&self) -> bool {
        matches!(self, Mode::Guitar)
    }

    /// Whether lyric editing applies.
    pub fn has_lyrics(&self) -> bool {
        matches!(self, Mode::Vocals)
    }

    /// Whether the drift/vibrato blend controls apply.
    ///
    /// They need a continuous pitch contour to decompose. A plain MIDI
    /// note has a flat one, so the controls would do nothing.
    pub fn has_pitch_shape(&self) -> bool {
        matches!(self, Mode::Audio | Mode::Mpe | Mode::Vocals)
    }

    /// Whether notes draw as amplitude blobs rather than bars.
    ///
    /// A sung note is not a rectangle. Its body follows the amplitude
    /// envelope and rides the pitch contour, which is what makes a
    /// breathy tail or a hard consonant visible at a glance — and what
    /// makes the pitch line's excursions readable *against* the note
    /// instead of inside a box that hides them.
    ///
    /// This tracks the mode rather than the row space: an audio editor
    /// and a MIDI editor are both `RowSpace::Pitch`, and only one of
    /// them has an envelope to draw.
    pub fn draws_blobs(&self) -> bool {
        matches!(self, Mode::Audio | Mode::Vocals)
    }

    /// Whether the note carries the seven drag handles.
    ///
    /// They edit a *contour* — pitch centre, its slopes, its vibrato
    /// depth, plus formant and gain trims. A plain MIDI or drum note
    /// has none of that, so the handles would be six inert targets
    /// cluttering every note.
    pub fn has_handles(&self) -> bool {
        matches!(self, Mode::Audio | Mode::Vocals)
    }

    /// Whether microtonal tuning targets are worth showing.
    pub fn has_tuning(&self) -> bool {
        !matches!(self, Mode::Drums | Mode::Percussive)
    }

    /// Which expression lanes start visible.
    pub fn default_overlays(&self) -> Vec<Lane> {
        if self.has_expression_lanes() {
            vec![Lane::Pitch]
        } else {
            Vec::new()
        }
    }

    pub fn default_strip(&self) -> StripLane {
        match self {
            Mode::Mpe | Mode::Audio => StripLane::Expression(Lane::Pressure),
            _ => StripLane::Velocity,
        }
    }

    /// Natural height of this mode's lane in a stacked view, relative
    /// to the other lanes.
    ///
    /// Rough, and meant to be: it encodes how many rows the surface
    /// needs to be readable at all — a slice strip has three bands, a
    /// string roll six, a vocal wants a couple of octaves — not a
    /// preference about importance.
    pub fn stack_weight(&self) -> f32 {
        match self {
            Mode::Percussive => 0.6,
            Mode::Drums => 1.0,
            Mode::Guitar => 0.8,
            Mode::Audio | Mode::Vocals => 1.4,
            Mode::Midi | Mode::Mpe => 1.0,
        }
    }

    pub fn default_row_space(&self) -> RowSpace {
        match self {
            Mode::Drums => RowSpace::Drums(DrumMap::general_midi()),
            Mode::Guitar => RowSpace::Strings(StringTuning::guitar_standard()),
            Mode::Percussive => RowSpace::Bands(SliceBands::default()),
            _ => RowSpace::Pitch,
        }
    }

    pub fn default_mouse(&self) -> MouseMap {
        match self {
            Mode::Drums => MouseMap::drums(),
            Mode::Guitar => MouseMap::riffer(),
            Mode::Vocals => MouseMap::lyrics(),
            _ => MouseMap::reaper_like(),
        }
    }
}
