//! What the engine asks of the window it has attached — the window's own
//! actions queued for its next frame ([`queue`], [`take_window`]): a key
//! the Action List ran, an action that needs the arrangement to do it
//! (`SESSION_ZOOM`, a drum restore the arrangement slips).
//!
//! Engine side: the queue and whether a window is attached ([`attach`]).
//! With no window attached nothing is queued — there is nobody to run it —
//! and an action that needs one says so ([`attached`]).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// One slice of a stretch: its guid, where it plays and how long (µs),
/// and its stretch markers (`(played, source)`, µs).
pub type StretchEdit = (String, i64, i64, Vec<(i64, i64)>);

/// What a key asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    PlayStop,
    PlayPause,
    GoToStart,
    /// Split the selected items at the edit cursor — or, with nothing
    /// selected, every item under it.
    SplitAtCursor,
    /// Quantize what is selected (`EDIT_QUANTIZE`): for the drums the
    /// kit, edited from its hit markers.
    QuantizeDrums,
    /// T / Shift+T: comp the selection — a fill's hits, rows, a razor, a
    /// time selection — from the next (1) or previous (-1) take in its
    /// ranking (`COMP_CYCLE`).
    CompCycle(i32),
    /// Ctrl+T: comp the selection — or, with none, the whole song — from
    /// each phrase's best take, as one edit (`COMP_BEST`).
    CompBest,
    /// Ctrl+Shift+T: keep the take comped over the selection (`COMP_KEEP`),
    /// or let a kept stretch go again.
    CompKeep,
    /// Rank a take up (`by` 1) or down (-1) by hand (`TAKES_RANK`): the
    /// take under the mouse first (`mouse`, buttons 4 and 5), else the
    /// selected takes — the last recording's once it ends — else the
    /// mouse's (Up and Down); while recording, the take being recorded
    /// from where the press falls.
    RankTake {
        by: i8,
        mouse: bool,
    },
    /// Level what is selected (`EDIT_LEVEL`), in Edit mode: for the drums
    /// each selected piece's strokes brought toward one another.
    LevelDrums,
    /// Show the selected track's lanes, or only the one that plays
    /// (`TRACK_SHOW_LANES` with no track: the lanes button's action, on
    /// the selection).
    ShowLanes,
    /// Put the drums back as played over the selected folded rows' span
    /// or the time selection (`DRUMS_REVERT`): the edit undone there, the
    /// hit markers kept.
    RevertDrums,
    /// Show or hide the Quantize panel (`crate::quantize_panel`).
    QuantizePanel,
    /// Slip-edit the whole kit — every folded row of the drums folder —
    /// by these moves: `(hit, shift)`, microseconds (whole numbers, so
    /// an action compares). What `QUANTIZE` on the Action List works
    /// out and hands the window, which carries it out where it is seen.
    /// Fill the lanes with a span of time (µs from the start) and/or
    /// these tracks (by GUID): a model looking at a fill, a section, the
    /// kit.
    Frame {
        time: Option<(i64, i64)>,
        tracks: Vec<String>,
    },
    /// Take `from..to` (µs) out of these tracks' edited takes (by GUID),
    /// or the whole kit's when there are none.
    RemoveRange {
        tracks: Vec<String>,
        from_us: i64,
        to_us: i64,
    },
    /// Hits moved by stretching the audio either side of them (a hit
    /// marker's foot dragged, `HITS_STRETCH`): each slice's new place,
    /// length and stretch markers, µs — `(guid, position, length,
    /// [(played, source)])`.
    StretchDrums {
        edits: Vec<StretchEdit>,
        /// Sent back as [`crate::engine::Edit::Done`] once the edits are.
        ticket: u64,
    },
    SlipDrums {
        moves: Vec<(i64, i64)>,
        /// The cut before each hit and the seams' crossfade, µs.
        pre_roll_us: i64,
        crossfade_us: i64,
        /// A gap longer than this is stretched over (0: never).
        stretch_over_us: i64,
        /// Every hit of the kit where it plays now, µs, by time: no seam
        /// reaches back over one.
        attacks_us: Vec<i64>,
        /// Sent back as [`crate::engine::Edit::Done`] once the edits are.
        ticket: u64,
    },
    /// The kit as recorded over `from..to` (µs): its EDIT lanes made its
    /// COMP lanes again there (`arrange_edit::Editor::restore`). What
    /// `EDIT_REVERT` queues for the window.
    RestoreDrums {
        from_us: i64,
        to_us: i64,
        /// Sent back as [`crate::engine::Edit::Done`] once the edits are.
        ticket: u64,
    },
    /// Gain stage the session (Shift+G), or with `clear` (Alt+G) put every
    /// staged item back at an item volume of 1 (`crate::gain_stage`).
    GainStage {
        clear: bool,
    },
    /// Ctrl+N: comp the razor areas from take N — on a folded row, every
    /// mic in the folder at once (`arrange_edit::Editor::comp`).
    CompTake(u32),
    DeleteSelectedItems,
    SelectAllItems,
    /// Snapping on or off: the main toolbar's switch.
    ToggleSnap,
    /// The selected items onto the arrangement's clipboard.
    CopyItems,
    /// The clipboard's items at the edit cursor, each on its own track.
    PasteItems,
    /// Escape: no time selection, nothing selected.
    ClearSelection,
    /// The edit cursor by a bar, or by a beat.
    CursorBar(i32),
    CursorBeat(i32),
    /// Track selection, up or down the panel; `extend` keeps the rest.
    TrackStep {
        by: i32,
        extend: bool,
    },
    /// The edit cursor to the previous or next marker.
    Marker(i32),
    ToggleRecord,
    /// Something about the view rather than the session: a zoom.
    View(crate::zoom::Command),
    /// The visibility manager: show or hide a group of tracks.
    Visibility(Visibility),
    /// Show or hide the docked mixer.
    ToggleMixer,
    /// The Editor: docked under the arrangement, or a view of its own
    /// (`e`, the profile's `FTS_EXPRESSION_EDITOR_OPEN`).
    ToggleEditor,
    /// Show or hide the FX (`f`): the selected track's effects as devices.
    ToggleFx,
    /// Show or hide the browser (`b`): what to load on the selected track.
    ToggleBrowser,
    /// Hide every edge dock of the window, or bring back the ones hidden
    /// (Shift+D, the profile's `FTS_TOGGLE_DOCKS`).
    ToggleDocks,
    /// The view a page of bars on, or back (`]`, `[`): `SESSION_STEP`.
    BarStep(i32),
    /// To the next point of interest or the one before (`}`, `{`):
    /// `POI_STEP`.
    PoiStep(i32),
    /// The point of interest stepped to ticked off, or open again (`|`):
    /// `POI_TOGGLE`.
    PoiToggle,
    /// The window into a mode (`Alt+1` … `Alt+9`, `Alt+0`, and the
    /// `Alt+m` tree): `SESSION_MODE`, by the mode's name.
    Mode(&'static str),
    /// The FX's own view, full size (`F`): several tracks' chains.
    FxFull,
    /// Show the session as a scene (`v v` Overview, `v t` Overview TOP),
    /// or, in it already, go back to the view before.
    ToggleScene(&'static str),
    /// Save the song (Ctrl+S, REAPER's "Save project"): its `.session`,
    /// as `SESSION_SAVE` does.
    Save,
    /// Undo the last step (Ctrl+Z, REAPER's 40029), as `SESSION_UNDO`.
    Undo,
    /// Redo what was undone (Ctrl+Shift+Z, REAPER's 40030), as
    /// `SESSION_REDO`.
    Redo,
    /// A binding the window does not do yet, by its REAPER id.
    Unbound(String),
}

/// The visibility manager's actions (`v` and its tree).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Show the group if all of it is hidden, else hide it. The group's
    /// name as the action spells it (`DRUMS`), matched loosely.
    Toggle(String),
    ShowAll,
    HideAll,
}

/// The window's actions waiting for it.
static WINDOW: Mutex<VecDeque<Action>> = Mutex::new(VecDeque::new());

/// The window's actions run since it last looked, in order.
pub fn take_window() -> Vec<Action> {
    let taken: Vec<Action> = WINDOW
        .lock()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    if !taken.is_empty() {
        FOLLOW_UP.store(true, Ordering::Release);
    }
    taken
}

/// Whether any are waiting.
pub fn window_pending() -> bool {
    WINDOW.lock().is_ok_and(|q| !q.is_empty())
}

/// Whether the window owes a frame for actions it just carried out —
/// taken, so one frame is asked for each time. For the shell's frame
/// loop alone.
///
/// The window sends its edits to the engine at the start of a frame and
/// carries out these actions while painting one, so what they edited
/// goes on the frame AFTER. No event is coming to ask for it.
pub fn frame_owed() -> bool {
    FOLLOW_UP.swap(false, Ordering::AcqRel)
}

/// A frame owed for actions just carried out.
static FOLLOW_UP: AtomicBool = AtomicBool::new(false);

/// Queue `action` for the attached window's next frame; dropped when no
/// window is attached (nobody would ever take it). Whether it was queued.
pub fn queue(action: Action) -> bool {
    if !attached() {
        tracing::debug!(?action, "window action dropped: no window attached");
        return false;
    }
    if let Ok(mut q) = WINDOW.lock() {
        q.push_back(action);
        return true;
    }
    false
}

// ── What the engine asks of a window ───────────────────────────────

/// What the engine asks a window to do: an edit the arrangement carries
/// out where it is seen (the kit slipped by the hits an action moved), or
/// a place to look. Engine side: queued with [`ask`], taken by a window
/// over its link (`crate::view::EngineView::take_window`) — each by one
/// window — and turned into its [`Action`].
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, facet::Facet)]
pub enum Asked {
    GoToStart,
    /// See [`Action::Frame`].
    Frame {
        time: Option<(i64, i64)>,
        tracks: Vec<String>,
    },
    /// See [`Action::RemoveRange`].
    RemoveRange {
        tracks: Vec<String>,
        from_us: i64,
        to_us: i64,
    },
    /// See [`Action::StretchDrums`].
    StretchDrums {
        edits: Vec<StretchEdit>,
        ticket: u64,
    },
    /// See [`Action::SlipDrums`].
    SlipDrums {
        moves: Vec<(i64, i64)>,
        pre_roll_us: i64,
        crossfade_us: i64,
        stretch_over_us: i64,
        attacks_us: Vec<i64>,
        ticket: u64,
    },
    /// See [`Action::RestoreDrums`].
    RestoreDrums {
        from_us: i64,
        to_us: i64,
        ticket: u64,
    },
    /// Run one of the window's own actions (`id`, its arguments as JSON) —
    /// what an engine action does to the view (a step pages it), or one run
    /// on the engine's Action List for the window ([`ask_window_action`]).
    /// A `ticket` other than 0 wants the result back
    /// (`crate::view::EngineView::answer`).
    Run {
        id: String,
        args: String,
        ticket: u64,
    },
}

impl From<Asked> for Action {
    fn from(asked: Asked) -> Self {
        match asked {
            Asked::GoToStart => Self::GoToStart,
            Asked::Frame { time, tracks } => Self::Frame { time, tracks },
            Asked::RemoveRange {
                tracks,
                from_us,
                to_us,
            } => Self::RemoveRange {
                tracks,
                from_us,
                to_us,
            },
            Asked::StretchDrums { edits, ticket } => Self::StretchDrums { edits, ticket },
            Asked::SlipDrums {
                moves,
                pre_roll_us,
                crossfade_us,
                stretch_over_us,
                attacks_us,
                ticket,
            } => Self::SlipDrums {
                moves,
                pre_roll_us,
                crossfade_us,
                stretch_over_us,
                attacks_us,
                ticket,
            },
            Asked::RestoreDrums {
                from_us,
                to_us,
                ticket,
            } => Self::RestoreDrums {
                from_us,
                to_us,
                ticket,
            },
            Asked::Run { id, .. } => Self::Unbound(id),
        }
    }
}

/// What the engine has asked, not yet taken by a window — each with the
/// window it is for (`None`: whichever takes it first).
static ASKED: Mutex<VecDeque<(Option<String>, Asked)>> = Mutex::new(VecDeque::new());

/// The window the engine's asks go to, by name (`SESSION_WINDOW_TARGET`);
/// `None`: whichever window takes them first.
static TARGET: Mutex<Option<String>> = Mutex::new(None);

/// The windows that have taken asks, by name, newest last.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Send the engine's asks to the window `name` from now on (`None`: to
/// whichever window takes them first).
pub fn set_target(name: Option<String>) {
    if let Ok(mut target) = TARGET.lock() {
        *target = name.filter(|n| !n.is_empty());
    }
}

/// The window the engine's asks go to, if one was named.
#[must_use]
pub fn target() -> Option<String> {
    TARGET.lock().ok().and_then(|t| t.clone())
}

/// The windows that have followed this engine and taken its asks, by name.
#[must_use]
pub fn seen_windows() -> Vec<String> {
    SEEN.lock().map(|s| s.clone()).unwrap_or_default()
}

/// This window's name, as it gives it to its engine (the one it
/// [`claim_as`], else `FTS_WINDOW_NAME`, else `main`): what
/// `SESSION_WINDOW_TARGET` names it by.
#[must_use]
pub fn window_name() -> String {
    if let Some(name) = CLAIMED_AS.get() {
        return name.clone();
    }
    std::env::var("FTS_WINDOW_NAME")
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "main".to_owned())
}

// ── Actions a window claims ──────────────────────────────────────────
//
// A window that shows one part of Studio (an Architect pane: the mixer
// alone) claims the window actions that part carries out (`MIXER_VIEW`,
// the mixer's toggle): the engine asks them of it by name, and it takes
// nothing else. Another window in the same session hands them over: a
// key of its that runs one goes to the engine, which asks the claimer.

/// The name this window claims its actions under, once it does.
static CLAIMED_AS: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// This window claims its own actions as the window `name`: it offers
/// them under that name, and the engine asks them of it alone and asks it
/// nothing else. Once per process (the first name stays).
pub fn claim_as(name: &str) {
    let _ = CLAIMED_AS.set(name.to_owned());
}

/// The name this window claims its actions under, when it does.
#[must_use]
pub fn claimed_as() -> Option<&'static str> {
    CLAIMED_AS.get().map(String::as_str)
}

/// The window actions another window claims, handed over by this one:
/// pressed here, they run on the engine, which asks the claimer.
static HANDED_OVER: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// This window leaves the actions `ids` to the window that claims them:
/// it neither offers them nor runs them itself.
pub fn hand_over(ids: &[&str]) {
    if let Ok(mut over) = HANDED_OVER.lock() {
        for id in ids {
            if !over.iter().any(|o| o == id) {
                over.push((*id).to_owned());
            }
        }
    }
}

/// Whether this window handed `id` over to the window that claims it.
#[must_use]
pub fn handed_over(id: &str) -> bool {
    HANDED_OVER
        .lock()
        .is_ok_and(|over| over.iter().any(|o| o == id))
}

/// Engine side: the window that claimed each window action, by its id.
static OWNERS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Engine side: the window `window` claimed the action `id`; the last
/// claim stands.
pub fn set_owner(id: &str, window: &str) {
    if let Ok(mut owners) = OWNERS.lock() {
        owners.retain(|(i, _)| i != id);
        owners.push((id.to_owned(), window.to_owned()));
    }
}

/// Engine side: the window that claimed the action `id`, if one did.
#[must_use]
pub fn owner(id: &str) -> Option<String> {
    OWNERS
        .lock()
        .ok()?
        .iter()
        .find(|(i, _)| i == id)
        .map(|(_, w)| w.clone())
}

/// Engine side: whether the window `window` claimed actions, and so takes
/// only what is asked of it by name.
fn claims(window: &str) -> bool {
    OWNERS
        .lock()
        .is_ok_and(|owners| owners.iter().any(|(_, w)| w == window))
}

/// How many windows follow this engine over a link (the engine's view
/// host counts them): what [`ask`] needs one of.
static LINKED_WINDOWS: AtomicUsize = AtomicUsize::new(0);

/// Told when something is asked: the engine's view host, which tells the
/// windows.
static ON_ASK: std::sync::OnceLock<fn(usize)> = std::sync::OnceLock::new();

/// The engine's view host counts `windows` following it.
pub fn set_linked_windows(windows: usize) {
    LINKED_WINDOWS.store(windows, Ordering::Release);
}

/// Whether a window follows this engine.
#[must_use]
pub fn engine_has_window() -> bool {
    LINKED_WINDOWS.load(Ordering::Acquire) > 0
}

/// How the engine's view host hears of an ask (once per process).
pub fn on_ask(notify: fn(usize)) {
    let _ = ON_ASK.set(notify);
}

/// Ask a window to do `asked`; refused (`false`) when no window follows
/// the engine — nobody would ever take it.
pub fn ask(asked: Asked) -> bool {
    ask_of(target(), asked)
}

/// [`ask`] of the window `window` (by name; `None`: whichever takes it
/// first).
fn ask_of(window: Option<String>, asked: Asked) -> bool {
    if !engine_has_window() {
        tracing::debug!(?asked, "engine ask dropped: no window follows the engine");
        return false;
    }
    let pending = match ASKED.lock() {
        Ok(mut q) => {
            q.push_back((window, asked));
            q.len()
        }
        Err(_) => return false,
    };
    if let Some(notify) = ON_ASK.get() {
        notify(pending);
    }
    true
}

/// What has been asked of the window `window` (by name), taken: one
/// window carries each out — the one it was for, else the first to ask.
/// A window that claimed actions takes only what was asked of it.
#[must_use]
pub fn take_asked(window: &str) -> Vec<Asked> {
    let only_its_own = claims(window);
    if let Ok(mut seen) = SEEN.lock() {
        seen.retain(|w| w != window);
        seen.push(window.to_owned());
    }
    let Ok(mut q) = ASKED.lock() else {
        return Vec::new();
    };
    let (mine, others): (Vec<_>, Vec<_>) = q
        .drain(..)
        .partition(|(for_window, _)| for_window.as_deref().map_or(!only_its_own, |w| w == window));
    q.extend(others);
    mine.into_iter().map(|(_, asked)| asked).collect()
}

/// What is waiting to be taken.
#[must_use]
pub fn asked_pending() -> usize {
    ASKED.lock().map_or(0, |q| q.len())
}

/// Hand `asked` to this window: its own actions run (`run` runs one by
/// id, with the ticket its result goes back on), the rest queued for its
/// next frame.
pub fn receive(asked: Vec<Asked>, run: impl Fn(&str, &str, u64)) {
    let mut queued = Vec::new();
    for one in asked {
        match one {
            Asked::Run { id, args, ticket } => run(&id, &args, ticket),
            other => queued.push(Action::from(other)),
        }
    }
    if queued.is_empty() {
        return;
    }
    if let Ok(mut q) = WINDOW.lock() {
        q.extend(queued);
    }
}

// ── The window's actions, run from the engine's list ────────────────

/// How long the engine waits for a window to answer one of its actions.
#[cfg(feature = "native")]
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// What a window's action came to: its result as JSON, or why not.
pub type Answer = Result<String, String>;

/// The asks waiting on a window's answer, by ticket.
#[cfg(feature = "native")]
static WAITING: Mutex<
    Option<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<Answer>>>,
> = Mutex::new(None);

/// The last ticket given out.
#[cfg(feature = "native")]
static TICKETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run the window's action `id` with `args` (JSON) there, and what it
/// gave back: what the engine's Action List does with a window action
/// (`crate::view::EngineView::window_actions`).
///
/// # Errors
///
/// No window follows the engine, it refused, or did not answer in time.
#[cfg(feature = "native")]
pub async fn ask_window_action(id: String, args: String) -> Answer {
    let ticket = TICKETS.fetch_add(1, Ordering::AcqRel) + 1;
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Ok(mut waiting) = WAITING.lock() {
        waiting
            .get_or_insert_with(std::collections::HashMap::new)
            .insert(ticket, tx);
    }
    let forget = || {
        if let Ok(mut waiting) = WAITING.lock()
            && let Some(map) = waiting.as_mut()
        {
            map.remove(&ticket);
        }
    };
    let to = owner(&id).or_else(target);
    if !ask_of(to, Asked::Run { id, args, ticket }) {
        forget();
        return Err("no window follows the engine".to_owned());
    }
    match tokio::time::timeout(ANSWER_WITHIN, rx).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(_)) => Err("the window went before it answered".to_owned()),
        Err(_) => {
            forget();
            Err(format!(
                "the window did not answer within {} s",
                ANSWER_WITHIN.as_secs()
            ))
        }
    }
}

/// A window's answer to the ask `ticket`; one nobody waits for (late, or
/// answered twice) is dropped.
#[cfg(feature = "native")]
pub fn answered(ticket: u64, answer: Answer) {
    let waiting = WAITING
        .lock()
        .ok()
        .and_then(|mut waiting| waiting.as_mut().and_then(|map| map.remove(&ticket)));
    if let Some(tx) = waiting {
        let _ = tx.send(answer);
    }
}

/// Whether a window follows the engine, as an Action List enabled check
/// for the actions it runs there.
///
/// # Errors
///
/// None follows it.
pub fn a_window_follows() -> Result<(), String> {
    if engine_has_window() {
        Ok(())
    } else {
        Err("no window follows the engine".to_owned())
    }
}

/// The ids of the window's own actions, once registered: what a window
/// offers its engine's list (not those it handed over).
#[must_use]
pub fn window_action_ids() -> Vec<String> {
    WINDOW_IDS
        .get()
        .map(|ids| ids.iter().filter(|id| !handed_over(id)).cloned().collect())
        .unwrap_or_default()
}

// ── The windows attached ─────────────────────────────────────────────

/// The ids of the window's own actions, once a window registered them.
static WINDOW_IDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// The window's own actions are `ids` (what it registered on the list).
pub fn window_actions(ids: Vec<String>) {
    let _ = WINDOW_IDS.set(ids);
}

/// Whether `id` is one of the window's own actions — what runs in the
/// window's process even when the engine is in another.
#[must_use]
pub fn is_window_action(id: &str) -> bool {
    !handed_over(id)
        && WINDOW_IDS
            .get()
            .is_some_and(|ids| ids.iter().any(|i| i == id))
}

/// How many windows are attached to this engine.
static WINDOWS: AtomicUsize = AtomicUsize::new(0);

/// A window attached to the engine, for as long as it is held: its
/// actions run ([`queue`]) and the window actions on the Action List are
/// enabled. Dropping it detaches the window.
#[derive(Debug)]
pub struct Attached(());

/// Attach a window.
#[must_use]
pub fn attach() -> Attached {
    WINDOWS.fetch_add(1, Ordering::AcqRel);
    Attached(())
}

impl Drop for Attached {
    fn drop(&mut self) {
        let before = WINDOWS.fetch_sub(1, Ordering::AcqRel);
        if before <= 1 {
            // The last window went: what it had not taken goes with it.
            if let Ok(mut q) = WINDOW.lock() {
                q.clear();
            }
        }
    }
}

/// Whether a window is attached.
#[must_use]
pub fn attached() -> bool {
    WINDOWS.load(Ordering::Acquire) > 0
}

/// Why a window action cannot run now, if it cannot: what the Action List
/// says of each while no window is attached.
///
/// # Errors
///
/// No window is attached.
pub fn needs_a_window() -> Result<(), String> {
    if attached() {
        Ok(())
    } else {
        Err("no window attached: this is the window's action".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_is_open_only_while_a_window_is_attached() {
        // One test owns the process's seat, so the steps run in order.
        assert!(!attached());
        assert!(needs_a_window().is_err());
        assert!(!queue(Action::GoToStart), "nobody to take it");
        let first = attach();
        let second = attach();
        assert!(queue(Action::GoToStart));
        drop(first);
        assert!(attached(), "one window is still there");
        assert!(window_pending());
        drop(second);
        assert!(!attached());
        assert!(!window_pending(), "the last window's queue goes with it");
        assert!(needs_a_window().is_err());
    }
}

#[cfg(all(test, feature = "native"))]
mod window_action_tests {
    use super::{Asked, answered, ask_window_action, set_linked_windows, take_asked};

    /// The queue and the window count are the process's: the tests that
    /// set them run one at a time.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// The ticket of the one `Run` asked so far, taken.
    fn asked_ticket() -> u64 {
        match take_asked("main").as_slice() {
            [Asked::Run { ticket, .. }] => *ticket,
            other => panic!("one run asked, not {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_window_action_is_asked_of_the_window_and_its_answer_comes_back() {
        let _serial = SERIAL.lock().await;
        // No window follows the engine: nobody to ask.
        set_linked_windows(0);
        assert!(
            ask_window_action("SESSION_VIEW".into(), "{}".into())
                .await
                .is_err()
        );

        set_linked_windows(1);
        let asking = tokio::spawn(ask_window_action("SESSION_FRAMES".into(), "{}".into()));
        let ticket = loop {
            if super::asked_pending() > 0 {
                break asked_ticket();
            }
            tokio::task::yield_now().await;
        };
        assert_ne!(ticket, 0);
        answered(ticket, Ok(r#"{"fps":118.2}"#.to_owned()));
        assert_eq!(
            asking.await.expect("joined"),
            Ok(r#"{"fps":118.2}"#.to_owned())
        );

        // A refusal comes back as one.
        let asking = tokio::spawn(ask_window_action("SESSION_VIEW".into(), "{}".into()));
        let ticket = loop {
            if super::asked_pending() > 0 {
                break asked_ticket();
            }
            tokio::task::yield_now().await;
        };
        answered(ticket, Err("bad args".to_owned()));
        assert_eq!(asking.await.expect("joined"), Err("bad args".to_owned()));
        set_linked_windows(0);
    }

    /// The ids of the `Run`s in `asked`.
    fn runs(asked: &[Asked]) -> Vec<String> {
        asked
            .iter()
            .filter_map(|a| match a {
                Asked::Run { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_claimed_action_is_asked_of_the_window_that_claimed_it_alone() {
        let _serial = SERIAL.lock().await;
        set_linked_windows(2);
        super::set_owner("MIXER_VIEW", "studio.mixer");
        let mixer = tokio::spawn(ask_window_action("MIXER_VIEW".into(), "{}".into()));
        let zoom = tokio::spawn(ask_window_action("SESSION_ZOOM".into(), "{}".into()));
        while super::asked_pending() < 2 {
            tokio::task::yield_now().await;
        }
        // The claimer takes only what was asked of it by name, so the
        // arrangement's zoom waits for a window that takes any ask.
        assert_eq!(runs(&take_asked("studio.mixer")), ["MIXER_VIEW"]);
        assert_eq!(runs(&take_asked("studio.mixer")), Vec::<String>::new());
        assert_eq!(runs(&take_asked("main")), ["SESSION_ZOOM"]);
        mixer.abort();
        zoom.abort();
        set_linked_windows(0);
    }
}

#[cfg(test)]
mod hand_over_tests {
    #[test]
    fn a_handed_over_action_is_not_this_windows_own() {
        super::window_actions(vec!["SESSION_ZOOM".into(), "MIXER_VIEW".into()]);
        assert!(super::is_window_action("MIXER_VIEW"));
        super::hand_over(&["MIXER_VIEW"]);
        assert!(!super::is_window_action("MIXER_VIEW"));
        assert!(super::is_window_action("SESSION_ZOOM"));
        assert_eq!(super::window_action_ids(), ["SESSION_ZOOM"]);
    }
}
