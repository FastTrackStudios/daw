//! The types of hit placement (`daw.edit`'s `PLACE`).
//!
//! A hit and how far it moves, how a cut is made around an attack, the
//! takes the recording is made of, how a gap is filled, a piece of the
//! recording laid on the timeline and what was done at each gap. The
//! algorithm is an extension's (rhythm's, or another), never the DAW's: the
//! DAW only applies the pieces it answers.
//!
//! A *gap* is where a hit moved later than the piece before can play on
//! to without sounding the next attack in its recording. 0.3 fills it
//! (the algorithm's rule; rhythm's): the piece before plays on at its own
//! speed up to just before that attack, the piece after reaches back into
//! its own take before its attack, the two crossfade where they overlap,
//! and only what is still left is stretched — never slower than
//! [`Fill::least_rate`], over the shortest span that allows — or, past
//! that, left as a gap. A hit is never moved to fill one.
//!
//! 0.3.1 fills what that leaves, in this order: the piece after may
//! replay a bit of the previous stroke's decay in its own recording
//! (reaching back past [`Cut::guard`], never into that stroke's first
//! [`Fill::keep`], so no attack sounds twice: [`Filled::replayed_decay`]);
//! then what is still left is stretched slower than [`Fill::least_rate`]
//! if it must, down to [`Fill::least_rate_leftover`]; only past both is a
//! short dip left, each side fading within itself (a piece's `crossfade`
//! of `Some(0.0)`), never a hard cut: [`Filled::left`].
//!
//! 0.3.2, the owner's rule: an edit never leaves silence. The replay may
//! reach as close to the previous stroke's attack as it must — at least
//! [`Fill::keep`] past it if it can, never nearer than [`Fill::keep_min`]
//! (some of the attack's body sounds twice, never its transient:
//! [`Filled::replay_from_attack`]); whatever is still left is stretched
//! at whatever rate fills it, below [`Fill::least_rate_leftover`] only if
//! it must ([`Filled::forced_rate`]). [`Filled::left`] is 0.

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

/// One take's stretch of the recording (the comp as recorded: recording
/// time, seconds): a fill never reaches across its ends, where another
/// take plays.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Take {
    pub from: f64,
    pub to: f64,
}

/// How a gap is filled.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Fill {
    /// The slowest a residual stretch may play (0.8: the owner's rule; a
    /// slice at 0.11× was heard). Past it what is left stays a gap.
    pub least_rate: f64,
    /// The crossfade at a seam, seconds: the two sides sound together this
    /// long (less where they overlap less).
    pub crossfade: f64,
    /// The longest crossfade of two decays where they overlap, seconds.
    pub most_crossfade: f64,
    /// How much of a piece's start (its attack) is never stretched,
    /// seconds; nor, past [`Cut::guard`], played again by the piece after
    /// replaying the previous stroke's decay.
    pub keep: f64,
    /// The slowest a stretch of what is still left after the replay may
    /// play (0.5: the owner's floor), when [`Fill::least_rate`] does not
    /// reach (0.3.1; a 0.3.0 request reads as 0.5). Since 0.3.2 the
    /// preferred floor: below it a stretch still fills, flagged
    /// [`Filled::forced_rate`].
    pub least_rate_leftover: f64,
    /// The nearest the piece after may start past the previous stroke's
    /// attack when it must replay that stroke's decay closer than
    /// [`Fill::keep`], seconds: the attack's body may sound twice, never
    /// its transient (0.3.2; default 0.010). Also the nearest past its own
    /// attack a stretch of the piece before may start when its decay past
    /// `keep` is too short.
    pub keep_min: f64,
}

impl Default for Fill {
    fn default() -> Self {
        Self {
            least_rate: 0.8,
            crossfade: 0.004,
            most_crossfade: 0.010,
            keep: 0.030,
            least_rate_leftover: 0.5,
            keep_min: 0.010,
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
    /// Where the draw starts, timeline seconds: the piece plays at its
    /// own speed up to here, and only `drawn_from..end` of it is drawn
    /// over `drawn_from..drawn_to`. None: drawn from just past its attack
    /// (0.2's whole decay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn_from: Option<f64>,
    /// How long it sounds together with the piece before, crossfaded,
    /// seconds: it starts sounding at `start`, the piece before stops at
    /// `start + crossfade`. `Some(0.0)`: a dip between the two (what no
    /// fill covers), each side fading out or in within itself, never a
    /// hard cut. None: the host's own seam fade, reaching back before
    /// `start` (0.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crossfade: Option<f64>,
}

impl Piece {
    /// Where it stops sounding: drawn out, or its natural end.
    #[must_use]
    pub fn reach(&self) -> f64 {
        self.drawn_to.unwrap_or(self.end)
    }
}

/// What was done at one gap, for the host to say.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Filled {
    /// The hit after the gap: where it plays (timeline seconds) and where
    /// it was recorded.
    pub at: f64,
    pub recorded: f64,
    /// The gap before it was filled: from where the piece before would
    /// have been cut (just before this hit's old place) to its lead,
    /// seconds.
    pub gap: f64,
    /// How far the piece before plays on past that, at its own speed. The
    /// old cut is just before this hit's attack, the next in the piece
    /// before's recording when the two were played one after the other,
    /// and the piece before never plays an attack again: so this is 0
    /// unless the hit after was played out of order (the piece before's
    /// next attack lies further on).
    pub extended: f64,
    /// How far the piece after reaches back before its lead, into its own
    /// take (the replay included).
    pub reached_back: f64,
    /// Of that, how much is past [`Cut::guard`]: the previous stroke's
    /// decay played again, never its first [`Fill::keep`] (0.3.1).
    #[serde(default)]
    pub replayed_decay: f64,
    /// Where the piece after starts, past the previous stroke's attack in
    /// its recording, seconds, when it replayed that stroke's decay
    /// (0.3.2): at least [`Fill::keep`] if it could, never under
    /// [`Fill::keep_min`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_from_attack: Option<f64>,
    /// How long the two sound together, crossfaded.
    pub crossfade: f64,
    /// What was still left and stretched: how long the stretched span
    /// plays and its rate (below 1: slower; below [`Fill::least_rate`]
    /// only for what the replay left, never below
    /// [`Fill::least_rate_leftover`]). None: no stretch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretched: Option<Stretched>,
    /// What nothing covers: the dip between the two sides' audio, seconds
    /// (the fades within each side not counted). 0 since 0.3.2: an edit
    /// never leaves silence.
    pub left: f64,
    /// The stretch had to play slower than [`Fill::least_rate_leftover`]
    /// to fill the gap (0.3.2): its rate is `stretched.rate`.
    #[serde(default)]
    pub forced_rate: bool,
    /// Whether the fill met a take switch, and what it did there.
    #[serde(default)]
    pub take_switch: TakeSwitch,
}

/// A fill at a take switch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TakeSwitch {
    /// None met.
    #[default]
    None,
    /// A side stopped at it: the other take's audio was not played.
    Stopped,
    /// A side played on through the comp's own switch there (as the comp
    /// plays it, never past an attack), which spared a stretch.
    Crossed,
}

/// A residual stretch.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stretched {
    /// Where it starts, timeline seconds, and how long it plays.
    pub from: f64,
    pub over: f64,
    pub rate: f64,
}
