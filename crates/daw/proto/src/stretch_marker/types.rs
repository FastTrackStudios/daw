//! Stretch marker data types.

use facet::Facet;

/// One stretch marker on a take.
///
/// A pair of positions: where a point in the source is *heard*, and
/// where it *is*. Everything between two markers is stretched to fit,
/// which is what makes this non-destructive — the media is untouched
/// and the timing lives in the project.
///
/// ## The two coordinate systems
///
/// This trips everyone, so it is stated plainly:
///
/// - [`position`](Self::position) is in **take playback time**:
///   seconds from the item's start, multiplied by the take's play
///   rate. It is *not* project time and *not* seconds into the file.
/// - [`source_position`](Self::source_position) is in **source time**:
///   seconds into the media, including the take's start offset.
///
/// A project position converts as
/// `(project_pos - item_pos) * play_rate` for the first, and
/// `start_offset + (project_pos - item_pos) * play_rate` for the
/// second. A caller with only project times wants
/// [`StretchMarker::at_project_time`].
#[derive(Clone, Copy, Debug, PartialEq, Facet)]
pub struct StretchMarker {
    /// Position in take playback time.
    pub position: f64,
    /// Corresponding position in the source media.
    pub source_position: f64,
    /// Curvature of the stretch leading into the next marker,
    /// `-1.0..=1.0`. Zero is linear, which is what an alignment or a
    /// timing drag wants; a non-zero slope eases the rate change and
    /// is what a hand-drawn tape effect uses.
    pub slope: f64,
}

impl StretchMarker {
    /// A linear marker.
    pub fn new(position: f64, source_position: f64) -> Self {
        Self {
            position,
            source_position,
            slope: 0.0,
        }
    }

    /// Build a marker for a point at `project_pos` that should play the
    /// source content originally at `source_project_pos`.
    ///
    /// The conversion both callers get wrong on their own — see the
    /// note on the two coordinate systems above.
    pub fn at_project_time(
        project_pos: f64,
        source_project_pos: f64,
        item_position: f64,
        start_offset: f64,
        play_rate: f64,
    ) -> Self {
        let rate = if play_rate > 0.0 { play_rate } else { 1.0 };
        Self {
            position: (project_pos - item_position) * rate,
            source_position: start_offset + (source_project_pos - item_position) * rate,
            slope: 0.0,
        }
    }

    /// How much the material around this marker is stretched, given the
    /// next marker along. Above 1 is slower, below 1 is faster.
    ///
    /// `None` when the two markers share a position, which carries no
    /// rate.
    pub fn rate_to(&self, next: &StretchMarker) -> Option<f64> {
        let played = next.position - self.position;
        let source = next.source_position - self.source_position;
        (source.abs() > f64::EPSILON).then(|| played / source)
    }
}

/// How the host resamples between markers.
///
/// REAPER's per-take stretch mode. The default follows the project
/// preference; the rest are worth naming because the right choice is
/// material-dependent and a vocal is not a drum loop.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub enum StretchMode {
    /// Whatever the project is set to.
    #[default]
    ProjectDefault = 0,
    /// Even-handed; the safe choice when the material is mixed.
    Balanced = 1,
    /// Preserves pitch and formants best on sustained, pitched
    /// material — a sung vocal, a held string.
    Tonal = 2,
    /// Preserves attacks; for drums and anything percussive, where a
    /// smeared transient is the artefact that shows.
    Transient = 4,
    /// Avoids pre-echo before transients, at some cost elsewhere.
    NoPreEcho = 5,
}
