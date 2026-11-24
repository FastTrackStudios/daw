//! The types of hit placement (`daw.edit`'s `PLACE`): a hit and how far it
//! moves, how a cut is made around an attack, and a piece of the recording
//! laid on the timeline. The algorithm is an extension's (rhythm's, or
//! another), never the DAW's: the DAW only applies the pieces it answers.

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cut {
    /// How far before a hit's attack its piece starts: the stick's
    /// approach is in the recording just before the transient.
    pub lead: f64,
    /// How far past a hit's attack the piece after may not reach back: a
    /// hit played twice is a flam.
    pub guard: f64,
}

impl Default for Cut {
    fn default() -> Self {
        Self {
            lead: 0.005,
            guard: 0.015,
        }
    }
}

/// A hit as recorded and how far it is moved, seconds.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hit {
    /// Its attack, in the recording's own time.
    pub at: f64,
    /// Where it plays minus where it was recorded.
    pub shift: f64,
}

/// A stretch of the recording played somewhere on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Piece {
    /// Where it starts playing, timeline seconds.
    pub start: f64,
    /// Where it stops, timeline seconds.
    pub end: f64,
    /// Timeline minus recording time: it plays the recording from
    /// `start - shift`.
    pub shift: f64,
    /// The last resort: the recording only reaches `end` naturally, and
    /// its tail is drawn out to here to meet the next piece.
    pub drawn_to: Option<f64>,
}

impl Piece {
    /// Where it stops sounding: drawn out, or its natural end.
    #[must_use]
    pub fn reach(&self) -> f64 {
        self.drawn_to.unwrap_or(self.end)
    }
}
