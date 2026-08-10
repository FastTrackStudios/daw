//! Multitrack: several documents behind one editing surface.
//!
//! Vovious calls this the TrackSwitcher, and its two load-bearing rules
//! are why this is a layer rather than "just open another editor":
//!
//! - **One undo history per track.** Switching tracks must never let an
//!   undo reach across into another track's edits. That is a property of
//!   where the history *lives*, so it cannot be bolted on afterwards —
//!   hence this landing before the audio surface rather than after.
//! - **Reference tracks.** Any subset of the inactive tracks can be
//!   drawn behind the active one, read-only, so a harmony part can be
//!   tuned against the lead without leaving the window.
//!
//! The camera, tools, grid, tuning and mouse map are deliberately *not*
//! per-track: switching tracks changes what you are editing, not where
//! you are looking. Only the document and its history swap.

use crate::Mode;
use crate::doc::ExpressionDoc;
use crate::edit::History;

/// Undo depth per track.
pub const HISTORY_LIMIT: usize = 10;

/// How a reference track is tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RefColor {
    /// The surface's own reference colour.
    #[default]
    Default,
    /// The colour the host gave the track.
    Host,
    /// Outline only — least visual noise, for when several references
    /// are shown at once.
    Shadow,
}

/// One track.
///
/// A track holds no camera: the view is shared across the whole
/// workspace, so switching does not move you.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub name: String,
    /// Stale while this track is the active one — the live document
    /// lives in `Editor::doc` and is written back on switch. Reads go
    /// through [`Workspace::doc_of`], which refuses the active slot for
    /// exactly this reason.
    doc: ExpressionDoc,
    history: History,
    /// How this track is edited and drawn.
    ///
    /// On the *track*, not on the editor, because a workspace holding a
    /// vocal, its reference MIDI, a guitar and a kit needs all four
    /// surfaces at once — which is the whole point of showing them
    /// stacked. The editor's mode is a view of the active track's.
    ///
    /// Inferred when the track is loaded and the user's from then on: no
    /// threshold gets a whispered vocal and a melodic tom fill both
    /// right, so a wrong guess has to be one click to correct.
    pub mode: Mode,
    /// Height weight in a stacked view, relative to the other tracks.
    ///
    /// Defaults to what the mode needs — a slice strip wants three
    /// bands, a vocal two octaves, a string roll six rows — because
    /// splitting the height evenly gives the guitar the same room as
    /// the kit and neither ends up readable.
    pub weight: f32,
    /// Hidden tracks stay in the workspace and keep their history; they
    /// are simply not drawn. Distinct from removing.
    pub hidden: bool,
    /// Colour the host assigned, for [`RefColor::Host`].
    pub color: Option<String>,
    /// Drawn behind the active track, read-only.
    pub reference: bool,
    pub ref_color: RefColor,
}

impl Track {
    pub fn new(name: impl Into<String>, doc: ExpressionDoc) -> Self {
        Self {
            name: name.into(),
            doc,
            history: History::new(HISTORY_LIMIT),
            mode: Mode::default(),
            weight: Mode::default().stack_weight(),
            hidden: false,
            color: None,
            reference: false,
            ref_color: RefColor::default(),
        }
    }

    /// The same, in a known mode — and taking the mode's natural height
    /// with it, which is the pairing a caller almost always wants.
    pub fn in_mode(name: impl Into<String>, doc: ExpressionDoc, mode: Mode) -> Self {
        let mut t = Self::new(name, doc);
        t.set_mode(mode);
        t
    }

    /// Change mode, and the height with it.
    ///
    /// Only while the weight is still the mode's default: once a user
    /// has sized a dimension by hand, switching modes must not silently undo
    /// that.
    pub fn set_mode(&mut self, mode: Mode) {
        if (self.weight - self.mode.stack_weight()).abs() < f32::EPSILON {
            self.weight = mode.stack_weight();
        }
        self.mode = mode;
    }
}

/// One track's slot in a stacked layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackRow {
    /// Index into [`Workspace::tracks`].
    pub track: usize,
    /// Top edge, in the same units as the height passed to
    /// [`Workspace::stack`].
    pub y: f32,
    pub height: f32,
}

/// The set of tracks behind one editor.
///
/// No `Default`: a workspace cannot exist without a document, and a
/// document cannot exist without a time base. Inventing one would mean
/// guessing whether the surface is in ticks or analysis frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    tracks: Vec<Track>,
    active: usize,
}

impl Workspace {
    /// A workspace around the document the editor was opened on.
    ///
    /// The slot's copy of `doc` is stale from this moment on — the live
    /// one lives in `Editor::doc`. See [`Workspace::doc_of`].
    pub fn single(name: impl Into<String>, doc: ExpressionDoc) -> Self {
        Self {
            tracks: vec![Track::new(name, doc)],
            active: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Never empty: an editor always has a track, even a blank one.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn track(&self, i: usize) -> Option<&Track> {
        self.tracks.get(i)
    }

    pub fn track_mut(&mut self, i: usize) -> Option<&mut Track> {
        self.tracks.get_mut(i)
    }

    pub fn names(&self) -> Vec<&str> {
        self.tracks.iter().map(|t| t.name.as_str()).collect()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.name == name)
    }

    /// A parked track's document.
    ///
    /// Returns `None` for the active slot, whose copy here is stale —
    /// the live one is in `Editor::doc`. Making that a `None` rather
    /// than silently handing back the stale copy is what stops a
    /// reference overlay from drawing a track's *previous* state over
    /// the notes you are editing.
    pub fn doc_of(&self, i: usize) -> Option<&ExpressionDoc> {
        if i == self.active {
            return None;
        }
        self.tracks.get(i).map(|t| &t.doc)
    }

    /// Tracks that appear in a stacked view, in order, with indices.
    ///
    /// Unlike [`Workspace::references`] this *includes* the active
    /// track: a stack shows everything, and the active one is simply
    /// the row you are editing.
    pub fn visible(&self) -> impl Iterator<Item = (usize, &Track)> {
        self.tracks.iter().enumerate().filter(|(_, t)| !t.hidden)
    }

    /// The active track's mode.
    pub fn mode(&self) -> Mode {
        self.tracks
            .get(self.active)
            .map(|t| t.mode)
            .unwrap_or_default()
    }

    /// Lay the visible tracks out over `height`, in order.
    ///
    /// Proportional to each track's weight, with a floor: a dimension
    /// squeezed to nothing is a dimension that cannot be clicked to select,
    /// so a user cannot get out of the state they fell into.
    ///
    /// `active_boost` gives the row being edited extra weight — you are
    /// working in it and it should have the room. `1.0` divides purely
    /// by weight.
    pub fn stack(&self, height: f32, active_boost: f32, min_row: f32) -> Vec<StackRow> {
        let rows: Vec<(usize, f32)> = self
            .visible()
            .map(|(i, t)| {
                let boost = if i == self.active { active_boost } else { 1.0 };
                (i, (t.weight.max(0.01)) * boost.max(0.01))
            })
            .collect();
        if rows.is_empty() || height <= 0.0 {
            return Vec::new();
        }

        // The floor can want more than there is. Rather than overflow
        // the viewport, fall back to an even split — at that point the
        // honest answer is that nothing fits well and every dimension should
        // at least be equally bad.
        let min_row = min_row.max(0.0);
        let n = rows.len() as f32;
        if min_row * n >= height {
            let each = height / n;
            let mut y = 0.0;
            return rows
                .iter()
                .map(|&(track, _)| {
                    let row = StackRow {
                        track,
                        y,
                        height: each,
                    };
                    y += each;
                    row
                })
                .collect();
        }

        // Hand every dimension its floor first, then share what is left by
        // weight. Sharing the whole height by weight and clamping
        // afterwards would overflow by however much the clamping added.
        let spare = height - min_row * n;
        let total: f32 = rows.iter().map(|(_, w)| w).sum();
        let mut y = 0.0;
        rows.iter()
            .map(|&(track, w)| {
                let h = min_row + spare * (w / total);
                let row = StackRow {
                    track,
                    y,
                    height: h,
                };
                y += h;
                row
            })
            .collect()
    }

    /// Which stacked row a y coordinate falls in.
    pub fn row_at(rows: &[StackRow], y: f32) -> Option<usize> {
        rows.iter()
            .find(|r| y >= r.y && y < r.y + r.height)
            .map(|r| r.track)
    }

    /// Parked tracks marked as references, with their indices.
    pub fn references(&self) -> impl Iterator<Item = (usize, &Track)> {
        self.tracks
            .iter()
            .enumerate()
            .filter(move |(i, t)| *i != self.active && t.reference)
    }

    /// Add a track and return its index.
    pub fn push(&mut self, track: Track) -> usize {
        self.tracks.push(track);
        self.tracks.len() - 1
    }

    /// Remove a track. The active one cannot be removed — closing what
    /// you are editing is a host decision, not an editor one.
    pub fn remove(&mut self, i: usize) -> bool {
        if i == self.active || i >= self.tracks.len() {
            return false;
        }
        self.tracks.remove(i);
        if i < self.active {
            self.active -= 1;
        }
        true
    }

    /// Park the live document and history into the active slot, then
    /// take the target's. Returns what the editor should load.
    ///
    /// Both halves happen here so there is no window in which two slots
    /// believe they own the same history.
    pub(crate) fn swap_active(
        &mut self,
        to: usize,
        doc: ExpressionDoc,
        history: History,
    ) -> Option<(ExpressionDoc, History)> {
        if to == self.active || to >= self.tracks.len() {
            return None;
        }
        let cur = &mut self.tracks[self.active];
        cur.doc = doc;
        cur.history = history;

        self.active = to;
        let next = &mut self.tracks[to];
        // The document is cloned rather than moved out: there is no
        // sensible empty `ExpressionDoc` to leave behind, and a track
        // switch is a keystroke, not a hot path. The copy left here is
        // stale, which is precisely what `doc_of` refuses to hand out.
        Some((
            next.doc.clone(),
            core::mem::replace(&mut next.history, History::new(HISTORY_LIMIT)),
        ))
    }

    /// Rename without disturbing anything else.
    pub fn rename(&mut self, i: usize, name: impl Into<String>) -> bool {
        match self.tracks.get_mut(i) {
            Some(t) => {
                t.name = name.into();
                true
            }
            None => false,
        }
    }
}
