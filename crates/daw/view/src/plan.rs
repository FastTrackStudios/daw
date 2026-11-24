//! Which track each row of a panel shows.
//!
//! What a panel shows and how large is the app's to plan (Studio lays a
//! session out by a template's scenes); a view is handed the planned rows
//! as `(Track, depth)` pairs and reads them through [`Rows`], which maps a
//! panel row to the live track it shows.

use daw_proto::Track;

/// Which of the window's live tracks each panel row is showing.
///
/// A preset can hide a track, so the mixer's third strip is not
/// necessarily the session's third track — and the live values (mute,
/// fader, meter) are kept in the SESSION's order, because that is the
/// order the engine's events and its meter frames are indexed by.
///
/// This is the one place those two orders meet. Without it the overlay
/// reads a neighbour's fader whenever a preset hides anything, which is
/// a bug that looks like the mixer being subtly, consistently wrong
/// rather than like anything crashing.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    /// `rows[panel] = index into the window's tracks`.
    rows: Vec<usize>,
}

impl Rows {
    /// Build the map by matching GUIDs.
    ///
    /// GUIDs rather than positions, because that is the only identity a
    /// track has that survives being filtered, reordered or renamed —
    /// and a map built from positions would be the very assumption this
    /// type exists to remove.
    #[must_use]
    pub fn of(panel: &[(Track, u32)], tracks: &[Track]) -> Self {
        let mut index: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::with_capacity(tracks.len());
        for (i, track) in tracks.iter().enumerate() {
            index.entry(track.guid.as_str()).or_insert(i);
        }
        Self {
            rows: panel
                .iter()
                .map(|(track, _)| {
                    index
                        .get(track.guid.as_str())
                        .copied()
                        .unwrap_or(usize::MAX)
                })
                .collect(),
        }
    }

    /// The live track a panel row is showing.
    ///
    /// `None` for a row whose track has gone — which happens for one
    /// frame between a project reload and the re-record that follows
    /// it, and is a row to skip rather than a reason to panic.
    #[must_use]
    pub fn live<'a>(&self, tracks: &'a [Track], row: usize) -> Option<&'a Track> {
        tracks.get(*self.rows.get(row)?)
    }

    /// Whether any row's track is selected — what decides whether the
    /// unselected ones are dimmed at all.
    #[must_use]
    pub fn any_selected(&self, tracks: &[Track]) -> bool {
        self.rows
            .iter()
            .filter_map(|i| tracks.get(*i))
            .any(|t| t.selected)
    }

    /// The same, as an index — for the paths that need to write.
    #[must_use]
    pub fn index(&self, row: usize) -> Option<usize> {
        self.rows.get(row).copied().filter(|i| *i != usize::MAX)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}
