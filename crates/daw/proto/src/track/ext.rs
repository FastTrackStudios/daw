//! Generic convenience helpers layered over the raw `Tracks` / `Items` /
//! `Projects` service traits — selection scoping, name lookup, moving items
//! between tracks, and `ProjectContext::Current`-defaulted mutators. None
//! of this is REAPER-specific or tied to any particular domain (session's
//! Track Manager, dynamic-template, anything else driving tracks); it's
//! plumbing every track-editing feature needs, so it lives once here
//! rather than being reinvented per feature.
//!
//! Undo-block wrapping is deliberately not here: `#[action(undo)]` gets it
//! from the action backend (see `daw-reaper`'s `ActionBackend` impl), and
//! anything outside an action can call `Projects::begin_undo_block` /
//! `end_undo_block` directly.
//!
//! Blanket-impl'd for any backend that already speaks the three traits —
//! `daw::reaper::Reaper` and `daw_standalone::sync::Standalone` get it for
//! free, same as `Tracks`/`Items`/`Projects` themselves.

use crate::DawError;
use crate::item::{ItemRef, Items};
use crate::project::{ProjectContext, Projects};
use crate::track::{Track, TrackRef, Tracks};

/// A subtree to create: one track plus, recursively, its children.
///
/// The nested counterpart to the flat [`TrackNode`](super::TrackNode) /
/// [`FolderDepthChange`](super::FolderDepthChange) representation a DAW
/// actually stores — [`TracksExt::append_shape`] flattens one into the
/// other on the way to the backend, and
/// [`TrackTree::shape_of_children`] reads an existing subtree back out as
/// one (so "give the new channel the same mics the old one has" is a
/// read-then-append, not a hand-rolled recursion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackShape {
    pub name: String,
    pub children: Vec<TrackShape>,
}

impl TrackShape {
    pub fn leaf(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            children: Vec::new(),
        }
    }

    pub fn with_children(name: impl Into<String>, children: Vec<TrackShape>) -> Self {
        Self {
            name: name.into(),
            children,
        }
    }

    /// Flatten into `(name, folder_depth)` pairs in mixer order, with
    /// `folder_depth` in the DAW's relative-depth encoding (`1` opens a
    /// folder, `0` plain, negative closes that many levels). The final
    /// entry closes one extra level so the whole run sits inside its
    /// parent.
    pub fn flatten(shape: &[TrackShape]) -> Vec<(String, i32)> {
        fn walk(shape: &[TrackShape], out: &mut Vec<(String, i32)>) {
            for node in shape {
                out.push((
                    node.name.clone(),
                    if node.children.is_empty() { 0 } else { 1 },
                ));
                if !node.children.is_empty() {
                    walk(&node.children, out);
                    if let Some(last) = out.last_mut() {
                        last.1 -= 1;
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(shape, &mut out);
        if let Some(last) = out.last_mut() {
            last.1 -= 1;
        }
        out
    }
}

/// An immutable snapshot of a project's whole track list, for navigating
/// the folder tree without re-querying the backend per lookup.
///
/// The per-call helpers on [`TracksExt`] (`children_of`, `get_track`,
/// `subtree_end_index`) each re-fetch the entire track list, so walking a
/// tree with them is quadratic and — over a real RPC transport — a
/// round-trip per node. Take one `TrackTree` and navigate it in memory
/// instead.
///
/// Being a snapshot is the point *and* the caveat: it reflects the project
/// as of when it was taken, so re-take it after any mutation rather than
/// reusing a stale one to compute indices.
#[derive(Debug, Clone)]
pub struct TrackTree {
    /// Sorted by `index` (mixer order), so positional walks are just
    /// slice order.
    tracks: Vec<Track>,
}

impl TrackTree {
    pub fn new(mut tracks: Vec<Track>) -> Self {
        tracks.sort_by_key(|track| track.index);
        Self { tracks }
    }

    /// Every track, in mixer order.
    pub fn all(&self) -> &[Track] {
        &self.tracks
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn get(&self, guid: &str) -> Option<&Track> {
        self.tracks.iter().find(|track| track.guid == guid)
    }

    pub fn at_index(&self, index: u32) -> Option<&Track> {
        self.tracks.iter().find(|track| track.index == index)
    }

    /// Direct children of `guid`, in mixer order.
    pub fn children_of<'a>(&'a self, guid: &'a str) -> impl Iterator<Item = &'a Track> + 'a {
        self.tracks
            .iter()
            .filter(move |track| track.parent_guid.as_deref() == Some(guid))
    }

    pub fn parent_of(&self, track: &Track) -> Option<&Track> {
        self.get(track.parent_guid.as_deref()?)
    }

    /// The subtree under `guid` as nested [`TrackShape`]s — read entirely
    /// from this snapshot, no per-node backend query. Pair with
    /// [`TracksExt::append_shape`] to clone an existing subtree's shape
    /// onto a new sibling.
    pub fn shape_of_children(&self, guid: &str) -> Vec<TrackShape> {
        self.children_of(guid)
            .map(|track| {
                TrackShape::with_children(track.name.clone(), self.shape_of_children(&track.guid))
            })
            .collect()
    }

    /// The index just past the end of `guid`'s subtree — where a new last
    /// child should be inserted, or `guid`'s own next-sibling position if
    /// it has no children yet.
    pub fn subtree_end_index(&self, guid: &str) -> Option<u32> {
        let parent = self.get(guid)?;
        let mut depth = 0;
        for track in self.tracks.iter().filter(|t| t.index >= parent.index) {
            depth += track.folder_depth;
            if track.index > parent.index && depth <= 0 {
                return Some(track.index + 1);
            }
        }
        Some(parent.index + 1)
    }
}

pub trait TracksExt: Tracks + Items + Projects {
    // ── Snapshot ────────────────────────────────────────────────────

    /// One fetch of the whole track list, for tree navigation without a
    /// query per node. See [`TrackTree`].
    fn track_tree(&self) -> TrackTree {
        TrackTree::new(self.all(ProjectContext::Current))
    }

    // ── Selection ───────────────────────────────────────────────────

    /// The single currently-selected track, or an error if nothing is
    /// selected. Most track-editing commands are scoped to "whatever's
    /// selected" — this is the first call almost all of them make.
    fn selected_scope(&self) -> Result<Track, DawError> {
        self.selected(ProjectContext::Current)
            .into_iter()
            .next()
            .ok_or_else(|| DawError::NotFound("no track is selected".into()))
    }

    /// Clear the current selection and select exactly `guid`. Use
    /// [`add_to_selection`](Self::add_to_selection) instead to extend an
    /// existing selection rather than replace it.
    fn select(&self, guid: &str) -> Result<(), DawError> {
        let project = ProjectContext::Current;
        self.clear_selection(project.clone())?;
        Tracks::set_selected(self, project, TrackRef::Guid(guid.to_string()), true)
    }

    /// Select `guid` in addition to whatever's already selected.
    fn add_to_selection(&self, guid: &str) -> Result<(), DawError> {
        Tracks::set_selected(
            self,
            ProjectContext::Current,
            TrackRef::Guid(guid.to_string()),
            true,
        )
    }

    // ── Lookup ──────────────────────────────────────────────────────

    /// One track by guid, or an "invalid object" error if it no longer
    /// exists (deleted between when a caller last fetched it and now).
    fn get_track(&self, guid: &str) -> Result<Track, DawError> {
        Tracks::get(
            self,
            ProjectContext::Current,
            TrackRef::Guid(guid.to_string()),
        )
        .ok_or_else(|| DawError::invalid_object("track", guid))
    }

    /// One track by mixer position.
    fn track_at_index(&self, index: u32) -> Option<Track> {
        Tracks::get(self, ProjectContext::Current, TrackRef::Index(index))
    }

    /// Find a track by exact name. Errors (rather than silently picking
    /// one) if the name is ambiguous — callers scope by selection first
    /// for anything that isn't a top-level, presumed-unique name.
    fn find_track(&self, name: &str) -> Result<Track, DawError> {
        let mut matches = self
            .all(ProjectContext::Current)
            .into_iter()
            .filter(|track| track.name == name);
        let found = matches
            .next()
            .ok_or_else(|| DawError::not_found("track", name))?;
        if matches.next().is_some() {
            return Err(DawError::operation_failed(format!(
                "multiple tracks named {name:?}; disambiguate by guid"
            )));
        }
        Ok(found)
    }

    /// Every track whose direct parent is `guid`, in mixer order. Fetches
    /// the whole track list — prefer [`track_tree`](Self::track_tree) when
    /// walking more than one level.
    fn children_of(&self, guid: &str) -> Vec<Track> {
        self.track_tree().children_of(guid).cloned().collect()
    }

    /// See [`TrackTree::subtree_end_index`]. Fetches the whole track list;
    /// prefer a [`TrackTree`] when you need more than one lookup.
    fn subtree_end_index(&self, guid: &str) -> Option<u32> {
        self.track_tree().subtree_end_index(guid)
    }

    // ── Mutation (all on `ProjectContext::Current`) ─────────────────

    /// Insert a new track and return its guid. With no other tracks or
    /// selection context to place it relative to, it lands at the top
    /// level, at the end of the track list.
    fn insert_track(&self, name: &str) -> Result<String, DawError> {
        self.add(ProjectContext::Current, name, None)
    }

    /// Insert a new track at a specific mixer position; returns its guid.
    fn insert_track_at(&self, name: &str, index: u32) -> Result<String, DawError> {
        self.add(ProjectContext::Current, name, Some(index))
    }

    /// Set a track's folder-depth change (`1` opens a folder, `0` is a
    /// plain track, negative closes that many levels).
    fn set_depth(&self, guid: &str, depth: i32) -> Result<(), DawError> {
        self.set_folder_depth(
            ProjectContext::Current,
            TrackRef::Guid(guid.to_string()),
            depth,
        )
    }

    /// Move every item on `from_guid` onto `to_guid`.
    fn move_items(&self, from_guid: &str, to_guid: &str) -> Result<(), DawError> {
        let project = ProjectContext::Current;
        let target = TrackRef::Guid(to_guid.to_string());
        for item in self.get_items(project.clone(), TrackRef::Guid(from_guid.to_string())) {
            self.move_to_track(project.clone(), ItemRef::Guid(item.guid), target.clone())?;
        }
        Ok(())
    }

    /// Create a single new track as the last child of `parent_guid`.
    fn append_child(&self, parent_guid: &str, name: &str) -> Result<(), DawError> {
        self.append_shape(parent_guid, &[TrackShape::leaf(name)])
    }

    /// Create `shape` (a nested subtree) as the last children of
    /// `parent_guid`, opening `parent_guid` as a folder if it isn't one
    /// already.
    fn append_shape(&self, parent_guid: &str, shape: &[TrackShape]) -> Result<(), DawError> {
        let insertion_index = self.prepare_append(parent_guid)?;
        self.set_depth(parent_guid, 1)?;
        self.insert_shape_at(shape, insertion_index)
    }

    /// Create `shape` starting at an explicit mixer position, without any
    /// parent bookkeeping. Prefer [`append_shape`](Self::append_shape) —
    /// this is the escape hatch for callers that already know exactly
    /// where the subtree goes (e.g. mid-restructure, when the tree is
    /// briefly not well-formed enough for a subtree-end walk).
    fn insert_shape_at(&self, shape: &[TrackShape], index: u32) -> Result<(), DawError> {
        for (offset, (name, depth)) in TrackShape::flatten(shape).into_iter().enumerate() {
            let track = self.insert_track_at(&name, index + offset as u32)?;
            self.set_depth(&track, depth)?;
        }
        Ok(())
    }

    /// The index to insert a new last child of `parent_guid` at, having
    /// first made room for it.
    ///
    /// Whatever track currently sits just before that index is the one
    /// terminating `parent_guid`'s subtree, so it closes one level too
    /// many once a new sibling follows it — its `folder_depth` is bumped
    /// by one and the newcomer takes over closing the folder. That
    /// terminator isn't necessarily a *direct* child: appending a third
    /// channel after `L/[mic]`, `R/[mic]` means fixing up `R`'s last mic,
    /// a grandchild closing two levels (`-2` → `-1`).
    ///
    /// The index is computed from a single snapshot taken *before* that
    /// fixup — it changes what a fresh subtree-end walk would see, so
    /// recomputing afterwards silently yields the wrong position.
    fn prepare_append(&self, parent_guid: &str) -> Result<u32, DawError> {
        let tree = self.track_tree();
        let parent = tree
            .get(parent_guid)
            .ok_or_else(|| DawError::invalid_object("track", parent_guid))?;
        let insertion_index = tree
            .subtree_end_index(parent_guid)
            .unwrap_or(parent.index + 1);

        if let Some(previous) = tree.at_index(insertion_index.saturating_sub(1))
            // `parent` itself when it has no children yet — nothing to fix.
            && previous.guid != parent_guid
            && previous.folder_depth < 0
        {
            self.set_depth(&previous.guid, previous.folder_depth + 1)?;
        }
        Ok(insertion_index)
    }
}

impl<D: Tracks + Items + Projects + ?Sized> TracksExt for D {}
