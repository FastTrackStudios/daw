//! The view settings the right rail switches.

/// How a focused track is sized, and what it costs its neighbours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Whether clicking a track opens it wide enough to edit a
    /// plugin's parameters on.
    pub focus_selected: bool,
    /// Whether a focused track takes its width FROM the other tracks or
    /// adds to the mixer's total.
    ///
    /// On, the mixer stays exactly as wide as it was and everything
    /// else gives up a few pixels — nothing moves off the screen you
    /// were looking at. Off, the other tracks keep their width and are
    /// simply pushed along, which is what you want when their widths
    /// are the thing you are comparing and must not change under you.
    pub take_focus_width: bool,
    /// How much of a 16:9 screen one focused track takes — this
    /// window's override of the app's focus fraction ([`FOCUS_FRACTION`]).
    pub focus_fraction: f64,
    /// Whether folding a phase's container folds it on every track.
    ///
    /// On, the chain stays in register across the mixer: fold Rescue
    /// away and every strip's Tone panels sit at the same height, which
    /// is what makes a row of racks comparable at all. Off, each track
    /// folds on its own — for when you are working one track rather
    /// than comparing them, and the other strips' chains are just in
    /// the way.
    pub fold_phases_together: bool,
    /// Whether a folder the view has shut shows its children's takes on
    /// its own row.
    ///
    /// On, a kit reads as one row per drum: the kick's three mics are
    /// one item in the kick's colour, under the kick's fader, and the
    /// mics are a fold away when you want them. That is what an
    /// arrangement is for — the routing that makes a drum a folder is a
    /// mix concern, and mixing happens on the strip.
    ///
    /// Off, a shut folder is empty, which is what REAPER draws. Worth
    /// keeping as a switch rather than a decision: the fold is a claim
    /// that the children are one performance, and on a folder where
    /// they are not — a bus with a bass and a vocal under it — the row
    /// says something true but useless, and you want to see the tree
    /// instead.
    pub folded_takes: bool,
    /// Compact mixer strips: no input section, the coloured band only as
    /// tall as the pan knob and the record arm, and the height they gave
    /// up handed to the fader. The FX row stays. See `strip::shape`.
    ///
    /// Live mode's strips first, and now every mode's: the input section
    /// was a tall empty band over every fader for a record-input picker
    /// set once per session. Off is REAPER's full collapse layout.
    pub live_strips: bool,
    /// Touchscreen strips: mute and solo grown to fill their column
    /// (`strip::Strip::touched`).
    pub touch_strips: bool,
    /// The mode the strips are laid out for, where a theme asks: a strip
    /// in Mix mode has no use for an arm, one in Record mode has the most
    /// for its routing (`skin`'s `ui.mixing` / `ui.recording`).
    pub mixing: bool,
    pub recording: bool,
}

/// How much of a 16:9 screen one focused track takes, by default (an
/// app's layout tables say the same; Studio's template holds it to this).
pub const FOCUS_FRACTION: f64 = 0.25;
/// What comes off the window's width before a focused track's share is
/// taken: the rails.
pub const FOCUS_GUTTER: f64 = 44.0;
/// A focused track is never narrower than this.
pub const FOCUS_LEGIBLE: f64 = 96.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            focus_selected: true,
            take_focus_width: true,
            focus_fraction: FOCUS_FRACTION,
            fold_phases_together: true,
            folded_takes: true,
            live_strips: true,
            touch_strips: false,
            mixing: false,
            recording: false,
        }
    }
}

impl Settings {
    /// How wide a focused track opens, for a panel `height` tall.
    ///
    /// A share of what a 16:9 screen of this height would leave between
    /// the rails (618 at 1440p): from the HEIGHT rather than the width, so
    /// a wider aspect gets more tracks instead of wider ones.
    #[must_use]
    pub fn focus_width(self, height: f64) -> f64 {
        let sixteen_by_nine = height * 16.0 / 9.0;
        let panel = FOCUS_GUTTER.mul_add(-2.0, sixteen_by_nine).max(0.0);
        (panel * self.focus_fraction).max(FOCUS_LEGIBLE)
    }
}
