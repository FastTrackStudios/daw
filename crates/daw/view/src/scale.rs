//! The level scale every view draws a fader and a meter to: one definition,
//! so a cap, its groove, a meter's bars and the numbers beside them cannot
//! disagree, and an engine's gain and a view's fader speak the same dB.

/// The top of the fader's travel, in dB.
///
/// Measured, not assumed. The dB labels down REAPER's mixer fader
/// were read off a screenshot at x 9..24 and fitted: the mapping is
/// LINEAR IN dB at 2.204 px/dB, and extrapolating it to 0 dB lands
/// at y 124.88 against a groove that starts at 125. Every residual
/// is under a pixel across the whole travel.
///
/// That fit describes REAPER's own fader, which tops out at unity:
/// there is no boost on it. Ours goes to +12, deliberately — a
/// fader that cannot add gain makes you reach for a plugin to do
/// the most ordinary thing in a mix, and the measurement is kept
/// above because it is what the SCALE's spacing is derived from,
/// not a limit we are bound by.
pub const FADER_TOP_DB: f64 = 12.0;

/// And the bottom of the travel.
///
/// The same fit puts the groove's last pixel at −55.86 dB. Rounded
/// to −56 rather than to a tidier −60, because −60 would put every
/// label seven pixels off the ones REAPER draws — a tidy constant
/// that is visibly wrong is worse than an untidy one that is right.
pub const FADER_BOTTOM_DB: f64 = -56.0;

/// Where a level sits on the fader, 0 at the bottom and 1 at the top.
///
/// The one definition of the fader's scale: the cap, the lit groove
/// and the dB labels beside it all come through here, so a label
/// cannot drift from the position it labels.
#[must_use]
pub fn fader_norm(db: f64) -> f64 {
    let span = FADER_TOP_DB - FADER_BOTTOM_DB;
    ((db - FADER_BOTTOM_DB) / span).clamp(0.0, 1.0)
}

/// The level at a position on the fader. The inverse of
/// [`fader_norm`], for hit testing and for dragging.
#[must_use]
pub fn fader_db(norm: f64) -> f64 {
    FADER_BOTTOM_DB + norm.clamp(0.0, 1.0) * (FADER_TOP_DB - FADER_BOTTOM_DB)
}

/// A linear gain as a position on the fader.
///
/// `Track::volume` is a gain, not a dB value — 1.0 is unity — so
/// this is the conversion the strip needs. Silence is a gain of
/// zero, whose logarithm is not a number, so it is answered before
/// the log rather than after it.
#[must_use]
pub fn gain_norm(gain: f64) -> f64 {
    if gain <= 0.0 {
        return 0.0;
    }
    fader_norm(20.0 * gain.log10())
}

/// The labels REAPER prints down its fader.
///
/// Not a round series: REAPER steps by 12, starting at −6. Twelve dB
/// is a doubling and a halving twice over, which is the interval a
/// mixing decision is actually made in.
/// The ceiling itself is NOT among them. A mark at the very top of
/// a column sits half a glyph off the end of it and labels the one
/// position you can find without a number — the fader will not go
/// further, which the fader already tells you by stopping. +6 is
/// the last mark; the travel still runs to [`FADER_TOP_DB`].
pub const FADER_MARKS: [f64; 7] = [6.0, 0.0, -6.0, -18.0, -30.0, -42.0, -54.0];

/// The meter's ceiling and floor, which are the fader's.
///
/// One scale for the whole column, which is what lets one set of
/// numbers label both the bar and the cap beside it. It also gives
/// the meter what it most needs: room above 0 dBFS. A meter whose
/// ceiling is unity pins there, so a clean −1 and a signal 5 dB
/// into the red draw the identical full bar — the one distinction a
/// meter exists to make.
pub const METER_TOP_DB: f64 = FADER_TOP_DB;
pub const METER_BOTTOM_DB: f64 = FADER_BOTTOM_DB;

/// Where a level sits in the column, 0 at the floor and 1 at the
/// top.
///
/// The one scale the column is drawn to: the lit bars, the hold
/// lines, the numbers beside them and the cap's own travel all come
/// through here, so nothing in the column can disagree with the
/// numbers labelling it.
#[must_use]
pub fn meter_norm(db: f64) -> f64 {
    fader_norm(db)
}

/// The decibels the meter's zones change colour at.
///
/// Landmarks rather than a smooth ramp: −18 is where a track is
/// sitting where it should, −6 is getting hot, 0 is over. A
/// continuous green-to-red gradient makes every level a position to
/// measure; these make the common ones a colour to recognise.
pub const METER_ZONES: [f64; 3] = [-18.0, -6.0, 0.0];
