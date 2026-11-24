//! The arrangement's gestures, as named actions
//! (`docs/design/actions-everywhere.md`).
//!
//! A drag in the arrangement — an item moved, trimmed, slipped, faded,
//! split, copied or deleted, a marker or a region dragged, items picked,
//! an item's volume dial or its take volume curve — is planned in the
//! window (`arrangement_view::arrange_edit`: snapping, a folded row standing for
//! its mics, ripple), drawn there at once, and committed when it ends as
//! ONE of these actions: its edits, in order, as one undo step. So `watch`
//! sees `Ran { id: "ARRANGE_TRIM" }` when a hand trims an item, and a
//! guide, a test or the CLI can make the very same edit:
//!
//! ```text
//! studio run ARRANGE_MOVE '{"label":"Move items","edits":[{"move":{"item":"{…}","at":12.5}}]}'
//! ```
//!
//! Every gesture action takes the same thing ([`GestureArg`]); which one a
//! gesture runs is what it did ([`gesture_id`]). A model editing items by
//! hand reaches for `ITEMS_SET` / `ITEM_SPLIT` / `ITEMS_DELETE` instead;
//! these replay a window's gesture exactly, the names it gave the pieces a
//! split or a copy makes included (`into`, answered in `made`).

use architect::action::{ActionList, ActionMeta, Effect};
use daw_proto::automation::{EnvelopePoint, EnvelopeRef, EnvelopeShape};
use daw_proto::item::FadeShape;
use daw_proto::primitives::AutomationMode;
use daw_proto::primitives::PositionInSeconds;
use facet::Facet;

use crate::actions::meta;
use crate::engine::Edit;

/// One edit of an arrangement gesture. Items by GUID (`item`); `into`
/// is the name the window gave the item a split or a copy makes.
#[derive(Facet, Clone, Debug, PartialEq)]
#[facet(rename_all = "snake_case")]
#[repr(u8)]
pub enum ArrangeEdit {
    /// The item to `at` seconds, what it plays with it.
    Move { item: String, at: f64 },
    /// The item to start at `at` and run `length` seconds (an edge drag).
    Trim { item: String, at: f64, length: f64 },
    /// The item starts `offset` seconds into its recording.
    Slip { item: String, offset: f64 },
    /// Its fade in, `length` seconds, in `shape`.
    FadeIn {
        item: String,
        length: f64,
        shape: FadeShape,
    },
    /// Its fade out.
    FadeOut {
        item: String,
        length: f64,
        shape: FadeShape,
    },
    /// Onto fixed lane `lane` of its track.
    Lane { item: String, lane: u32 },
    /// Its volume, a linear gain (1 is unity).
    Volume { item: String, gain: f64 },
    /// Its active take played through these stretch markers (none: as
    /// recorded).
    Stretch {
        item: String,
        markers: Vec<StretchAt>,
    },
    /// Its snap offset, seconds from its start.
    SnapOffset { item: String, at: f64 },
    /// Split at `at`: it keeps what is before, `into` what is after.
    Split { item: String, at: f64, into: String },
    /// A copy of it, named `into`, at `at`.
    Copy { item: String, at: f64, into: String },
    /// It, gone.
    Delete { item: String },
    /// It selected; `only` deselects the rest first.
    Select { item: String, only: bool },
    /// It deselected.
    Deselect { item: String },
    /// Every item selected.
    SelectAll,
    /// No item selected.
    DeselectAll,
    /// A marker at `at` in ruler lane `lane`.
    MarkerAdd { at: f64, lane: u32 },
    /// Marker `id` to `at`.
    MarkerMove { id: u32, at: f64 },
    /// Marker `id`, gone.
    MarkerRemove { id: u32 },
    /// A region from `from` to `to` in ruler lane `lane`.
    RegionAdd { from: f64, to: f64, lane: u32 },
    /// Region `id` from `from` to `to`.
    RegionBounds { id: u32, from: f64, to: f64 },
    /// Region `id`, gone.
    RegionRemove { id: u32 },
    /// The item's take volume curve, whole.
    TakeVolume {
        item: String,
        points: Vec<TakePoint>,
    },
    /// `envelope` of `track`: every point of it (`time` seconds into the
    /// song, `value` 0..1).
    Envelope {
        track: String,
        envelope: EnvelopeRef,
        points: Vec<TakePoint>,
    },
    /// Automation item `item` of `envelope`: its curve (its pool's, so
    /// every instance's; `time` seconds into it).
    ItemCurve {
        track: String,
        envelope: EnvelopeRef,
        item: u32,
        points: Vec<TakePoint>,
    },
    /// Automation item `item` moved to `at` seconds.
    ItemMove {
        track: String,
        envelope: EnvelopeRef,
        item: u32,
        at: f64,
    },
    /// An automation item made at `at` for `length` seconds — in `pool`
    /// (`-1`: a fresh one, its curve `points`).
    ItemAdd {
        track: String,
        envelope: EnvelopeRef,
        at: f64,
        length: f64,
        pool: i32,
        points: Vec<TakePoint>,
    },
    /// Automation item `item`'s bounds: where it starts, how long, how far
    /// into its source.
    ItemBounds {
        track: String,
        envelope: EnvelopeRef,
        item: u32,
        at: f64,
        length: f64,
        offset: f64,
    },
    /// Automation item `item`, gone.
    ItemDelete {
        track: String,
        envelope: EnvelopeRef,
        item: u32,
    },
    /// `envelope`'s lane shown or hidden.
    LaneShow {
        track: String,
        envelope: EnvelopeRef,
        show: bool,
    },
    /// `envelope`'s automation mode.
    LaneMode {
        track: String,
        envelope: EnvelopeRef,
        mode: AutomationMode,
    },
    /// `envelope`'s lane height, pixels.
    LaneHeight {
        track: String,
        envelope: EnvelopeRef,
        pixels: u32,
    },
}

/// Envelope points as the vocabulary says them.
fn points_of(points: &[EnvelopePoint]) -> Vec<TakePoint> {
    points
        .iter()
        .map(|p| TakePoint {
            time: p.time.as_seconds(),
            value: p.value,
            shape: p.shape,
            tension: p.tension,
        })
        .collect()
}

/// The vocabulary's points as the engine's.
fn envelope_points(points: &[TakePoint]) -> Vec<EnvelopePoint> {
    points
        .iter()
        .zip(0_u32..)
        .map(|(p, index)| EnvelopePoint {
            index,
            time: PositionInSeconds::from_seconds(p.time),
            value: p.value,
            shape: p.shape,
            tension: p.tension,
            selected: false,
        })
        .collect()
}

/// A stretch marker: `at` in the take, `source` in its recording.
#[derive(Facet, Clone, Copy, Debug, PartialEq)]
pub struct StretchAt {
    pub at: f64,
    pub source: f64,
}

/// A point of a take volume curve: `time` seconds into the item, `value`
/// 0..1, its shape to the next point.
#[derive(Facet, Clone, Copy, Debug, PartialEq)]
pub struct TakePoint {
    pub time: f64,
    pub value: f64,
    #[facet(default)]
    pub shape: EnvelopeShape,
    #[facet(default)]
    pub tension: f64,
}

impl ArrangeEdit {
    /// The window's edit as an arrangement edit; `None` for one that is
    /// not (a track's, an undo bracket, a barrier).
    #[must_use]
    pub fn of(edit: &Edit) -> Option<Self> {
        let item = |g: &String| g.clone();
        Some(match edit {
            Edit::MoveItem(g, at) => Self::Move {
                item: item(g),
                at: *at,
            },
            Edit::TrimItem(g, at, length) => Self::Trim {
                item: item(g),
                at: *at,
                length: *length,
            },
            Edit::SlipItem(g, offset) => Self::Slip {
                item: item(g),
                offset: *offset,
            },
            Edit::SetFadeIn(g, length, shape) => Self::FadeIn {
                item: item(g),
                length: *length,
                shape: *shape,
            },
            Edit::SetFadeOut(g, length, shape) => Self::FadeOut {
                item: item(g),
                length: *length,
                shape: *shape,
            },
            Edit::SetItemLane(g, lane) => Self::Lane {
                item: item(g),
                lane: *lane,
            },
            Edit::SetItemVolume(g, gain) => Self::Volume {
                item: item(g),
                gain: *gain,
            },
            Edit::SetStretchMarkers(g, markers) => Self::Stretch {
                item: item(g),
                markers: markers
                    .iter()
                    .map(|&(at, source)| StretchAt { at, source })
                    .collect(),
            },
            Edit::SetSnapOffset(g, at) => Self::SnapOffset {
                item: item(g),
                at: *at,
            },
            Edit::SplitItem(g, at, into) => Self::Split {
                item: item(g),
                at: *at,
                into: into.clone(),
            },
            Edit::CopyItem(g, at, into) => Self::Copy {
                item: item(g),
                at: *at,
                into: into.clone(),
            },
            Edit::DeleteItem(g) => Self::Delete { item: item(g) },
            Edit::SelectItem(g, only) => Self::Select {
                item: item(g),
                only: *only,
            },
            Edit::DeselectItem(g) => Self::Deselect { item: item(g) },
            Edit::SelectAllItems(_) => Self::SelectAll,
            Edit::DeselectAllItems(_) => Self::DeselectAll,
            Edit::AddMarker(_, at, lane) => Self::MarkerAdd {
                at: *at,
                lane: *lane,
            },
            Edit::MoveMarker(_, id, at) => Self::MarkerMove { id: *id, at: *at },
            Edit::RemoveMarker(_, id) => Self::MarkerRemove { id: *id },
            Edit::AddRegion(_, from, to, lane) => Self::RegionAdd {
                from: *from,
                to: *to,
                lane: *lane,
            },
            Edit::SetRegionBounds(_, id, from, to) => Self::RegionBounds {
                id: *id,
                from: *from,
                to: *to,
            },
            Edit::RemoveRegion(_, id) => Self::RegionRemove { id: *id },
            Edit::SetTakeVolume(g, points) => Self::TakeVolume {
                item: item(g),
                points: points_of(points),
            },
            Edit::SetEnvelope(t, e, points) => Self::Envelope {
                track: t.clone(),
                envelope: e.clone(),
                points: points_of(points),
            },
            Edit::SetItemCurve(t, e, i, points) => Self::ItemCurve {
                track: t.clone(),
                envelope: e.clone(),
                item: *i,
                points: points_of(points),
            },
            Edit::MoveAutomationItem(t, e, i, at) => Self::ItemMove {
                track: t.clone(),
                envelope: e.clone(),
                item: *i,
                at: *at,
            },
            Edit::AddAutomationItem(t, e, (at, length, pool), points) => Self::ItemAdd {
                track: t.clone(),
                envelope: e.clone(),
                at: *at,
                length: *length,
                pool: *pool,
                points: points_of(points),
            },
            Edit::SetAutomationItemBounds(t, e, i, (at, length, offset)) => Self::ItemBounds {
                track: t.clone(),
                envelope: e.clone(),
                item: *i,
                at: *at,
                length: *length,
                offset: *offset,
            },
            Edit::DeleteAutomationItem(t, e, i) => Self::ItemDelete {
                track: t.clone(),
                envelope: e.clone(),
                item: *i,
            },
            Edit::ShowEnvelope(t, e, show) => Self::LaneShow {
                track: t.clone(),
                envelope: e.clone(),
                show: *show,
            },
            Edit::SetEnvelopeMode(t, e, mode) => Self::LaneMode {
                track: t.clone(),
                envelope: e.clone(),
                mode: *mode,
            },
            Edit::SetLaneHeight(t, e, pixels) => Self::LaneHeight {
                track: t.clone(),
                envelope: e.clone(),
                pixels: *pixels,
            },
            _ => return None,
        })
    }

    /// The edit the engine carries out.
    #[must_use]
    pub fn edit(&self) -> Edit {
        let none = String::new;
        match self {
            Self::Move { item, at } => Edit::MoveItem(item.clone(), *at),
            Self::Trim { item, at, length } => Edit::TrimItem(item.clone(), *at, *length),
            Self::Slip { item, offset } => Edit::SlipItem(item.clone(), *offset),
            Self::FadeIn {
                item,
                length,
                shape,
            } => Edit::SetFadeIn(item.clone(), *length, *shape),
            Self::FadeOut {
                item,
                length,
                shape,
            } => Edit::SetFadeOut(item.clone(), *length, *shape),
            Self::Lane { item, lane } => Edit::SetItemLane(item.clone(), *lane),
            Self::Volume { item, gain } => Edit::SetItemVolume(item.clone(), *gain),
            Self::Stretch { item, markers } => Edit::SetStretchMarkers(
                item.clone(),
                markers.iter().map(|m| (m.at, m.source)).collect(),
            ),
            Self::SnapOffset { item, at } => Edit::SetSnapOffset(item.clone(), *at),
            Self::Split { item, at, into } => Edit::SplitItem(item.clone(), *at, into.clone()),
            Self::Copy { item, at, into } => Edit::CopyItem(item.clone(), *at, into.clone()),
            Self::Delete { item } => Edit::DeleteItem(item.clone()),
            Self::Select { item, only } => Edit::SelectItem(item.clone(), *only),
            Self::Deselect { item } => Edit::DeselectItem(item.clone()),
            Self::SelectAll => Edit::SelectAllItems(none()),
            Self::DeselectAll => Edit::DeselectAllItems(none()),
            Self::MarkerAdd { at, lane } => Edit::AddMarker(none(), *at, *lane),
            Self::MarkerMove { id, at } => Edit::MoveMarker(none(), *id, *at),
            Self::MarkerRemove { id } => Edit::RemoveMarker(none(), *id),
            Self::RegionAdd { from, to, lane } => Edit::AddRegion(none(), *from, *to, *lane),
            Self::RegionBounds { id, from, to } => Edit::SetRegionBounds(none(), *id, *from, *to),
            Self::RegionRemove { id } => Edit::RemoveRegion(none(), *id),
            Self::TakeVolume { item, points } => {
                Edit::SetTakeVolume(item.clone(), envelope_points(points))
            }
            Self::Envelope {
                track,
                envelope,
                points,
            } => Edit::SetEnvelope(track.clone(), envelope.clone(), envelope_points(points)),
            Self::ItemCurve {
                track,
                envelope,
                item,
                points,
            } => Edit::SetItemCurve(
                track.clone(),
                envelope.clone(),
                *item,
                envelope_points(points),
            ),
            Self::ItemMove {
                track,
                envelope,
                item,
                at,
            } => Edit::MoveAutomationItem(track.clone(), envelope.clone(), *item, *at),
            Self::ItemAdd {
                track,
                envelope,
                at,
                length,
                pool,
                points,
            } => Edit::AddAutomationItem(
                track.clone(),
                envelope.clone(),
                (*at, *length, *pool),
                envelope_points(points),
            ),
            Self::ItemBounds {
                track,
                envelope,
                item,
                at,
                length,
                offset,
            } => Edit::SetAutomationItemBounds(
                track.clone(),
                envelope.clone(),
                *item,
                (*at, *length, *offset),
            ),
            Self::ItemDelete {
                track,
                envelope,
                item,
            } => Edit::DeleteAutomationItem(track.clone(), envelope.clone(), *item),
            Self::LaneShow {
                track,
                envelope,
                show,
            } => Edit::ShowEnvelope(track.clone(), envelope.clone(), *show),
            Self::LaneMode {
                track,
                envelope,
                mode,
            } => Edit::SetEnvelopeMode(track.clone(), envelope.clone(), *mode),
            Self::LaneHeight {
                track,
                envelope,
                pixels,
            } => Edit::SetLaneHeight(track.clone(), envelope.clone(), *pixels),
        }
    }

    /// Whether every number in it is one (no NaN, no infinity).
    fn is_finite(&self) -> bool {
        let all = |xs: &[f64]| xs.iter().all(|x| x.is_finite());
        match self {
            Self::Move { at, .. } | Self::SnapOffset { at, .. } => at.is_finite(),
            Self::Split { at, .. } | Self::Copy { at, .. } => at.is_finite(),
            Self::Trim { at, length, .. } => all(&[*at, *length]),
            Self::Slip { offset, .. } => offset.is_finite(),
            Self::FadeIn { length, .. } | Self::FadeOut { length, .. } => length.is_finite(),
            Self::Volume { gain, .. } => gain.is_finite(),
            Self::Stretch { markers, .. } => markers.iter().all(|m| all(&[m.at, m.source])),
            Self::MarkerAdd { at, .. } | Self::MarkerMove { at, .. } => at.is_finite(),
            Self::RegionAdd { from, to, .. } | Self::RegionBounds { from, to, .. } => {
                all(&[*from, *to])
            }
            Self::TakeVolume { points, .. }
            | Self::Envelope { points, .. }
            | Self::ItemCurve { points, .. } => {
                points.iter().all(|p| all(&[p.time, p.value, p.tension]))
            }
            Self::ItemMove { at, .. } => at.is_finite(),
            Self::ItemAdd {
                at, length, points, ..
            } => all(&[*at, *length]) && points.iter().all(|p| all(&[p.time, p.value, p.tension])),
            Self::ItemBounds {
                at, length, offset, ..
            } => all(&[*at, *length, *offset]),
            Self::ItemDelete { .. }
            | Self::LaneShow { .. }
            | Self::LaneMode { .. }
            | Self::LaneHeight { .. } => true,
            Self::Lane { .. }
            | Self::Delete { .. }
            | Self::Select { .. }
            | Self::Deselect { .. }
            | Self::SelectAll
            | Self::DeselectAll
            | Self::MarkerRemove { .. }
            | Self::RegionRemove { .. } => true,
        }
    }
}

/// What every gesture action takes: its edits, in order, and the undo
/// step they make (`label`; none: no step of its own — a selection).
#[derive(Facet)]
pub struct GestureArg {
    #[facet(default)]
    pub label: Option<String>,
    pub edits: Vec<ArrangeEdit>,
}

/// A name the window gave an item, and the engine's own for it.
#[derive(Facet)]
pub struct Made {
    pub predicted: String,
    pub real: String,
}

/// What a gesture action says: how many edits it made, and the engine's
/// names for the items its splits and copies made (`made`).
#[derive(Facet)]
pub struct Carried {
    pub edits: u32,
    pub made: Vec<Made>,
}

macro_rules! gesture {
    ($name:ident, $id:literal, $method:literal, $display:literal, $what:literal) => {
        pub static $name: ActionMeta = meta(
            $id,
            $method,
            $display,
            concat!(
                $what,
                " `edits`: the gesture's edits, in order (each one of `move`, `trim`, `slip`, `fade_in`, `fade_out`, `lane`, `volume`, `stretch`, `snap_offset`, `split`, `copy`, `delete`, `select`, `deselect`, `select_all`, `deselect_all`, `marker_add`, `marker_move`, `marker_remove`, `region_add`, `region_bounds`, `region_remove`, `take_volume`, and on an automation lane `envelope`, `item_curve`, `item_move`, `item_add`, `item_bounds`, `item_delete`, `lane_show`, `lane_mode`, `lane_height` (each with `track` and its `envelope`); items by GUID, `into` the name the window gives the item a split or a copy makes); `label` names the undo step they make together. Says how many edits it made and, in `made`, the engine's GUID for each item named by `into`. To edit items by hand prefer ITEMS_SET, ITEM_SPLIT, ITEMS_DELETE."
            ),
            "Arrange",
            "",
        );
    };
}

gesture!(
    MOVE,
    "ARRANGE_MOVE",
    "arrange_move",
    "Move items",
    "What a drag of an item's body commits when it lets go: items moved in time or onto another lane (the marks joined to them carried along)."
);
gesture!(
    TRIM,
    "ARRANGE_TRIM",
    "arrange_trim",
    "Trim items",
    "What a drag of an item's edge commits: its start or end moved (a stretch, an edge's snap offset with it)."
);
gesture!(
    SLIP,
    "ARRANGE_SLIP",
    "arrange_slip",
    "Slip item contents",
    "What a slip drag commits: what items play moved inside them, the items staying put."
);
gesture!(
    FADE,
    "ARRANGE_FADE",
    "arrange_fade",
    "Fade items",
    "What a drag of an item's fade handle commits: its fade in or out lengthened or shortened."
);
gesture!(
    SPLIT,
    "ARRANGE_SPLIT",
    "arrange_split",
    "Split items",
    "What a split in the arrangement commits (the razor's click, a split at the pointer): items cut in two."
);
gesture!(
    COPY,
    "ARRANGE_COPY",
    "arrange_copy",
    "Copy items",
    "What a modified drag of an item or a paste commits: copies of items, where they landed."
);
gesture!(
    DELETE,
    "ARRANGE_DELETE",
    "arrange_delete",
    "Delete items",
    "What deleting in the arrangement commits (the razor's area, items picked): items or the parts of them in an area, gone."
);
gesture!(
    MARKERS,
    "ARRANGE_MARKERS",
    "arrange_markers",
    "Edit markers and regions",
    "What a drag of a marker or a region's edge on the ruler commits, or a mark made or removed there."
);
gesture!(
    ITEM_VOLUME,
    "ARRANGE_ITEM_VOLUME",
    "arrange_item_volume",
    "Item volume",
    "What an item's volume dial commits when it lets go."
);
gesture!(
    TAKE_VOLUME,
    "ARRANGE_TAKE_VOLUME",
    "arrange_take_volume",
    "Take volume curve",
    "What a drag on an item's take volume curve commits: a point made, moved or deleted."
);
gesture!(
    AUTOMATION_POINTS,
    "AUTOMATION_POINTS_EDIT",
    "automation_points_edit",
    "Edit automation points",
    "What a press or drag in an automation lane commits: a point drawn, dragged or deleted — the envelope's points as they ended."
);
gesture!(
    AUTOMATION_ITEMS,
    "AUTOMATION_ITEMS_EDIT",
    "automation_items_edit",
    "Edit automation items",
    "What a gesture on automation items commits: one made (Shift-drag) or copied into a pool (Ctrl-drag), moved, its edges trimmed, its curve drawn, or deleted (Alt-click)."
);
gesture!(
    AUTOMATION_LANE,
    "AUTOMATION_LANE_SET",
    "automation_lane_set",
    "Set an automation lane",
    "What an automation lane's header commits: the lane hidden, its mode chip stepped to the next mode, its height dragged."
);
gesture!(
    SELECT,
    "ITEMS_SELECT",
    "items_select",
    "Select items",
    "What a click on an item, a marquee or a modified click commits: which items are selected."
);
gesture!(
    EDIT,
    "ARRANGE_EDIT",
    "arrange_edit",
    "Edit the arrangement",
    "What an arrangement gesture commits that is more than one of the others (a razor carving items, a cut that splits and trims)."
);

/// Every gesture action.
pub static GESTURES: [&ActionMeta; 15] = [
    &AUTOMATION_POINTS,
    &AUTOMATION_ITEMS,
    &AUTOMATION_LANE,
    &MOVE,
    &TRIM,
    &SLIP,
    &FADE,
    &SPLIT,
    &COPY,
    &DELETE,
    &MARKERS,
    &ITEM_VOLUME,
    &TAKE_VOLUME,
    &SELECT,
    &EDIT,
];

/// The action a gesture of `edits` runs: what it did. A selection made
/// along the way does not count; one kind of edit is that kind's action
/// (a trim's stretch and snap offset are the trim's; a split's snap
/// offset the split's); anything else is `ARRANGE_EDIT`.
#[must_use]
pub fn gesture_id(edits: &[ArrangeEdit]) -> &'static str {
    use ArrangeEdit as E;
    let mut kinds = std::collections::BTreeSet::new();
    let mut selects = false;
    let mut snaps = false;
    for edit in edits {
        let kind = match edit {
            E::Select { .. } | E::Deselect { .. } | E::SelectAll | E::DeselectAll => {
                selects = true;
                continue;
            }
            // Goes with whatever it is part of.
            E::SnapOffset { .. } => {
                snaps = true;
                continue;
            }
            E::Move { .. } | E::Lane { .. } => MOVE.id,
            E::Trim { .. } | E::Stretch { .. } => TRIM.id,
            E::Slip { .. } => SLIP.id,
            E::FadeIn { .. } | E::FadeOut { .. } => FADE.id,
            E::Split { .. } => SPLIT.id,
            E::Copy { .. } => COPY.id,
            E::Delete { .. } => DELETE.id,
            E::MarkerAdd { .. }
            | E::MarkerMove { .. }
            | E::MarkerRemove { .. }
            | E::RegionAdd { .. }
            | E::RegionBounds { .. }
            | E::RegionRemove { .. } => MARKERS.id,
            E::Volume { .. } => ITEM_VOLUME.id,
            E::TakeVolume { .. } => TAKE_VOLUME.id,
            E::Envelope { .. } => AUTOMATION_POINTS.id,
            E::ItemCurve { .. }
            | E::ItemMove { .. }
            | E::ItemAdd { .. }
            | E::ItemBounds { .. }
            | E::ItemDelete { .. } => AUTOMATION_ITEMS.id,
            E::LaneShow { .. } | E::LaneMode { .. } | E::LaneHeight { .. } => AUTOMATION_LANE.id,
        };
        kinds.insert(kind);
    }
    // A move drags the marks joined to it along.
    if kinds.len() == 2 && kinds.contains(MOVE.id) && kinds.contains(MARKERS.id) {
        return MOVE.id;
    }
    match (kinds.len(), kinds.first()) {
        (0, _) if snaps => TRIM.id,
        (0, _) if selects => SELECT.id,
        (1, Some(only)) => *only,
        _ => EDIT.id,
    }
}

/// A gesture of the window's `edits` (in order, no undo brackets), as the
/// action it runs and that action's arguments (JSON). `None` when an edit
/// is not an arrangement edit — or when there are none.
#[must_use]
pub fn gesture(label: Option<&str>, edits: &[Edit]) -> Option<(&'static str, String)> {
    let edits: Vec<ArrangeEdit> = edits.iter().map(ArrangeEdit::of).collect::<Option<_>>()?;
    if edits.is_empty() {
        return None;
    }
    let id = gesture_id(&edits);
    let arg = GestureArg {
        label: label.map(str::to_owned),
        edits,
    };
    facet_json::to_string(&arg).ok().map(|json| (id, json))
}

/// A gesture's edits carried out on the song this engine plays, in order,
/// as one undo step when it has a label — exactly as the window's edits
/// always were (`crate::engine::apply_reported`, the view link's `apply`).
async fn carry_out(arg: GestureArg) -> Result<Carried, String> {
    if let Some(bad) = arg.edits.iter().find(|e| !e.is_finite()) {
        return Err(format!("not a number in {bad:?}"));
    }
    let label = arg.label.filter(|l| !l.trim().is_empty());
    if let Some(label) = &label {
        crate::engine::apply_reported(&Edit::BeginUndo(label.clone())).await;
    }
    let mut carried = Carried {
        edits: 0,
        made: Vec::new(),
    };
    let mut items = false;
    for edit in arg.edits.iter().map(ArrangeEdit::edit) {
        items |= edit.is_item();
        crate::engine::apply_reported(&edit).await;
        carried.edits = carried.edits.saturating_add(1);
        if let Some(predicted) = edit.predicted() {
            let real = crate::engine::made(predicted);
            if real != predicted {
                carried.made.push(Made {
                    predicted: predicted.to_owned(),
                    real,
                });
            }
        }
    }
    if let Some(label) = label {
        crate::engine::apply_reported(&Edit::EndUndo(label)).await;
    }
    if items {
        crate::hit_overlay::refresh();
        #[cfg(feature = "native")]
        crate::body::body().view_changed(None, crate::view::Change::Items);
    }
    Ok(carried)
}

/// Put the gesture actions on the list.
///
/// # Errors
/// When one is on it already.
pub fn register(list: &ActionList) -> Result<(), architect::action::ActionError> {
    for meta in GESTURES {
        list.register_async(meta, Effect::Mutating, carry_out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind() -> Vec<Edit> {
        let g = || "{item}".to_owned();
        let t = || "{track}".to_owned();
        let e = || EnvelopeRef::FxParam {
            fx_guid: "{fx}".to_owned(),
            param_index: 3,
        };
        let point = || EnvelopePoint {
            index: 0,
            time: PositionInSeconds::from_seconds(2.0),
            value: 0.25,
            shape: EnvelopeShape::Linear,
            tension: 0.0,
            selected: false,
        };
        vec![
            Edit::SetEnvelope(t(), e(), vec![point()]),
            Edit::SetItemCurve(t(), e(), 1, vec![point()]),
            Edit::MoveAutomationItem(t(), e(), 1, 8.0),
            Edit::AddAutomationItem(t(), e(), (4.0, 2.0, -1), vec![point()]),
            Edit::SetAutomationItemBounds(t(), e(), 1, (4.0, 3.0, 0.5)),
            Edit::DeleteAutomationItem(t(), e(), 1),
            Edit::ShowEnvelope(t(), e(), false),
            Edit::SetEnvelopeMode(t(), e(), AutomationMode::Touch),
            Edit::SetLaneHeight(t(), e(), 80),
            Edit::MoveItem(g(), 1.0),
            Edit::TrimItem(g(), 1.0, 2.0),
            Edit::SlipItem(g(), 0.5),
            Edit::SetFadeIn(g(), 0.1, FadeShape::FastStart),
            Edit::SetFadeOut(g(), 0.2, FadeShape::Linear),
            Edit::SetItemLane(g(), 2),
            Edit::SetItemVolume(g(), 0.5),
            Edit::SetStretchMarkers(g(), vec![(0.0, 0.0), (1.0, 1.5)]),
            Edit::SetSnapOffset(g(), 0.25),
            Edit::SplitItem(g(), 1.5, "{right}".to_owned()),
            Edit::CopyItem(g(), 4.0, "{copy}".to_owned()),
            Edit::DeleteItem(g()),
            Edit::SelectItem(g(), true),
            Edit::DeselectItem(g()),
            Edit::SelectAllItems(String::new()),
            Edit::DeselectAllItems(String::new()),
            Edit::AddMarker(String::new(), 3.0, 1),
            Edit::MoveMarker(String::new(), 4, 3.5),
            Edit::RemoveMarker(String::new(), 4),
            Edit::AddRegion(String::new(), 1.0, 2.0, 0),
            Edit::SetRegionBounds(String::new(), 2, 1.0, 3.0),
            Edit::RemoveRegion(String::new(), 2),
            Edit::SetTakeVolume(
                g(),
                vec![EnvelopePoint {
                    index: 0,
                    time: PositionInSeconds::from_seconds(0.5),
                    value: 0.7,
                    shape: EnvelopeShape::Linear,
                    tension: 0.0,
                    selected: false,
                }],
            ),
        ]
    }

    #[test]
    fn every_arrangement_edit_comes_back_as_it_went() {
        for edit in every_kind() {
            let there = ArrangeEdit::of(&edit).expect("an arrangement edit");
            assert_eq!(there.edit(), edit);
        }
    }

    #[test]
    fn a_gesture_survives_its_trip_as_json() {
        let edits = every_kind();
        let (id, json) = gesture(Some("Everything"), &edits).expect("a gesture");
        assert_eq!(id, EDIT.id);
        // Named as the description and the CLI name them.
        for name in [
            r#"{"move":"#,
            r#"{"fade_in":"#,
            r#""select_all""#,
            r#"{"take_volume":"#,
        ] {
            assert!(json.contains(name), "{name} not in {json}");
        }
        let back: GestureArg = facet_json::from_str(&json).expect("its arguments read back");
        assert_eq!(back.label.as_deref(), Some("Everything"));
        let edits_back: Vec<Edit> = back.edits.iter().map(ArrangeEdit::edit).collect();
        assert_eq!(edits_back, edits);
    }

    #[test]
    fn a_gesture_runs_the_action_named_for_what_it_did() {
        let g = || "{i}".to_owned();
        let of = |edits: Vec<Edit>| gesture(None, &edits).map(|(id, _)| id);
        assert_eq!(of(vec![Edit::MoveItem(g(), 1.0)]), Some(MOVE.id));
        assert_eq!(
            of(vec![
                Edit::MoveItem(g(), 1.0),
                Edit::MoveMarker(String::new(), 1, 1.0)
            ]),
            Some(MOVE.id)
        );
        assert_eq!(
            of(vec![
                Edit::TrimItem(g(), 1.0, 2.0),
                Edit::SetStretchMarkers(g(), vec![]),
                Edit::SetSnapOffset(g(), 0.1),
            ]),
            Some(TRIM.id)
        );
        assert_eq!(
            of(vec![
                Edit::SplitItem(g(), 1.0, "{r}".to_owned()),
                Edit::SetSnapOffset(g(), 0.1)
            ]),
            Some(SPLIT.id)
        );
        assert_eq!(
            of(vec![
                Edit::DeselectAllItems(String::new()),
                Edit::SelectItem(g(), false)
            ]),
            Some(SELECT.id)
        );
        assert_eq!(
            of(vec![
                Edit::SplitItem(g(), 1.0, "{r}".to_owned()),
                Edit::DeleteItem(g())
            ]),
            Some(EDIT.id)
        );
        let e = || EnvelopeRef::FxParam {
            fx_guid: "{fx}".to_owned(),
            param_index: 0,
        };
        assert_eq!(
            of(vec![Edit::SetEnvelope(g(), e(), vec![])]),
            Some(AUTOMATION_POINTS.id)
        );
        assert_eq!(
            of(vec![
                Edit::AddAutomationItem(g(), e(), (1.0, 1.0, -1), vec![]),
                Edit::SetItemCurve(g(), e(), 0, vec![])
            ]),
            Some(AUTOMATION_ITEMS.id)
        );
        assert_eq!(
            of(vec![Edit::SetLaneHeight(g(), e(), 60)]),
            Some(AUTOMATION_LANE.id)
        );
        assert_eq!(of(vec![Edit::ToggleMute(g())]), None);
        assert_eq!(of(vec![]), None);
    }
}
