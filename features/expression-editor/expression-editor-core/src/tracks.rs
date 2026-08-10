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

use core::sync::atomic::{AtomicU64, Ordering};

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
    /// Stable identity, and the only thing durable data may reference.
    ///
    /// The host supplies it — the DAW adapter passes REAPER's track
    /// GUID, tests pass something readable — and it never changes for
    /// the life of the track. A bare `String` rather than a newtype:
    /// considered and declined, the accepted trade being that a track
    /// guid and a take guid are both bare strings.
    ///
    /// Indices into [`Workspace::tracks`] remain correct for in-memory
    /// addressing and are used throughout. The rule this exists to make
    /// possible is narrower and absolute: **nothing durable may hold an
    /// index**. A stored lane layout keyed on position is re-targeted by
    /// inserting a track above it; keyed on name, it is broken by
    /// [`Workspace::rename`].
    pub guid: String,
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

/// Source of generated guids for tracks nobody named.
///
/// Guids are project-scoped, so a counter is sufficient: two projects
/// may both contain `t1` and never meet. Generation happens once, at
/// construction — the value is then persisted with the track, so it is
/// stable across save and reload, which is the only stability that
/// matters here.
static NEXT_GUID: AtomicU64 = AtomicU64::new(1);

fn generated_guid() -> String {
    let n = NEXT_GUID.fetch_add(1, Ordering::Relaxed);
    let mut s = String::from("t");
    s.push_str(&n.to_string());
    s
}

impl Track {
    /// A track with a generated guid, for callers with no host identity
    /// to supply — the standalone editor, tests, demo scenes.
    pub fn new(name: impl Into<String>, doc: ExpressionDoc) -> Self {
        Self::with_guid(generated_guid(), name, doc)
    }

    /// A track carrying the host's identity. This is the constructor the
    /// DAW adapter uses, so that persisted data keyed on a guid means
    /// the same track when the project is reopened.
    pub fn with_guid(
        guid: impl Into<String>,
        name: impl Into<String>,
        doc: ExpressionDoc,
    ) -> Self {
        Self {
            guid: guid.into(),
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
    /// has sized a track by hand, switching modes must not silently undo
    /// that.
    pub fn set_mode(&mut self, mode: Mode) {
        if (self.weight - self.mode.stack_weight()).abs() < f32::EPSILON {
            self.weight = mode.stack_weight();
        }
        self.mode = mode;
    }
}

/// A lane: the tracks drawn on one shared vertical strip.
///
/// Lanes are a **view** concern, not a container for ownership —
/// everything still layers into one window, and a lane exists only to
/// split the visualisation. That is why a track's document, history and
/// mode stay on the track and none of them appear here.
///
/// A lane holds as many tracks as you want. The motivating case is a
/// vocal and its MIDI guide superimposed, because tuning one against the
/// other means seeing them on the same axis — and, inversely, two
/// instruments doubling a line need *separate* lanes, because layered on
/// top of each other they are invisible.
///
/// **A lane has no identity of its own.** It *is* its set of track
/// guids, and its position in [`LaneLayout`] is its order. Nothing
/// dereferences a lane durably — the per-lane camera is ephemeral,
/// weight lives here, and merge/split rewrite the list wholesale — so an
/// id would be a field that never gets read and quietly drifts.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    /// Track guids, in draw order. Never indices: a lane outlives the
    /// insertion of a track above it.
    pub tracks: Vec<String>,
    /// Height weight relative to the other lanes.
    pub weight: f32,
    /// What `weight` would be if nobody had dragged it.
    ///
    /// Kept so a member's mode change can update the height *only while
    /// it is still the default* — the same bargain [`Track::set_mode`]
    /// strikes, and for the same reason: once you have sized a lane by
    /// hand, switching modes must not silently undo it.
    default_weight: f32,
}

impl Lane {
    /// A lane holding one track, at that track's natural height.
    pub fn single(guid: impl Into<String>, weight: f32) -> Self {
        Self {
            tracks: vec![guid.into()],
            weight,
            default_weight: weight,
        }
    }

    pub fn contains(&self, guid: &str) -> bool {
        self.tracks.iter().any(|g| g == guid)
    }

    /// Whether the height has been set by hand.
    pub fn is_hand_sized(&self) -> bool {
        (self.weight - self.default_weight).abs() > f32::EPSILON
    }

    /// Take a new natural height, unless the user has overridden it.
    pub fn set_default_weight(&mut self, weight: f32) {
        if !self.is_hand_sized() {
            self.weight = weight;
        }
        self.default_weight = weight;
    }
}

/// The lanes of a workspace, in top-to-bottom order.
///
/// Persisted as one unit and **project-scoped** — a lane spans tracks
/// across the whole song, so keying it per take would give every track a
/// conflicting copy of the same layout, and keying it per track leaves
/// lane order and lane weight with nowhere to live.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneLayout {
    lanes: Vec<Lane>,
}

impl LaneLayout {
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    pub fn lanes(&self) -> &[Lane] {
        &self.lanes
    }

    pub fn lane(&self, i: usize) -> Option<&Lane> {
        self.lanes.get(i)
    }

    pub fn lane_mut(&mut self, i: usize) -> Option<&mut Lane> {
        self.lanes.get_mut(i)
    }

    pub fn push(&mut self, lane: Lane) -> usize {
        self.lanes.push(lane);
        self.lanes.len() - 1
    }

    /// Which lane holds this track.
    pub fn lane_of(&self, guid: &str) -> Option<usize> {
        self.lanes.iter().position(|l| l.contains(guid))
    }

    /// Drop a track from whichever lane holds it, removing the lane if
    /// that empties it. A lane with no tracks is not a lane.
    pub fn forget(&mut self, guid: &str) {
        if let Some(i) = self.lane_of(guid) {
            self.lanes[i].tracks.retain(|g| g != guid);
            if self.lanes[i].tracks.is_empty() {
                self.lanes.remove(i);
            }
        }
    }
}

/// One lane's slot in a stacked layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackRow {
    /// Index into [`Workspace::layout`].
    pub lane: usize,
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
    /// How the tracks are grouped into vertical strips.
    ///
    /// Kept beside the tracks rather than derived from them, because it
    /// is the one part of this that a user arranges by hand and expects
    /// back — everything else here can be recomputed.
    layout: LaneLayout,
}

impl Workspace {
    /// A workspace around the document the editor was opened on.
    ///
    /// The slot's copy of `doc` is stale from this moment on — the live
    /// one lives in `Editor::doc`. See [`Workspace::doc_of`].
    pub fn single(name: impl Into<String>, doc: ExpressionDoc) -> Self {
        let track = Track::new(name, doc);
        let mut layout = LaneLayout::default();
        layout.push(Lane::single(track.guid.clone(), track.weight));
        Self {
            tracks: vec![track],
            active: 0,
            layout,
        }
    }

    pub fn layout(&self) -> &LaneLayout {
        &self.layout
    }

    pub fn layout_mut(&mut self) -> &mut LaneLayout {
        &mut self.layout
    }

    /// The track indices a lane draws, in draw order.
    ///
    /// Resolved from guids on every call. Lanes never cache indices —
    /// that is the whole point of holding guids.
    pub fn lane_tracks(&self, lane: usize) -> Vec<usize> {
        self.layout
            .lane(lane)
            .map(|l| {
                l.tracks
                    .iter()
                    .filter_map(|g| self.index_of_guid(g))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The lane holding the active track.
    pub fn active_lane(&self) -> Option<usize> {
        let guid = &self.tracks.get(self.active)?.guid;
        self.layout.lane_of(guid)
    }

    /// Whether a lane has anything to draw.
    ///
    /// A lane whose every track is hidden is not drawn at all — which is
    /// what gives both gestures, hiding a track *inside* a lane and
    /// hiding a whole lane, without a second flag to keep consistent.
    pub fn lane_is_visible(&self, lane: usize) -> bool {
        self.lane_tracks(lane)
            .iter()
            .any(|&i| !self.tracks[i].hidden)
    }

    /// The natural height for a lane: the **max** of its members' modes.
    ///
    /// Max rather than sum or mean, because the tallest member's need is
    /// what makes the lane readable — a kit layered with its MIDI still
    /// needs the kit's rows.
    pub fn natural_weight(&self, lane: usize) -> f32 {
        self.lane_tracks(lane)
            .iter()
            .map(|&i| self.tracks[i].mode.stack_weight())
            .fold(0.0_f32, f32::max)
            .max(0.01)
    }

    /// Re-apply natural heights after something changed a member's mode.
    /// Hand-sized lanes keep their height.
    pub fn refresh_lane_weights(&mut self) {
        for i in 0..self.layout.len() {
            let natural = self.natural_weight(i);
            if let Some(lane) = self.layout.lane_mut(i) {
                lane.set_default_weight(natural);
            }
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

    /// Resolve a guid to its current index.
    ///
    /// The index is a *result*, never something to store: it is valid
    /// until the next insert or remove. Durable data holds the guid and
    /// resolves it on load, which is what makes a stored layout survive
    /// a track being inserted above it or renamed.
    pub fn index_of_guid(&self, guid: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.guid == guid)
    }

    pub fn track_by_guid(&self, guid: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.guid == guid)
    }

    pub fn track_by_guid_mut(&mut self, guid: &str) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.guid == guid)
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
        // One row per *lane*, not per track. The boost goes to the
        // active lane — the active track inside it already carries the
        // highlight, so boosting per-track would double-count.
        let active_lane = self.active_lane();
        let rows: Vec<(usize, f32)> = (0..self.layout.len())
            .filter(|&i| self.lane_is_visible(i))
            .map(|i| {
                let weight = self.layout.lane(i).map(|l| l.weight).unwrap_or(1.0);
                let boost = if Some(i) == active_lane {
                    active_boost
                } else {
                    1.0
                };
                (i, weight.max(0.01) * boost.max(0.01))
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
                        lane: track,
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
                    lane: track,
                    y,
                    height: h,
                };
                y += h;
                row
            })
            .collect()
    }

    /// Which lane a y coordinate falls in.
    pub fn row_at(rows: &[StackRow], y: f32) -> Option<usize> {
        rows.iter()
            .find(|r| y >= r.y && y < r.y + r.height)
            .map(|r| r.lane)
    }

    /// Parked tracks marked as references, with their indices.
    pub fn references(&self) -> impl Iterator<Item = (usize, &Track)> {
        self.tracks
            .iter()
            .enumerate()
            .filter(move |(i, t)| *i != self.active && t.reference)
    }

    /// Add a track and return its index.
    ///
    /// It arrives in a lane of its own. Pairing it with an existing lane
    /// is the matcher's job, not this one's — and a track alone in a
    /// lane is the safe default, because too many lanes is legible and
    /// one merge away, whereas a wrong pairing hides a track underneath
    /// another, which is the failure lanes exist to prevent.
    pub fn push(&mut self, track: Track) -> usize {
        let lane = Lane::single(track.guid.clone(), track.mode.stack_weight());
        self.layout.push(lane);
        self.tracks.push(track);
        self.tracks.len() - 1
    }

    /// Remove a track. The active one cannot be removed — closing what
    /// you are editing is a host decision, not an editor one.
    pub fn remove(&mut self, i: usize) -> bool {
        if i == self.active || i >= self.tracks.len() {
            return false;
        }
        let guid = self.tracks[i].guid.clone();
        self.tracks.remove(i);
        self.layout.forget(&guid);
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
