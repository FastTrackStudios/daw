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
//! | drums          | named drum dimension  | drum name  |
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
        self.open_pitches.get(string).copied().unwrap_or(40) + self.capo as i32
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

/// A named drum dimension.
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
    /// sequence. A full 47-dimension GM map is unusable as a roll.
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
/// The bands a percussive track's hits are sorted into.
///
/// Split points are spectral centroids in Hz, ascending, and there is
/// one more band than there are splits. Three by default — kick low,
/// snare middle, cymbals high — which is enough to read a kit at a
/// glance without pretending to identify the drums.
///
/// Deliberately not a [`DrumMap`]: this makes no claim to know a kick
/// from a floor tom, only that one hit is darker than another. Naming
/// lanes is what [`crate::Mode::Drums`] is for, and that mode has MIDI
/// note numbers to name them from.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceBands {
    /// Ascending centroid boundaries in Hz. `n` splits, `n + 1` bands.
    pub splits: Vec<f64>,
    pub names: Vec<String>,
}

impl Default for SliceBands {
    fn default() -> Self {
        Self {
            // 250 Hz and 2 kHz: below the first is body, above the
            // second is almost entirely cymbal and snare snap. The
            // exact numbers matter less than that they are editable —
            // a shaker track wants both moved up.
            splits: vec![250.0, 2000.0],
            names: vec!["Low".into(), "Mid".into(), "High".into()],
        }
    }
}

impl SliceBands {
    pub fn count(&self) -> usize {
        self.splits.len() + 1
    }

    /// Which band a hit with this spectral centroid belongs to.
    ///
    /// Banding is a view, not a commitment: a slice keeps its measured
    /// centroid, so moving a split re-sorts without losing anything.
    pub fn band_of(&self, centroid_hz: f64) -> usize {
        self.splits
            .iter()
            .position(|&s| centroid_hz < s)
            .unwrap_or(self.splits.len())
    }

    pub fn name(&self, band: usize) -> &str {
        self.names.get(band).map(String::as_str).unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum RowSpace {
    /// Rows are MIDI pitches. Audio, MPE, plain MIDI, vocals.
    #[default]
    Pitch,
    /// Rows are drum lanes; `row` indexes [`DrumMap::lanes`].
    Drums(DrumMap),
    /// Rows are strings; the note's `fret` completes the position.
    Strings(StringTuning),
    /// Rows are spectral bands; a note is a percussive hit, not a
    /// pitch. See [`SliceBands`].
    Bands(SliceBands),
}

impl RowSpace {
    /// Whether two row spaces are the same *kind*, ignoring their
    /// contents.
    ///
    /// What re-applying a mode preset needs to know: a document already
    /// in band space keeps its own splits, because the user may have
    /// moved them and a preset must not undo that. A document in some
    /// other space is converted.
    pub fn same_kind(&self, other: &RowSpace) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }

    /// Inclusive row range the roll can show.
    pub fn bounds(&self) -> (i32, i32) {
        match self {
            RowSpace::Pitch => (0, 127),
            RowSpace::Drums(m) => (0, m.lanes.len().saturating_sub(1) as i32),
            RowSpace::Strings(t) => (0, t.strings().saturating_sub(1) as i32),
            RowSpace::Bands(b) => (0, b.count().saturating_sub(1) as i32),
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
            RowSpace::Bands(b) => b.name(row.max(0) as usize).to_string(),
        }
    }

    /// Rows that get a heavier divider (C in pitch space, every string
    /// in a string roll, group boundaries in drums).
    pub fn is_major_row(&self, row: i32) -> bool {
        match self {
            RowSpace::Pitch => row.rem_euclid(12) == 0,
            RowSpace::Drums(_) => false,
            RowSpace::Strings(_) => true,
            // Every band boundary is a real division of the material.
            RowSpace::Bands(_) => true,
        }
    }

    /// How many semitones one row is worth, when a pitch curve is drawn
    /// against it.
    ///
    /// In pitch space a row *is* a semitone and this is 1. Everywhere
    /// else the row axis is not pitch at all, and drawing a semitone
    /// curve at one-row-per-semitone is a unit error: a whole-tone bend
    /// on a string roll would deflect the line across two neighbouring
    /// strings.
    ///
    /// Two semitones per string row is the whole-step bend a guitarist
    /// actually plays, drawn as exactly one row of deflection — the
    /// largest interval that reads as "a bend" rather than "a note
    /// somewhere else".
    pub fn semitones_per_row(&self) -> f64 {
        match self {
            RowSpace::Pitch => 1.0,
            RowSpace::Strings(_) => 2.0,
            // Neither space has a pitch axis to scale against; the
            // curves drawn over them are decoration, not a reading.
            RowSpace::Drums(_) | RowSpace::Bands(_) => 1.0,
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
            // A slice has no pitch. Callers that need one for playback
            // get silence rather than a number that would sound.
            RowSpace::Bands(_) => 0,
        }
    }

    /// Row a sounding pitch belongs on, for import.
    pub fn row_of_pitch(&self, pitch: i32) -> Option<i32> {
        match self {
            RowSpace::Pitch => Some(pitch.clamp(0, 127)),
            RowSpace::Drums(m) => m.row_of_pitch(pitch).map(|r| r as i32),
            RowSpace::Strings(t) => t.best_position(pitch, 5).map(|(s, _)| s as i32),
            // Nothing imports *into* band space by pitch: a band comes
            // from a measured centroid, and a MIDI note has none.
            RowSpace::Bands(_) => None,
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
            // The band is already the row, and a hit is too narrow to
            // print in anyway.
            RowSpace::Bands(_) => None,
        }
    }

    /// How a note body is drawn in this space.
    pub fn note_shape(&self) -> NoteShape {
        match self {
            RowSpace::Drums(_) => NoteShape::Triangle,
            // A hit is an attack with a decay, which is what the
            // triangle already says — and it puts the flat edge on the
            // onset, the one part of a slice you align against.
            RowSpace::Bands(_) => NoteShape::Triangle,
            _ => NoteShape::Bar,
        }
    }

    /// Per-row colour, when the row itself carries meaning.
    ///
    /// A string roll colours by *string*, not by pitch class: on a
    /// guitar the string a note is played on is the thing you are
    /// tracking, and pitch-class colour would scatter one string's part
    /// across six hues. Drums colour by kit section for the same
    /// reason. Pitch space returns `None` and keeps pitch-class colour.
    pub fn row_color(&self, row: i32) -> Option<&'static str> {
        match self {
            RowSpace::Pitch => None,
            RowSpace::Strings(_) => Some(STRING_COLORS[row.max(0) as usize % STRING_COLORS.len()]),
            RowSpace::Drums(m) => {
                let name = m.lanes.get(row.max(0) as usize)?.name.as_str();
                Some(drum_color(name))
            }
            // Dark to bright as the band rises, so a kit reads the way
            // it sounds without naming a single drum.
            RowSpace::Bands(_) => Some(BAND_COLORS[row.max(0) as usize % BAND_COLORS.len()]),
        }
    }
}

/// Band colours, dark to bright as the centroid rises.
const BAND_COLORS: [&str; 4] = ["#7a5cff", "#3fa9f5", "#4fd1a5", "#ffd166"];

/// How a note body is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteShape {
    /// A rectangle spanning the note's length.
    Bar,
    /// A right-pointing triangle with its flat edge on the onset.
    ///
    /// A drum hit has no meaningful length, so it needs a fixed-size
    /// head rather than a bar — a bar invites editing a duration
    /// nothing will hear. A triangle beats a diamond because its flat
    /// edge sits exactly on the attack, where a diamond's widest point
    /// is its middle and the onset has to be inferred.
    Triangle,
}

/// String colours, low to high. Warm at the bottom, cool at the top,
/// so the register reads without counting rows.
pub const STRING_COLORS: [&str; 8] = [
    "#f97316", // low — orange
    "#f59e0b", "#eab308", "#84cc16", "#22d3ee", "#60a5fa", // high — blue
    "#a78bfa", "#f472b6",
];

/// Kit-section colour, so a groove reads at a glance.
fn drum_color(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.contains("kick") {
        "#ef4444"
    } else if n.contains("snare") || n.contains("stick") || n.contains("clap") {
        "#f59e0b"
    } else if n.contains("hh") || n.contains("hat") {
        "#22d3ee"
    } else if n.contains("tom") {
        "#a3e635"
    } else if n.contains("ride") {
        "#60a5fa"
    } else {
        "#c084fc"
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
