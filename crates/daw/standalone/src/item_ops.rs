//! Changes to a project's items as values, applied many at once.
//!
//! A caller works out what it wants — a comp's pieces, a hit placed again —
//! as a list of [`ItemOp`]s and hands the list over once:
//!
//! ```ignore
//! let piece = ItemId::new();
//! daw.apply_items(&song, &[
//!     ItemOp::Copy { item: source.clone(), into: piece.clone() },
//!     ItemOp::Lane { item: piece.clone(), lane: edit },
//!     ItemOp::Place { item: piece.clone(), at: 84.37, length: 0.25, offset: 12.5 },
//!     ItemOp::Fades { item: piece, fade_in: 0.005, fade_out: 0.005 },
//! ])?;
//! ```
//!
//! The engine takes its lock once for the whole list, writes each change
//! through its journal (and an open undo step), and says once per item what
//! changed. A setter per field took the lock, resolved the project, wrote
//! the journal and published an event for every field of every item — a
//! comp of a kit was thousands of each. The caller names a new item
//! (`ItemId::new()`), so a list never waits on the engine to learn it.

use daw_proto::id::{ItemId, ProjectId};
use daw_proto::item::ItemEvent;
use daw_proto::primitives::{Duration, PositionInSeconds};
use daw_proto::{DawError, DawResult, StretchMarker};

use crate::sync::{ProjectState, Standalone};

/// One change to an item.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemOp {
    /// `item` copied as `into`, on its track and lane, its takes with it.
    Copy { item: ItemId, into: ItemId },
    /// Where it plays, how long, and from how far into its active take's
    /// source (its take envelopes slipped with the audio).
    Place {
        item: ItemId,
        at: f64,
        length: f64,
        offset: f64,
    },
    /// The fixed lane it is on.
    Lane { item: ItemId, lane: u32 },
    /// Its fades' lengths, seconds (their shapes kept).
    Fades {
        item: ItemId,
        fade_in: f64,
        fade_out: f64,
    },
    /// Its active take played through these stretch markers (none: as
    /// recorded).
    Stretch {
        item: ItemId,
        markers: Vec<StretchMarker>,
    },
    /// Gone, with its takes.
    Delete { item: ItemId },
}

impl ItemOp {
    /// The item it changes.
    #[must_use]
    pub fn item(&self) -> &ItemId {
        match self {
            Self::Copy { into: item, .. }
            | Self::Place { item, .. }
            | Self::Lane { item, .. }
            | Self::Fades { item, .. }
            | Self::Stretch { item, .. }
            | Self::Delete { item } => item,
        }
    }
}

impl Standalone {
    /// Apply `ops` to project `song` in order, under one lock: all of them,
    /// or — at the first that cannot be (no such item, a lane the track
    /// does not have) — those before it, and the error.
    ///
    /// # Errors
    /// When the project is not open, or an op names an item it does not
    /// have.
    pub fn apply_items(&self, song: &ProjectId, ops: &[ItemOp]) -> DawResult<()> {
        let mut changed: Vec<Change> = Vec::with_capacity(ops.len());
        let applied = self.with_project_mut(song.as_str(), |p| {
            ops.iter()
                .try_for_each(|op| apply(p, op).map(|c| changed.push(c)))
        });
        // Said once per item, after the lock: what it became.
        self.announce(song, &changed);
        applied?
    }

    fn announce(&self, song: &ProjectId, changed: &[Change]) {
        let mut said = std::collections::HashSet::new();
        for change in changed.iter().rev() {
            if !said.insert(change.item()) {
                continue;
            }
            let project_guid = song.to_string();
            let event = match change {
                Change::Made { item, track } => {
                    let Some(item) = self.item_now(song, item) else {
                        continue;
                    };
                    let track_guid = track.clone();
                    ItemEvent::Created {
                        project_guid,
                        track_guid,
                        item,
                    }
                }
                Change::Changed(item) => ItemEvent::Changed {
                    project_guid,
                    item_guid: item.to_string(),
                },
                Change::Gone { item, track } => ItemEvent::Deleted {
                    project_guid,
                    track_guid: track.clone(),
                    item_guid: item.to_string(),
                },
            };
            crate::item::publish_item_event(self, event);
        }
    }

    fn item_now(&self, song: &ProjectId, item: &ItemId) -> Option<daw_proto::Item> {
        self.read_project(song.as_str(), |p| {
            p.items.get(item.as_str()).map(|e| e.item.clone())
        })
        .flatten()
    }
}

/// What one op did to its item.
enum Change {
    Made { item: ItemId, track: String },
    Changed(ItemId),
    Gone { item: ItemId, track: String },
}

impl Change {
    fn item(&self) -> &ItemId {
        match self {
            Self::Made { item, .. } | Self::Changed(item) | Self::Gone { item, .. } => item,
        }
    }
}

/// `op` on `p`, and what it did.
fn apply(p: &mut ProjectState, op: &ItemOp) -> DawResult<Change> {
    let missing = |item: &ItemId| DawError::not_found("Item", item.as_str());
    match op {
        ItemOp::Copy { item, into } => {
            let copy = crate::item::copy_item_in(p, item.as_str(), into.to_string())
                .ok_or_else(|| missing(item))?;
            Ok(Change::Made {
                item: into.clone(),
                track: copy.track_guid,
            })
        }
        ItemOp::Place {
            item,
            at,
            length,
            offset,
        } => {
            let entry = p
                .items
                .get_mut(item.as_str())
                .ok_or_else(|| missing(item))?;
            entry.item.position = PositionInSeconds::from_seconds(*at);
            entry.item.length = Duration::from_seconds(*length);
            // An item with no take (an empty one) has no source to play
            // from: placed, and nothing slipped.
            let _ = crate::take::slip_take_in(p, item.as_str(), *offset);
            Ok(Change::Changed(item.clone()))
        }
        ItemOp::Lane { item, lane } => {
            let track = p
                .items
                .get(item.as_str())
                .map(|e| e.item.track_guid.clone())
                .ok_or_else(|| missing(item))?;
            let lanes = p
                .tracks
                .iter()
                .find(|t| t.guid == track)
                .map_or(0, |t| t.lane_count);
            crate::track::check_lane(*lane, lanes)?;
            if let Some(entry) = p.items.get_mut(item.as_str()) {
                entry.item.fixed_lane = Some(*lane);
            }
            Ok(Change::Changed(item.clone()))
        }
        ItemOp::Fades {
            item,
            fade_in,
            fade_out,
        } => {
            let entry = p
                .items
                .get_mut(item.as_str())
                .ok_or_else(|| missing(item))?;
            entry.item.fade_in_length = Duration::from_seconds(*fade_in);
            entry.item.fade_out_length = Duration::from_seconds(*fade_out);
            Ok(Change::Changed(item.clone()))
        }
        ItemOp::Stretch { item, markers } => {
            let take = active_take(p, item).ok_or_else(|| missing(item))?;
            if markers.is_empty() {
                p.stretch_markers.remove(&take);
            } else {
                let mut sorted = markers.clone();
                sorted.sort_by(|a, b| a.position.total_cmp(&b.position));
                p.stretch_markers.insert(take, sorted);
            }
            Ok(Change::Changed(item.clone()))
        }
        ItemOp::Delete { item } => {
            let track = crate::item::delete_in(p, item.as_str()).ok_or_else(|| missing(item))?;
            Ok(Change::Gone {
                item: item.clone(),
                track,
            })
        }
    }
}

/// The guid of `item`'s active take.
fn active_take(p: &ProjectState, item: &ItemId) -> Option<String> {
    let list = p.takes.get(item.as_str())?;
    let index = usize::try_from(list.active_idx).ok()?;
    list.takes.get(index).map(|t| t.guid.clone())
}

#[cfg(test)]
mod tests {
    use super::ItemOp;
    use daw_proto::id::{ItemId, ProjectId};
    use daw_proto::{
        ItemRef, Items as _, ProjectContext, TakeRef, Takes as _, TrackRef, Tracks as _,
    };

    #[test]
    fn a_list_of_ops_is_applied_in_order_under_one_lock() {
        let daw = crate::sync::Standalone::new();
        let song = daw.seed_project(daw_proto::ProjectInfo {
            guid: "ops-test".into(),
            name: "Ops".into(),
            path: String::new(),
        });
        let ctx = ProjectContext::Project(song.clone());
        let track = daw.add(ctx.clone(), "Kick", None).expect("a track");
        let source = daw
            .add_item(
                ctx.clone(),
                TrackRef::Guid(track.clone()),
                daw_proto::PositionInSeconds::from_seconds(1.0),
                daw_proto::Duration::from_seconds(4.0),
            )
            .expect("an item");
        let _ = daw.add_take(ctx.clone(), ItemRef::Guid(source.clone()));
        let song_id = ProjectId::from(song.as_str());
        let source = ItemId::from(source);
        let piece = ItemId::new();
        daw.apply_items(
            &song_id,
            &[
                ItemOp::Copy {
                    item: source.clone(),
                    into: piece.clone(),
                },
                ItemOp::Place {
                    item: piece.clone(),
                    at: 2.0,
                    length: 0.5,
                    offset: 1.0,
                },
                ItemOp::Fades {
                    item: piece.clone(),
                    fade_in: 0.005,
                    fade_out: 0.01,
                },
            ],
        )
        .expect("applied");
        let copy = daw
            .get_item(ctx.clone(), ItemRef::Guid(piece.to_string()))
            .expect("the copy");
        assert_eq!(copy.track_guid, track);
        assert!((copy.position.as_seconds() - 2.0).abs() < 1e-9);
        assert!((copy.length.as_seconds() - 0.5).abs() < 1e-9);
        assert!((copy.fade_out_length.as_seconds() - 0.01).abs() < 1e-9);
        let take = daw
            .get_active_take(ctx.clone(), ItemRef::Guid(piece.to_string()))
            .expect("its take");
        assert!((take.start_offset.as_seconds() - 1.0).abs() < 1e-9);
        let _ = TakeRef::Active;

        let gone = daw.apply_items(
            &song_id,
            &[
                ItemOp::Delete {
                    item: piece.clone(),
                },
                ItemOp::Lane {
                    item: piece.clone(),
                    lane: 0,
                },
            ],
        );
        assert!(gone.is_err(), "a lane for an item just deleted is refused");
        assert!(
            daw.get_item(ctx, ItemRef::Guid(piece.to_string()))
                .is_none(),
            "the ops before the refused one stand"
        );
    }
}
