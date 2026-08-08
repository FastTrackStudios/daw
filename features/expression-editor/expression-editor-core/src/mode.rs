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
use crate::rows::{DrumMap, RowSpace, StringTuning};
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
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Midi,
        Mode::Mpe,
        Mode::Drums,
        Mode::Guitar,
        Mode::Vocals,
        Mode::Audio,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Mode::Midi => "MIDI",
            Mode::Mpe => "MPE",
            Mode::Drums => "Drums",
            Mode::Guitar => "Guitar",
            Mode::Vocals => "Vocals",
            Mode::Audio => "Audio",
        }
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
        !matches!(self, Mode::Drums)
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

    pub fn default_row_space(&self) -> RowSpace {
        match self {
            Mode::Drums => RowSpace::Drums(DrumMap::general_midi()),
            Mode::Guitar => RowSpace::Strings(StringTuning::guitar_standard()),
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
