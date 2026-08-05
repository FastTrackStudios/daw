//! Generic convenience helpers layered over the raw `Tracks` / `Items` /
//! `Projects` service traits — selection scoping, name lookup, moving items
//! between tracks. None of this is REAPER-specific or tied to any
//! particular domain (session's Track Manager, dynamic-template, anything
//! else driving tracks); it's plumbing every track-editing feature needs,
//! so it lives once here rather than being reinvented per feature.
//! Undo-block wrapping is deliberately not here — it's a `Projects`
//! primitive (`begin_undo_block`/`end_undo_block`) callers already have
//! direct access to; open/close it inline around whatever needs it rather
//! than routing through another layer of abstraction.
//!
//! Blanket-impl'd for any backend that already speaks the three traits —
//! `daw::reaper::Reaper` and `daw_standalone::sync::Standalone` get it for
//! free, same as `Tracks`/`Items`/`Projects` themselves.

use crate::item::{ItemRef, Items};
use crate::project::{ProjectContext, Projects};
use crate::track::{Track, TrackRef, Tracks};
use crate::DawError;

pub trait TracksExt: Tracks + Items + Projects {
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

    /// Insert a new track and return its guid. With no other tracks or
    /// selection context to place it relative to, it lands at the top
    /// level, at the end of the track list.
    fn insert_track(&self, name: &str) -> Result<String, DawError> {
        self.add(ProjectContext::Current, name, None)
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

    /// Move every item on `from_guid` onto `to_guid`.
    fn move_items(&self, from_guid: &str, to_guid: &str) -> Result<(), DawError> {
        let project = ProjectContext::Current;
        let target = TrackRef::Guid(to_guid.to_string());
        for item in self.get_items(project.clone(), TrackRef::Guid(from_guid.to_string())) {
            self.move_to_track(project.clone(), ItemRef::Guid(item.guid), target.clone())?;
        }
        Ok(())
    }

    /// One track by guid, or an "invalid object" error if it no longer
    /// exists (deleted between when a caller last fetched it and now).
    fn get_track(&self, guid: &str) -> Result<Track, DawError> {
        Tracks::get(self, ProjectContext::Current, TrackRef::Guid(guid.to_string()))
            .ok_or_else(|| DawError::invalid_object("track", guid))
    }

    /// Every track whose direct parent is `guid`, in mixer order.
    fn children_of(&self, guid: &str) -> Vec<Track> {
        self.all(ProjectContext::Current)
            .into_iter()
            .filter(|track| track.parent_guid.as_deref() == Some(guid))
            .collect()
    }

    /// The index just past the end of `guid`'s subtree — where a new
    /// last child should be inserted, or `guid`'s own next-sibling
    /// position if it has no children yet.
    fn subtree_end_index(&self, guid: &str) -> Option<u32> {
        let all = self.all(ProjectContext::Current);
        let parent = all.iter().find(|track| track.guid == guid)?;
        let mut depth = 0;
        for track in all.iter().filter(|track| track.index >= parent.index) {
            depth += track.folder_depth;
            if track.index > parent.index && depth <= 0 {
                return Some(track.index + 1);
            }
        }
        Some(parent.index + 1)
    }
}

impl<D: Tracks + Items + Projects + ?Sized> TracksExt for D {}
