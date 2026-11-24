//! The song a view draws: the project as the engine has it ([`Project`]),
//! the rows planned from it ([`RowsRef`]), its items' previews, and the
//! read-back an engine asks for after an edit ([`request_resync`]).
//!
//! Which rows a panel shows is the app's to plan: a [`Planner`] carries the
//! project and the app's [`Plans`], so a view that changes what is shown
//! (a track hidden, a scene toggled) plans again without knowing how.

pub mod folded;
pub mod project;

use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use daw_proto::Track;

pub use project::Project;

/// A project, shared: compared by identity, so a panel re-records only when
/// it is handed a different one.
#[derive(Clone)]
pub struct ProjectRef(pub Arc<Project>);

impl PartialEq for ProjectRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for ProjectRef {
    type Target = Project;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A panel's rows (each track and its folder depth), shared: compared by
/// identity.
#[derive(Clone)]
pub struct RowsRef(pub Arc<Vec<(Track, u32)>>);

impl PartialEq for RowsRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for RowsRef {
    type Target = Vec<(Track, u32)>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// How an app plans a panel's rows from a project: which tracks show, how
/// large, which folders start shut. Studio's lays a session out by its
/// template's scenes; a view only asks.
pub trait Plans: Send + Sync + 'static {
    /// The arrangement's project and rows for `raw`, laid out by `scene`.
    fn rows(&self, raw: &Project, scene: Option<&str>) -> (ProjectRef, RowsRef);

    /// A mixer's strips for `raw` beside the arrangement, by `scene`.
    fn strips(&self, raw: &Project, scene: Option<&str>) -> RowsRef;

    /// A scene's name, as a notice says it (`None`: no such scene).
    fn scene_name(&self, _slug: &str) -> Option<&'static str> {
        None
    }

    /// The tracks `names` grouped as a visibility change names them (a
    /// group's key, the names in it).
    ///
    /// # Errors
    ///
    /// The names could not be grouped.
    fn groups(
        &self,
        _names: Vec<String>,
    ) -> Result<std::collections::HashMap<String, Vec<String>>, String> {
        Ok(std::collections::HashMap::new())
    }

    /// A group's name as [`Self::groups`] keys it.
    fn group_key(&self, group: &str) -> String {
        group.trim().to_ascii_lowercase()
    }

    /// The app's own, for the app to read back (Studio's track kinds).
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Every row as the project has it, no folder shut: what a view plans
/// with when the app gave it no [`Plans`] (a test, a bench).
#[derive(Clone, Copy, Debug, Default)]
pub struct AsItIs;

impl Plans for AsItIs {
    fn rows(&self, raw: &Project, _scene: Option<&str>) -> (ProjectRef, RowsRef) {
        (ProjectRef(Arc::new(raw.clone())), self.strips(raw, None))
    }

    fn strips(&self, raw: &Project, _scene: Option<&str>) -> RowsRef {
        RowsRef(Arc::new(depths(&raw.tracks)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `tracks` with their folder depths, from each track's change of depth
/// (`folder_depth`: 1 opens a folder, -n closes n).
fn depths(tracks: &[Track]) -> Vec<(Track, u32)> {
    let mut depth = 0_u32;
    tracks
        .iter()
        .map(|track| {
            let here = depth;
            depth = depth.saturating_add_signed(track.folder_depth);
            (track.clone(), here)
        })
        .collect()
}

/// The row plan's inputs: the project as the engine had it (before the
/// plan folds anything), the scene it was laid out by, and how the app
/// plans ([`Plans`]).
#[derive(Clone)]
pub struct Planner {
    pub raw: Arc<Project>,
    pub scene: Option<&'static str>,
    plans: Arc<dyn Plans>,
}

impl Planner {
    /// A planner over `raw`, laid out by `scene` the way `plans` says.
    #[must_use]
    pub fn new(raw: Arc<Project>, scene: Option<&'static str>, plans: Arc<dyn Plans>) -> Self {
        Self { raw, scene, plans }
    }

    /// Plan the rows for `raw` with this planner's scene.
    #[must_use]
    pub fn plan(&self, raw: &Project) -> (ProjectRef, RowsRef) {
        self.plans.rows(raw, self.scene)
    }

    /// A mixer's strips for `raw` beside the arrangement.
    #[must_use]
    pub fn plan_strips(&self, raw: &Project) -> RowsRef {
        self.plans.strips(raw, self.scene)
    }

    /// How this planner plans.
    #[must_use]
    pub fn plans(&self) -> &dyn Plans {
        &*self.plans
    }
}

/// The song as a view reads it: the project, the rows planned from it, its
/// items' previews, and how to plan again. Cheap to clone: everything in it
/// is shared.
#[derive(Clone)]
pub struct SongView {
    pub project: ProjectRef,
    pub rows: RowsRef,
    pub previews: crate::midi::Previews,
    pub planner: Planner,
}

impl PartialEq for SongView {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.project.0, &other.project.0) && Arc::ptr_eq(&self.rows.0, &other.rows.0)
    }
}

// ── Read-back ────────────────────────────────────────────────────────

/// Asked for when something changed the song in the ENGINE rather than
/// through a view — a toolbar action, an edited chart laid over the song —
/// so the views read it back on their next frame.
static RESYNC: AtomicBool = AtomicBool::new(false);

/// Whether the read-back asked for must cover everything (the engine
/// changed something nobody named) or only items (the engine's journal
/// says which).
static ALL: AtomicBool = AtomicBool::new(false);

/// Where the window has read the engine's journal up to.
#[cfg(feature = "native")]
static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What a read-back must cover.
#[derive(Debug)]
pub enum Resync {
    /// The whole song: something changed that nobody named.
    All,
    /// Only items: those the engine's journal says changed.
    Items,
}

/// Ask the views to read the song back from the engine — all of it: the
/// safe ask, for a change that does not say what it touched.
pub fn request_resync() {
    request_resync_here();
    // And every window on the engine, over its link.
    #[cfg(feature = "native")]
    crate::body::body().view_changed(None, crate::view::Change::All);
}

/// [`request_resync`] for this window alone — what its link does when the
/// engine says something changed.
pub fn request_resync_here() {
    ALL.store(true, Ordering::Release);
    RESYNC.store(true, Ordering::Release);
}

/// Ask for the items that changed to be read back: an edit that changed
/// only items and nothing else of the song. Which items, the engine's
/// journal says.
pub fn request_resync_items() {
    request_resync_items_here();
    #[cfg(feature = "native")]
    crate::body::body().view_changed(None, crate::view::Change::Items);
}

/// [`request_resync_items`] for this window alone.
pub fn request_resync_items_here() {
    RESYNC.store(true, Ordering::Release);
}

/// Whether a read-back is asked for; asking clears it.
#[must_use]
pub fn take_resync() -> bool {
    take_resync_scope().is_some()
}

/// Whether one is waiting, without taking it — what a panel's
/// `needs_redraw` asks.
#[must_use]
pub fn resync_pending() -> bool {
    RESYNC.load(Ordering::Relaxed)
}

/// The read-back asked for, if one is; asking clears it.
#[must_use]
pub fn take_resync_scope() -> Option<Resync> {
    if !RESYNC.swap(false, Ordering::AcqRel) {
        return None;
    }
    if ALL.swap(false, Ordering::AcqRel) {
        Some(Resync::All)
    } else {
        Some(Resync::Items)
    }
}

/// The window has read the whole song as of now: the journal from here.
/// Taken BEFORE the read, so a write during it is read again next time.
#[cfg(feature = "native")]
pub fn mark_seen() {
    // The engine's journal, over the window's link: none, and every edit
    // reads the whole song back.
    SEEN.store(
        crate::view_client::journal_now().unwrap_or(0),
        Ordering::Release,
    );
}

/// The items the engine's journal says changed since the window last
/// read, read again into `raw` in place. The guids of those still in the
/// song, or `None` when the journal cannot say or there is no engine here
/// (read the whole song back instead).
#[cfg(feature = "native")]
#[must_use]
pub fn patch_items(raw: &mut Project) -> Option<Vec<String>> {
    let song = crate::open::current_song()?;
    let seen = SEEN.load(Ordering::Acquire);
    // Never read whole yet: nowhere to read on from.
    if seen == 0 {
        return None;
    }
    let changes = crate::view_client::item_changes(&song, seen)?;
    let changed = changes.changed?;
    SEEN.store(changes.cursor, Ordering::Release);
    Some(patch_with(raw, changed, &changes.gone))
}

/// `raw` with `changed` (each item as it is now) put where it is and
/// `gone` taken out: each where it was, taken out; then each where it is,
/// put back, its lane in order, its take's name and whether it is MIDI.
/// The guids of those still in the song.
#[must_use]
pub fn patch_with(
    raw: &mut Project,
    changed: Vec<crate::view::ItemNow>,
    gone: &[String],
) -> Vec<String> {
    if changed.is_empty() && gone.is_empty() {
        return Vec::new();
    }
    let named: std::collections::HashSet<&str> = changed
        .iter()
        .map(|c| c.item.guid.as_str())
        .chain(gone.iter().map(String::as_str))
        .collect();
    let mut lanes: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (track, items) in &mut raw.items {
        let before = items.len();
        items.retain(|i| !named.contains(i.guid.as_str()));
        if items.len() != before {
            lanes.insert(track.clone());
        }
    }
    for guid in &named {
        raw.names.remove(*guid);
        raw.midi.remove(*guid);
    }
    let mut now = Vec::new();
    for crate::view::ItemNow {
        item,
        take_name,
        midi,
    } in changed
    {
        let guid = item.guid.clone();
        if let Some(name) = take_name {
            raw.names.insert(guid.clone(), name);
        }
        if midi {
            raw.midi.insert(guid.clone());
        }
        lanes.insert(item.track_guid.clone());
        raw.items
            .entry(item.track_guid.clone())
            .or_default()
            .push(item);
        now.push(guid);
    }
    for track in &lanes {
        if let Some(items) = raw.items.get_mut(track) {
            project::sort_lane(items);
            if items.is_empty() {
                raw.items.remove(track);
            }
        }
    }
    raw.item_count = raw.items.values().map(Vec::len).sum();
    let reach = raw
        .items
        .values()
        .flatten()
        .map(|i| i.position.as_seconds() + i.length.as_seconds())
        .fold(0.0_f64, f64::max);
    raw.length_secs = reach.max(60.0);
    now
}

/// The open project, as the engine has it, with its automation lanes
/// adopted.
///
/// It blocks on a runtime of its own: on a thread that is driving one
/// already (an Architect process view paints inside its runtime), it
/// reads on a thread of its own, which it waits for.
#[cfg(feature = "native")]
#[must_use]
pub fn fetch() -> Option<Project> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::scope(|s| s.spawn(fetch_here).join().ok().flatten());
    }
    fetch_here()
}

/// [`fetch`], on a thread driving no runtime.
#[cfg(feature = "native")]
fn fetch_here() -> Option<Project> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let step = tracing::info_span!("fetch.read").entered();
    mark_seen();
    let project = rt.block_on(project::fetch())?;
    drop(step);
    let step = tracing::info_span!("fetch.automation").entered();
    // The lanes as the engine reads them (the leveler's among them), over
    // the window's link; a REAPER driven has its lanes on the facade.
    match crate::open::current_song().and_then(|song| crate::view_client::automation(&song)) {
        Some(read) => crate::automation::adopt_read(read),
        None => {
            let (lanes, takes) = rt.block_on(crate::automation::read_remote(&project.tracks));
            crate::automation::adopt_remote(lanes, takes);
        }
    }
    drop(step);
    Some(project)
}

/// Every MIDI item's notes, read before anything is drawn — an item drawn
/// from a waveform it does not have is why a chord track once looked like
/// a shaker — and every audio item's waveform behind the window.
#[must_use]
pub fn previews_of(project: &ProjectRef) -> crate::midi::Previews {
    let previews = crate::midi::Previews::default();
    previews.fill_blocking(
        project
            .0
            .items
            .values()
            .flatten()
            .filter(|item| project.0.is_midi(&item.guid))
            .map(|item| (item.guid.clone(), item.length.as_seconds()))
            .collect(),
    );
    let waves: Vec<String> = project
        .0
        .items
        .values()
        .flatten()
        .filter(|item| !project.0.is_midi(&item.guid))
        .map(|item| item.guid.clone())
        .collect();
    // The waveforms behind the window, each item drawn as it arrives:
    // reading them first held the window still while a song of thousands
    // of items opened.
    #[cfg(feature = "native")]
    {
        // Read as the song it is now: a set opening songs one after
        // another has moved on to the next by the time this runs.
        let song = crate::open::current_song();
        let reading = previews.clone();
        crate::wave_pool::spawn(move || reading.fill_waves_blocking_in(song, waves));
    }
    #[cfg(not(feature = "native"))]
    previews.fill_waves_blocking(waves);
    previews
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_s_children_sit_one_deeper() {
        let track = |name: &str, change: i32| Track {
            name: name.to_owned(),
            folder_depth: change,
            ..Track::default()
        };
        let rows = depths(&[
            track("Drums", 1),
            track("Kick", 0),
            track("Snare", -1),
            track("Bass", 0),
        ]);
        let got: Vec<u32> = rows.iter().map(|(_, d)| *d).collect();
        assert_eq!(got, [0, 1, 1, 0]);
    }
}
