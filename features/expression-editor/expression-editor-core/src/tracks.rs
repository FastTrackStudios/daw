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
            color: None,
            reference: false,
            ref_color: RefColor::default(),
        }
    }
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
