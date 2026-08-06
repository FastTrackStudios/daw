//! What a row *means*.
//!
//! Every product this editor serves differs mainly in one thing: what
//! the vertical axis is.
//!
//! | product        | row              | note label |
//! |----------------|------------------|------------|
//! | audio pitch    | MIDI pitch       | note name  |
//! | MPE / MIDI     | MIDI pitch       | note name  |
//! | vocal lyrics   | MIDI pitch       | syllable   |
//! | drums          | named drum lane  | drum name  |
//! | guitar / bass  | **string**       | **fret**   |
//!
//! So the row axis is abstracted here rather than hard-coded as pitch.
//! A guitar "string roll" (Ample's Riffer) is the interesting case: the
//! row is the string, the note carries a fret, and sounding pitch is
//! `open_pitch[string] + fret`. Everything else in the editor —
//! gestures, zones, curves, the camera — is unchanged, because it all
//! works in *row* space already.

use crate::doc::Note;
use crate::tuning;

/// A guitar or bass tuning: open pitch of each string.
///
/// Index 0 is the **lowest** string, matching how tunings are written
/// (E A D G B E). The roll draws them inverted so the high string is on
/// top, the way tab is read.
#[derive(Clone, Debug, PartialEq)]
pub struct StringTuning {
    pub name: &'static str,
    pub open_pitches: Vec<i32>,
    /// Highest fret the instrument has.
    pub frets: u8,
    /// Capo position; open strings sound this many semitones higher.
    pub capo: u8,
}

impl StringTuning {
    pub fn guitar_standard() -> Self {
        Self {
            name: "Guitar (E A D G B E)",
            open_pitches: vec![40, 45, 50, 55, 59, 64],
            frets: 22,
            capo: 0,
        }
    }

    pub fn guitar_drop_d() -> Self {
        Self {
            name: "Guitar (Drop D)",
            open_pitches: vec![38, 45, 50, 55, 59, 64],
            frets: 22,
            capo: 0,
        }
    }

    pub fn bass_4() -> Self {
        Self {
            name: "Bass (E A D G)",
            open_pitches: vec![28, 33, 38, 43],
            frets: 24,
            capo: 0,
        }
    }

    pub fn bass_5() -> Self {
        Self {
            name: "Bass 5 (B E A D G)",
            open_pitches: vec![23, 28, 33, 38, 43],
            frets: 24,
            capo: 0,
        }
    }

    pub fn strings(&self) -> usize {
        self.open_pitches.len()
    }

    /// Open pitch of a string, capo included.
    pub fn open(&self, string: usize) -> i32 {
        self.open_pitches
            .get(string)
            .copied()
            .unwrap_or(40)
            + self.capo as i32
    }

    /// Sounding pitch of a fingered position.
    pub fn pitch(&self, string: usize, fret: u8) -> i32 {
        self.open(string) + fret as i32
    }

    /// Fret that puts `pitch` on `string`, if it is reachable.
    pub fn fret_for(&self, string: usize, pitch: i32) -> Option<u8> {
        let f = pitch - self.open(string);
        (f >= 0 && f <= self.frets as i32).then_some(f as u8)
    }

    /// The playable position for `pitch` nearest `preferred_fret`.
    ///
    /// Guitarists do not play the lowest available fret; they play the
    /// one nearest the hand. Biasing toward the current position is
    /// what makes imported MIDI land somewhere playable instead of
    /// scattered down at the nut.
    pub fn best_position(&self, pitch: i32, preferred_fret: u8) -> Option<(usize, u8)> {
        (0..self.strings())
            .filter_map(|s| self.fret_for(s, pitch).map(|f| (s, f)))
            .min_by_key(|&(_, f)| (f as i32 - preferred_fret as i32).abs())
    }
}

/// A named drum lane.
#[derive(Clone, Debug, PartialEq)]
pub struct DrumLane {
    pub pitch: i32,
    pub name: String,
    /// Lanes sharing a group choke each other (hi-hat open/closed).
    pub group: Option<u8>,
}

/// An ordered set of drum lanes — the rows of a drum editor.
#[derive(Clone, Debug, PartialEq)]
pub struct DrumMap {
    pub name: &'static str,
    /// Ordered low-to-high as drawn (kick at the bottom).
    pub lanes: Vec<DrumLane>,
}

impl DrumMap {
    /// General MIDI, trimmed to the kit pieces people actually
    /// sequence. A full 47-lane GM map is unusable as a roll.
    pub fn general_midi() -> Self {
        let lanes = [
            (35, "Kick 2", None),
            (36, "Kick", None),
            (37, "Side Stick", None),
            (38, "Snare", None),
            (39, "Clap", None),
            (40, "Snare 2", None),
            (41, "Tom Floor L", None),
            (42, "HH Closed", Some(1)),
            (43, "Tom Floor H", None),
            (44, "HH Pedal", Some(1)),
            (45, "Tom Low", None),
            (46, "HH Open", Some(1)),
            (47, "Tom Low-Mid", None),
            (48, "Tom Hi-Mid", None),
            (49, "Crash", None),
            (50, "Tom High", None),
            (51, "Ride", None),
            (52, "China", None),
            (53, "Ride Bell", None),
            (54, "Tambourine", None),
            (55, "Splash", None),
            (57, "Crash 2", None),
            (59, "Ride 2", None),
        ];
        Self {
            name: "General MIDI",
            lanes: lanes
                .iter()
                .map(|&(pitch, name, group)| DrumLane {
                    pitch,
                    name: name.to_string(),
                    group,
                })
                .collect(),
        }
    }

    pub fn row_of_pitch(&self, pitch: i32) -> Option<usize> {
        self.lanes.iter().position(|l| l.pitch == pitch)
    }

    pub fn pitch_of_row(&self, row: usize) -> Option<i32> {
        self.lanes.get(row).map(|l| l.pitch)
    }
}

/// What the vertical axis means.
#[derive(Clone, Debug, PartialEq)]
pub enum RowSpace {
    /// Rows are MIDI pitches. Audio, MPE, plain MIDI, vocals.
    Pitch,
    /// Rows are drum lanes; `row` indexes [`DrumMap::lanes`].
    Drums(DrumMap),
    /// Rows are strings; the note's `fret` completes the position.
    Strings(StringTuning),
}

impl Default for RowSpace {
    fn default() -> Self {
        RowSpace::Pitch
    }
}

impl RowSpace {
    /// Inclusive row range the roll can show.
    pub fn bounds(&self) -> (i32, i32) {
        match self {
            RowSpace::Pitch => (0, 127),
            RowSpace::Drums(m) => (0, m.lanes.len().saturating_sub(1) as i32),
            RowSpace::Strings(t) => (0, t.strings().saturating_sub(1) as i32),
        }
    }

    /// Rows are drawn with the *last* index on top for pitch (higher
    /// pitch is higher), but strings read like tab — string 0 is the
    /// lowest and belongs at the bottom, which is the same direction.
    /// Drum lanes follow their declared order, kick at the bottom.
    pub fn row_label(&self, row: i32) -> String {
        match self {
            RowSpace::Pitch => tuning::note_name(row),
            RowSpace::Drums(m) => m
                .lanes
                .get(row.max(0) as usize)
                .map(|l| l.name.clone())
                .unwrap_or_default(),
            RowSpace::Strings(t) => {
                let open = t.open(row.max(0) as usize);
                // Pitch class only: "E", "A" — the string's name, not
                // its octave, which is how players refer to them.
                tuning::pitch_class_name(open).to_string()
            }
        }
    }

    /// Rows that get a heavier divider (C in pitch space, every string
    /// in a string roll, group boundaries in drums).
    pub fn is_major_row(&self, row: i32) -> bool {
        match self {
            RowSpace::Pitch => row.rem_euclid(12) == 0,
            RowSpace::Drums(_) => false,
            RowSpace::Strings(_) => true,
        }
    }

    /// Black-key shading only means anything in pitch space.
    pub fn is_accidental(&self, row: i32) -> bool {
        match self {
            RowSpace::Pitch => matches!(row.rem_euclid(12), 1 | 3 | 6 | 8 | 10),
            _ => false,
        }
    }

    /// Sounding MIDI pitch of a note in this space.
    pub fn pitch_of(&self, note: &Note) -> i32 {
        match self {
            RowSpace::Pitch => note.row,
            RowSpace::Drums(m) => m.pitch_of_row(note.row.max(0) as usize).unwrap_or(note.row),
            RowSpace::Strings(t) => t.pitch(note.row.max(0) as usize, note.fret.unwrap_or(0)),
        }
    }

    /// Row a sounding pitch belongs on, for import.
    pub fn row_of_pitch(&self, pitch: i32) -> Option<i32> {
        match self {
            RowSpace::Pitch => Some(pitch.clamp(0, 127)),
            RowSpace::Drums(m) => m.row_of_pitch(pitch).map(|r| r as i32),
            RowSpace::Strings(t) => t.best_position(pitch, 5).map(|(s, _)| s as i32),
        }
    }

    /// What the note body prints.
    ///
    /// A lyric always wins: in a vocal editor the syllable *is* the
    /// note's identity, and the pitch is already shown by its row.
    pub fn note_label(&self, note: &Note) -> Option<String> {
        if let Some(text) = &note.text {
            return Some(text.clone());
        }
        match self {
            RowSpace::Pitch => Some(tuning::note_name(note.row)),
            RowSpace::Drums(_) => None,
            // The fret number is the whole point of a tab view.
            RowSpace::Strings(_) => Some(note.fret.unwrap_or(0).to_string()),
        }
    }

    /// Drums are drawn as fixed-size diamonds: a drum hit has no
    /// meaningful length, and drawing one as a bar invites editing a
    /// duration that nothing will hear.
    pub fn draws_diamonds(&self) -> bool {
        matches!(self, RowSpace::Drums(_))
    }
}

/// Playing techniques, following Ample Sound's Riffer set — the
/// vocabulary a sampled guitar/bass actually responds to.
///
/// Which of these an instrument supports varies; the editor offers them
/// all and the host filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Articulation {
    Sustain,
    PalmMute,
    NaturalHarmonic,
    Staccato,
    Slap,
    Pop,
    Tap,
    SlideIn,
    SlideOut,
    HammerOn,
    PullOff,
    LegatoSlide,
    Bend,
    Vibrato,
    SlideGuitar,
    /// Vocal/percussion dead note.
    Mute,
}

impl Articulation {
    pub const ALL: [Articulation; 16] = [
        Articulation::Sustain,
        Articulation::PalmMute,
        Articulation::NaturalHarmonic,
        Articulation::Staccato,
        Articulation::Slap,
        Articulation::Pop,
        Articulation::Tap,
        Articulation::SlideIn,
        Articulation::SlideOut,
        Articulation::HammerOn,
        Articulation::PullOff,
        Articulation::LegatoSlide,
        Articulation::Bend,
        Articulation::Vibrato,
        Articulation::SlideGuitar,
        Articulation::Mute,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Articulation::Sustain => "Sustain",
            Articulation::PalmMute => "Palm Mute",
            Articulation::NaturalHarmonic => "Nat. Harmonic",
            Articulation::Staccato => "Staccato",
            Articulation::Slap => "Slap",
            Articulation::Pop => "Pop",
            Articulation::Tap => "Tap",
            Articulation::SlideIn => "Slide In",
            Articulation::SlideOut => "Slide Out",
            Articulation::HammerOn => "Hammer On",
            Articulation::PullOff => "Pull Off",
            Articulation::LegatoSlide => "Legato Slide",
            Articulation::Bend => "Bend",
            Articulation::Vibrato => "Vibrato",
            Articulation::SlideGuitar => "Slide Guitar",
            Articulation::Mute => "Dead Note",
        }
    }

    /// Single-glyph badge for the note body.
    pub fn glyph(&self) -> &'static str {
        match self {
            Articulation::Sustain => "",
            Articulation::PalmMute => "P.M.",
            Articulation::NaturalHarmonic => "◇",
            Articulation::Staccato => "·",
            Articulation::Slap => "S",
            Articulation::Pop => "P",
            Articulation::Tap => "T",
            Articulation::SlideIn => "╱",
            Articulation::SlideOut => "╲",
            Articulation::HammerOn => "H",
            Articulation::PullOff => "p",
            Articulation::LegatoSlide => "≈",
            Articulation::Bend => "↑",
            Articulation::Vibrato => "∿",
            Articulation::SlideGuitar => "⌇",
            Articulation::Mute => "✕",
        }
    }

    /// Techniques that only make sense joined to the following note on
    /// the same string. Riffer's rule: the legato is marked on the
    /// *first* note of the pair.
    pub fn is_legato(&self) -> bool {
        matches!(
            self,
            Articulation::HammerOn | Articulation::PullOff | Articulation::LegatoSlide
        )
    }

    /// Natural harmonics only speak at these frets.
    pub fn valid_frets(&self) -> Option<&'static [u8]> {
        match self {
            Articulation::NaturalHarmonic => Some(&[5, 7, 9, 12, 17, 19, 24]),
            _ => None,
        }
    }
}
