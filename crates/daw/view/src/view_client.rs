//! A window's link to its engine: the one way a window reads and drives it.
//!
//! [`connect`] is handed a vox `Caller` that reaches an engine's router —
//! the daw facade and [`crate::view::EngineView`] side by side — over a
//! Unix socket (`studio engine`, on the desktop) or in memory (an engine in
//! the window's process: iOS, a test). Either way the window then:
//!
//! - **keeps replicas** of what it draws, fed by the engine's streams on a
//!   task of their own: the hit overlay (`crate::hit_overlay`), the
//!   tracks' marks ([`marks`]), the jobs, the performance meter, the live
//!   session ([`live`]), the playheads of
//!   every song ([`playhead_of`], from `TransportSync::positions`), what
//!   changed (`crate::song`'s read-back flags) and what the engine
//!   asks of it (`crate::window_queue::receive`). A frame reads these and
//!   never waits on the engine;
//! - **calls** the engine for what it reads on demand, off its frame: a
//!   song's waveforms ([`waves`]), a stretch's detail ([`detail`]), its
//!   automation ([`automation`]), what changed ([`item_changes`]), a song's
//!   facts ([`song_facts`]), an action ([`run`]), its edits ([`apply`]).
//!
//! A link that drops (an engine that died) is said in [`state`]; whoever
//! made the link makes another and [`connect`]s again — the replicas are
//! kept meanwhile and refreshed from the new link's intros.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use tokio::sync::watch;

use crate::view::{
    ActionCall, Applied, AutomationRead, ChangesAsk, DetailAsk, EditBatch, EngineViewClient,
    EngineViewStreamClient, ItemChanges, MarksFrame, SetFrame, SongFacts, WaveRead, WavesAsk,
};

/// How long a call may take before the window gives up on it. Giving up
/// on one call is that call's failure, not the link's: only the streams
/// ending ([`follow`]) say the engine went away.
const CALL: Duration = Duration::from_secs(20);

/// How long an action may take: some run long on purpose (a quantize
/// waits up to half a minute for the window to land its edits).
const RUN: Duration = Duration::from_secs(120);

/// The link: the engine's view service over one connection.
#[derive(Clone)]
struct Link {
    view: EngineViewClient,
    streams: EngineViewStreamClient,
    daw: daw_control::Daw,
    /// Bumped by every [`connect`]: a stream task of an older link stops.
    generation: u64,
}

static LINK: RwLock<Option<Link>> = RwLock::new(None);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How the link stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// Never linked.
    None,
    /// Linked, its streams flowing.
    Up,
    /// It dropped: the engine went away (or stopped answering).
    Down,
}

fn state_cell() -> &'static watch::Sender<LinkState> {
    static STATE: OnceLock<watch::Sender<LinkState>> = OnceLock::new();
    STATE.get_or_init(|| watch::Sender::new(LinkState::None))
}

/// How the link stands now.
#[must_use]
pub fn state() -> LinkState {
    *state_cell().borrow()
}

/// How the link stands, as it changes.
#[must_use]
pub fn watch_state() -> watch::Receiver<LinkState> {
    state_cell().subscribe()
}

/// Whether this window is linked to an engine.
#[must_use]
pub fn is_linked() -> bool {
    LINK.read().is_ok_and(|l| l.is_some())
}

fn link() -> Option<Link> {
    LINK.read().ok().and_then(|l| l.clone())
}

/// The runtime a window's link runs on.
fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    crate::open::engine_runtime().ok()
}

/// Link this window to the engine `caller` reaches (its router serves the
/// daw facade and the view service), and follow it: every replica fed
/// from now on. A link made before is let go.
pub fn connect(caller: vox::Caller) {
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
    // Each call carries the caller's trace context, so the engine's work
    // for it lands under the window's span (a song's read, its waveforms).
    let traced = || architect::trace_context::TraceContextMiddleware;
    let link = Link {
        view: EngineViewClient::new(caller.clone()).with_middleware(traced()),
        streams: EngineViewStreamClient::new(caller.clone()).with_middleware(traced()),
        daw: daw_control::Daw::new(caller),
        generation,
    };
    if let Ok(mut slot) = LINK.write() {
        *slot = Some(link.clone());
    }
    state_cell().send_replace(LinkState::Up);
    // A new engine has heard nothing of the window's edit cursor yet.
    forget_shown_edit();
    let Some(rt) = runtime() else {
        tracing::warn!("view link: no runtime to follow the engine on");
        return;
    };
    rt.spawn(follow(link.clone(), Stream::Set));
    rt.spawn(follow(link.clone(), Stream::Overlays));
    rt.spawn(follow(link.clone(), Stream::Marks));
    rt.spawn(follow(link.clone(), Stream::Streams));
    rt.spawn(follow(link.clone(), Stream::Jobs));
    rt.spawn(follow(link.clone(), Stream::Perf));
    rt.spawn(follow(link.clone(), Stream::Live));
    rt.spawn(follow(link.clone(), Stream::Edits));
    rt.spawn(follow(link.clone(), Stream::Changes));
    rt.spawn(follow(link.clone(), Stream::Queued));
    rt.spawn(follow_playheads(link));
    // A new engine knows nothing of the window's own actions yet.
    offer_window_actions();
}

/// Offer the window's own actions (once it has registered them) to the
/// engine's Action List: there, each runs here (`take_asked`). Called as
/// the link comes up and as the window registers them, whichever is last;
/// the engine keeps those it has.
pub fn offer_window_actions() {
    let ids = crate::window_queue::window_action_ids();
    let (Some(link), Some(rt)) = (link(), runtime()) else {
        return;
    };
    if ids.is_empty() {
        return;
    }
    let list = crate::actions::list();
    let actions: Vec<crate::view::WindowAction> = ids
        .iter()
        .filter_map(|id| list.info(id))
        .map(|info| crate::view::WindowAction {
            effect: match info.effect {
                architect::action::Effect::ReadOnly => "read-only",
                architect::action::Effect::Mutating => "mutating",
                architect::action::Effect::Destructive => "destructive",
            }
            .to_owned(),
            id: info.id,
            name: info.display_name,
            description: info.description,
            category: info.category,
            group: info.group,
            shortcut: info.shortcut,
            params_schema: info.params_schema,
            result_schema: info.result_schema,
            window: crate::window_queue::claimed_as()
                .unwrap_or_default()
                .to_owned(),
        })
        .collect();
    let view = link.view.clone();
    rt.spawn(async move {
        match tokio::time::timeout(CALL, view.window_actions(actions)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!(error = ?e, "view link: the engine did not take the window's actions"),
            Err(_) => tracing::warn!("view link: offering the window's actions timed out"),
        }
    });
}

/// Whether `link` is still the window's.
fn current(link: &Link) -> bool {
    GENERATION.load(std::sync::atomic::Ordering::Acquire) == link.generation
}

/// The window's link dropped: said once (the replicas stay as they were).
fn dropped(link: &Link, why: &str) {
    if !current(link) {
        return;
    }
    // Its jobs went with it, and its live session.
    if let Ok(mut slot) = JOBS.write() {
        *slot = None;
    }
    if let Ok(mut slot) = LIVE.write() {
        *slot = None;
    }
    if state_cell().send_replace(LinkState::Down) != LinkState::Down {
        tracing::warn!(link.why = why, "view link: the engine went away");
    }
}

#[derive(Clone, Copy, Debug)]
enum Stream {
    Set,
    Overlays,
    Marks,
    Streams,
    Jobs,
    Perf,
    Live,
    Edits,
    Changes,
    Queued,
}

/// Follow one of the engine's streams into its replica until the link
/// goes.
async fn follow(link: Link, which: Stream) {
    let ended = match which {
        Stream::Set => {
            let (tx, rx) = vox::channel::<SetFrame>();
            drain(link.streams.set(tx), rx, |frame| {
                crate::host::host().set_arrived(frame);
                crate::wake::wake();
            })
            .await
        }
        Stream::Overlays => {
            let (tx, rx) = vox::channel::<crate::view::OverlayFrame>();
            drain(link.streams.overlays(tx), rx, |frame| {
                crate::hit_overlay::publish(frame.overlay);
                crate::wave_detail::engine_changed();
            })
            .await
        }
        Stream::Marks => {
            let (tx, rx) = vox::channel::<MarksFrame>();
            drain(link.streams.marks(tx), rx, |frame| {
                marks_arrived(frame);
                crate::wake::wake();
            })
            .await
        }
        Stream::Streams => {
            let (tx, rx) = vox::channel::<crate::view::StreamsFrame>();
            drain(link.streams.streams(tx), rx, |frame| {
                crate::host::host().streams_arrived(frame);
                crate::wake::wake();
            })
            .await
        }
        Stream::Jobs => {
            let (tx, rx) = vox::channel::<crate::view::JobsFrame>();
            drain(link.streams.jobs(tx), rx, jobs_arrived).await
        }
        Stream::Perf => {
            let (tx, rx) = vox::channel::<crate::view::PerfFrame>();
            drain(link.streams.perf(tx), rx, perf_arrived).await
        }
        Stream::Live => {
            let (tx, rx) = vox::channel::<crate::view::LiveFrame>();
            drain(link.streams.live(tx), rx, |frame| {
                live_arrived(frame);
                crate::wake::wake();
            })
            .await
        }
        Stream::Edits => {
            let (tx, rx) = vox::channel::<crate::view::EditAsked>();
            drain(link.streams.edits(tx), rx, |frame| {
                edit_asked(frame);
                crate::wake::wake();
            })
            .await
        }
        Stream::Changes => {
            let (tx, rx) = vox::channel::<crate::view::Changed>();
            drain(link.streams.changes(tx), rx, |frame| {
                changed(frame);
                crate::wake::wake();
            })
            .await
        }
        Stream::Queued => {
            let (tx, rx) = vox::channel::<crate::view::Queued>();
            let taker = link.clone();
            drain(link.streams.queued(tx), rx, move |queued| {
                // Only a window with window actions of its own takes what
                // the engine asks of one: a process that follows the
                // engine without them (a mixer pane in a process of its
                // own) would answer every ask "no action", in place of
                // the window that has it.
                if queued.pending > 0 && !crate::window_queue::window_action_ids().is_empty() {
                    take_asked(&taker);
                }
            })
            .await
        }
    };
    dropped(&link, &format!("{which:?}: {ended}"));
}

/// Every frame of a held-open subscription to `on`, until it ends; why.
async fn drain<T, R, F>(
    call: impl std::future::Future<Output = Result<(), vox::VoxError<R>>>,
    mut rx: vox::Rx<T>,
    mut on: F,
) -> String
where
    T: facet::Facet<'static> + Send + 'static,
    R: std::fmt::Debug,
    F: FnMut(T),
{
    let mut call = std::pin::pin!(call);
    loop {
        tokio::select! {
            biased;
            res = &mut call => return format!("the stream ended: {res:?}"),
            frame = rx.recv() => match frame {
                Ok(Some(frame)) => {
                    let mut got = None;
                    let _ = frame.map(|f| got = Some(f));
                    if let Some(f) = got {
                        on(f);
                    }
                }
                other => return format!("the channel closed: {:?}", other.err()),
            },
        }
    }
}

// ── The marks ────────────────────────────────────────────────────────

static MARKS: RwLock<Option<Arc<MarksFrame>>> = RwLock::new(None);

fn marks_arrived(frame: MarksFrame) {
    crate::poi::adopt(frame.pois.clone(), frame.poi_current.clone());
    if let Ok(mut slot) = MARKS.write() {
        *slot = Some(Arc::new(frame));
    }
}

/// The tracks' own marks of the song on screen, as the engine last said.
#[must_use]
pub fn marks() -> Option<Arc<MarksFrame>> {
    MARKS.read().ok().and_then(|m| m.clone())
}

// ── The engine's jobs ────────────────────────────────────────────────

static JOBS: RwLock<Option<Arc<crate::view::JobsFrame>>> = RwLock::new(None);

fn jobs_arrived(frame: crate::view::JobsFrame) {
    if let Ok(mut slot) = JOBS.write() {
        *slot = Some(Arc::new(frame));
    }
}

/// What the engine is doing in the background, as it last said: none
/// with no engine linked.
#[must_use]
pub fn jobs() -> Vec<crate::view::JobRow> {
    JOBS.read()
        .ok()
        .and_then(|j| j.as_ref().map(|f| f.jobs.clone()))
        .unwrap_or_default()
}

// ── The performance meter ────────────────────────────────────────────

static PERF: RwLock<Option<Arc<crate::view::PerfFrame>>> = RwLock::new(None);

fn perf_arrived(frame: crate::view::PerfFrame) {
    if let Ok(mut slot) = PERF.write() {
        *slot = Some(Arc::new(frame));
    }
}

/// The engine's performance meter as it last said: none with no engine
/// linked, or before its first frame.
#[must_use]
pub fn perf() -> Option<Arc<crate::view::PerfFrame>> {
    PERF.read().ok().and_then(|p| p.clone())
}

// ── The live session ─────────────────────────────────────────────────

static LIVE: RwLock<Option<Arc<crate::view::LiveFrame>>> = RwLock::new(None);

/// The engine's live session arrived: kept for the collaboration control,
/// and handed to the app (who is in it, for what draws them).
fn live_arrived(frame: crate::view::LiveFrame) {
    crate::host::host().live_arrived(&frame);
    if let Ok(mut slot) = LIVE.write() {
        *slot = Some(Arc::new(frame));
    }
}

/// The engine's live session as it last said: none with no engine linked,
/// or before its first frame.
#[must_use]
pub fn live() -> Option<Arc<crate::view::LiveFrame>> {
    LIVE.read().ok().and_then(|l| l.clone())
}

// ── The edit cursor ──────────────────────────────────────────────────

/// An action put a song's edit cursor and time selection somewhere: the
/// window's frames take it up as they would one asked here.
fn edit_asked(asked: crate::view::EditAsked) {
    crate::edit_state::take_up(
        &asked.song,
        crate::edit_state::Edit {
            at: asked.at,
            selection: asked
                .selection
                .map(|s| crate::edit_state::Span::between(s.from, s.to)),
        },
    );
}

/// Tell the engine where the window shows the current song's edit cursor,
/// time selection and selection (every frame; only a change is sent, the
/// latest winning) — what its actions act on.
pub fn show_edit(edit: crate::edit_state::Edit, scope: Option<daw_edit::scope::Scope>) {
    let Some(song) = crate::open::current_song() else {
        return;
    };
    let shown = crate::view::EditShown {
        song,
        at: edit.at,
        selection: edit
            .selection
            .map(|s| daw_edit::scope::Span::between(s.start, s.end)),
        scope,
    };
    // The sender starts with its task, on the runtime it needs.
    let Some(rt) = runtime() else {
        return;
    };
    let tx = SHOWN_EDIT.get_or_init(|| {
        let (tx, rx) = watch::channel(None);
        rt.spawn(send_edits(rx));
        tx
    });
    tx.send_if_modified(|was| {
        if was.as_ref() == Some(&shown) {
            return false;
        }
        *was = Some(shown);
        true
    });
}

/// What [`show_edit`] last told the engine; its sender task holds the
/// other end.
static SHOWN_EDIT: std::sync::OnceLock<watch::Sender<Option<crate::view::EditShown>>> =
    std::sync::OnceLock::new();

/// The engine is not known to have what the window last showed: the next
/// [`show_edit`] sends it, the same or not.
fn forget_shown_edit() {
    if let Some(tx) = SHOWN_EDIT.get() {
        tx.send_if_modified(|was| {
            *was = None;
            false
        });
    }
}

/// Send what [`show_edit`] was told, the latest each time, while the
/// window runs.
async fn send_edits(mut rx: watch::Receiver<Option<crate::view::EditShown>>) {
    while rx.changed().await.is_ok() {
        let Some(shown) = rx.borrow_and_update().clone() else {
            continue;
        };
        // Not sent: the same place shown again is sent then.
        let Some(link) = link() else {
            forget_shown_edit();
            continue;
        };
        if tokio::time::timeout(CALL, link.view.show_edit(shown))
            .await
            .is_err()
        {
            forget_shown_edit();
            tracing::warn!(
                view.call = "show_edit",
                "view link: the engine did not answer in time"
            );
        }
    }
}

// ── What changed ─────────────────────────────────────────────────────

fn changed(change: crate::view::Changed) {
    use crate::view::Change;
    match change.what {
        Change::All | Change::Automation => crate::song::request_resync_here(),
        Change::Undone => {
            crate::actions::undone_there();
            crate::song::request_resync_here();
        }
        Change::Items => {
            crate::song::request_resync_items_here();
            crate::wave_detail::engine_changed();
        }
        Change::Waves => {
            crate::midi::mark_stale_here(change.items);
            crate::song::request_resync_items_here();
        }
        Change::Marks => {}
    }
}

// ── What the engine asks ─────────────────────────────────────────────

fn take_asked(link: &Link) {
    let view = link.view.clone();
    let Some(rt) = runtime() else { return };
    rt.spawn(async move {
        match tokio::time::timeout(CALL, view.take_window(crate::window_queue::window_name())).await
        {
            Ok(Ok(asked)) => crate::window_queue::receive(asked, |id, args, ticket| {
                run_asked(view.clone(), id.to_owned(), args.to_owned(), ticket);
            }),
            Ok(Err(e)) => {
                tracing::warn!(error = ?e, "view link: what the engine asked was not taken")
            }
            Err(_) => tracing::warn!("view link: taking what the engine asked timed out"),
        }
    });
}

/// Run the window action `id` the engine asked for, and answer `ticket`
/// (not 0) with what it came to.
fn run_asked(view: EngineViewClient, id: String, args: String, ticket: u64) {
    let Some(rt) = runtime() else { return };
    rt.spawn(async move {
        let list = crate::actions::list();
        // A sync action may block (`SESSION_FRAMES` waits for frames): off
        // the link's workers, which carry the engine's other asks and what
        // the window draws from.
        let ran = if list.is_async(&id) {
            list.run_json_async(&id, args.as_bytes()).await
        } else {
            let (id, args) = (id.clone(), args.clone());
            tokio::task::spawn_blocking(move || crate::actions::list().run_json(&id, args.as_bytes()))
                .await
                .unwrap_or_else(|e| {
                    Err(architect::action::ActionError::Failed {
                        id: String::new(),
                        message: format!("it panicked: {e}"),
                    })
                })
        };
        if let Err(e) = &ran {
            tracing::warn!(action.id = %id, error = %e, "view link: the engine's ask refused");
        }
        if ticket == 0 {
            return;
        }
        let answer = match ran {
            Ok(out) => crate::view::WindowAnswer {
                ticket,
                ok: true,
                json: String::from_utf8_lossy(&out).into_owned(),
            },
            Err(e) => crate::view::WindowAnswer {
                ticket,
                ok: false,
                json: e.to_string(),
            },
        };
        if let Err(e) = tokio::time::timeout(CALL, view.answer(answer)).await {
            tracing::warn!(action.id = %id, error = ?e, "view link: answering the engine's ask timed out");
        }
    });
}

// ── The playheads ────────────────────────────────────────────────────

/// Each song's last stamped position, and when it came.
static PLAYHEADS: Mutex<Option<HashMap<String, (daw_proto::StampedPosition, web_time::Instant)>>> =
    Mutex::new(None);

async fn follow_playheads(link: Link) {
    let mut positions = link.daw.transport_sync().positions();
    while let Ok(Some(frame)) = positions.recv().await {
        if !current(&link) {
            return;
        }
        let at = frame.get();
        if let Ok(mut map) = PLAYHEADS.lock() {
            map.get_or_insert_with(HashMap::new).insert(
                at.project_guid.clone(),
                (at.clone(), web_time::Instant::now()),
            );
        }
    }
}

/// Where `song`'s playhead is now, seconds, and whether it plays — from its
/// last stamped position, carried on to now at its rate while it plays.
/// `None` before the engine has said.
#[must_use]
pub fn playhead_of(song: &str) -> Option<(f64, bool)> {
    let map = PLAYHEADS.lock().ok()?;
    let (stamp, when) = map.as_ref()?.get(song)?;
    Some(extrapolate(stamp, when.elapsed().as_secs_f64()))
}

/// A stamped position `since` seconds after it was heard: on, at its rate,
/// while it plays — never further than a quarter second (a stalled stream
/// does not run the playhead away).
#[must_use]
pub fn extrapolate(stamp: &daw_proto::StampedPosition, since: f64) -> (f64, bool) {
    if !stamp.is_playing {
        return (stamp.playhead_seconds, false);
    }
    let ahead = since.clamp(0.0, 0.25) * stamp.playrate;
    (stamp.playhead_seconds + ahead, true)
}

// ── Calls ────────────────────────────────────────────────────────────

/// Run `call` on the link, from sync code, bounded by `within`; `None`
/// unlinked or when the engine did not answer in time.
fn block<T: Send, F>(
    what: &str,
    within: Duration,
    call: impl FnOnce(EngineViewClient) -> F,
) -> Option<T>
where
    F: std::future::Future<Output = T> + Send,
{
    let link = link()?;
    let rt = runtime()?;
    let asked = call(link.view.clone());
    // Under the caller's span — a key's, an action's — wherever it is
    // polled: a scoped thread below has no span of its own, and the call's
    // trace context comes from the span it is made in.
    let bounded = tracing::Instrument::instrument(
        async move { tokio::time::timeout(within, asked).await.ok() },
        tracing::info_span!("view.call", view.call = what),
    );
    // Blocking on the runtime from inside one panics: a thread of its own.
    let answered = if tokio::runtime::Handle::try_current().is_err() {
        rt.block_on(bounded)
    } else {
        std::thread::scope(|scope| scope.spawn(|| rt.block_on(bounded)).join().ok().flatten())
    };
    if answered.is_none() {
        tracing::warn!(
            view.call = what,
            "view link: the engine did not answer in time"
        );
    }
    answered
}

/// Run Action List action `id` with `args` (JSON) on the engine.
///
/// # Errors
///
/// Not linked, the engine did not answer, or the action refused.
pub fn run(id: &str, args: &[u8]) -> Result<String, String> {
    let call = ActionCall {
        id: id.to_owned(),
        args: String::from_utf8_lossy(args).into_owned(),
    };
    if !is_linked() {
        return Err(format!("{id}: no engine to run it on"));
    }
    block("run", RUN, |view| async move { view.run(call).await })
        .ok_or_else(|| format!("{id}: the engine did not answer in time"))?
        .map_err(|e| match e {
            vox::VoxError::User(e) => e.to_string(),
            other => format!("{id}: {other:?}"),
        })
}

/// Say on the engine's Action List that the window's own action `id` ran
/// here (a key, a click), so the engine's `watch` sees it: sent behind the
/// window, nothing waited for. Not linked, nothing to say it to.
pub fn ran_in_window(id: &str, args: &[u8]) {
    let (Some(link), Some(rt)) = (link(), runtime()) else {
        return;
    };
    let call = ActionCall {
        id: id.to_owned(),
        args: String::from_utf8_lossy(args).into_owned(),
    };
    let view = link.view.clone();
    rt.spawn(async move {
        if tokio::time::timeout(CALL, view.ran_in_window(call))
            .await
            .is_err()
        {
            tracing::warn!("view link: saying a window action ran timed out");
        }
    });
}

/// The window's edits, carried out by the engine in order.
pub async fn apply(song: String, edits: Vec<crate::engine::Edit>) -> Option<Applied> {
    let link = link()?;
    match tokio::time::timeout(CALL, link.view.apply(EditBatch { song, edits })).await {
        Ok(Ok(applied)) => Some(applied),
        Ok(Err(e)) => {
            tracing::warn!(error = ?e, "view link: the engine refused the window's edits");
            None
        }
        Err(_) => {
            tracing::warn!("view link: the engine did not apply the window's edits in time");
            None
        }
    }
}

/// `items` of `song`'s waveforms, as the engine draws them.
#[must_use]
pub fn waves(song: &str, items: Vec<String>) -> Option<Vec<WaveRead>> {
    let ask = WavesAsk {
        song: song.to_owned(),
        items,
    };
    block(
        "waves",
        CALL,
        |view| async move { view.waves(ask).await.ok() },
    )
    .flatten()
}

/// `items` of `song`'s waveforms, as the engine draws them, awaited.
pub async fn waves_of(song: &str, items: Vec<String>) -> Option<Vec<WaveRead>> {
    let link = link()?;
    let ask = WavesAsk {
        song: song.to_owned(),
        items,
    };
    match tokio::time::timeout(CALL, link.view.waves(ask)).await {
        Ok(read) => read.ok(),
        Err(_) => {
            tracing::warn!(
                view.call = "waves",
                "view link: the engine did not answer in time"
            );
            None
        }
    }
}

/// One stretch's detail, from the engine.
#[must_use]
pub fn detail(ask: DetailAsk) -> Option<crate::wave_detail::Detail> {
    block(
        "detail",
        CALL,
        |view| async move { view.detail(ask).await.ok() },
    )
    .flatten()
    .flatten()
}

/// `song`'s automation, from the engine.
#[must_use]
pub fn automation(song: &str) -> Option<AutomationRead> {
    let song = song.to_owned();
    block("automation", CALL, |view| async move {
        view.automation(song).await.ok()
    })
    .flatten()
}

/// `song`'s automation, awaited.
pub async fn automation_of(song: &str) -> Option<AutomationRead> {
    let link = link()?;
    tokio::time::timeout(CALL, link.view.automation(song.to_owned()))
        .await
        .ok()?
        .ok()
}

/// The items of `song` changed since `since`.
#[must_use]
pub fn item_changes(song: &str, since: u64) -> Option<ItemChanges> {
    let ask = ChangesAsk {
        song: song.to_owned(),
        since,
    };
    block("item_changes", CALL, |view| async move {
        view.item_changes(ask).await.ok()
    })
    .flatten()
}

/// Where the engine's journal stands.
#[must_use]
pub fn journal_now() -> Option<u64> {
    block("journal_now", CALL, |view| async move {
        view.journal_now().await.ok()
    })
    .flatten()
}

/// What the window needs of `song` beside the facade, awaited.
pub async fn song_facts(song: &str) -> Option<SongFacts> {
    let link = link()?;
    tokio::time::timeout(CALL, link.view.song_facts(song.to_owned()))
        .await
        .ok()?
        .ok()
}

/// Every song's playhead, asked of the engine.
#[must_use]
pub fn playheads() -> Option<Vec<crate::view::Playhead>> {
    block("playheads", CALL, |view| async move {
        view.playheads().await.ok()
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(at: f64, playing: bool) -> daw_proto::StampedPosition {
        daw_proto::StampedPosition {
            project_guid: "song".to_owned(),
            host_micros: 0.0,
            playhead_seconds: at,
            playrate: 1.0,
            is_playing: playing,
            sequence: 0,
            sample_rate: 48_000.0,
        }
    }

    #[test]
    fn a_playing_position_runs_on_at_its_rate_but_not_far() {
        assert_eq!(extrapolate(&stamp(10.0, true), 0.1), (10.1, true));
        assert_eq!(extrapolate(&stamp(10.0, true), 5.0), (10.25, true));
        assert_eq!(extrapolate(&stamp(10.0, false), 5.0), (10.0, false));
    }
}

// ── A take's samples, over the link ──────────────────────────────────

/// The engine's audio accessors, reached over the window's link: what
/// reads a take's samples for the window's own analysis (the expression
/// editor's audio takes) as it would read them from an engine in its
/// process. Each call waits on the engine — use it off the window's
/// thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinkedAudio;

impl LinkedAudio {
    fn client() -> Option<daw_proto::AudioAccessorsClient> {
        let daw = daw::rpc::Daw::try_get()?;
        Some(
            daw_proto::AudioAccessorsClient::new(daw.caller().clone())
                .with_middleware(architect::trace_context::TraceContextMiddleware),
        )
    }

    fn ask<T: Send, F>(
        what: &str,
        call: impl FnOnce(daw_proto::AudioAccessorsClient) -> F,
    ) -> Option<T>
    where
        F: std::future::Future<Output = Option<T>> + Send,
    {
        let client = Self::client()?;
        let rt = runtime()?;
        let asked = call(client);
        let bounded = async move { tokio::time::timeout(CALL, asked).await.ok().flatten() };
        let answered = if tokio::runtime::Handle::try_current().is_err() {
            rt.block_on(bounded)
        } else {
            std::thread::scope(|scope| scope.spawn(|| rt.block_on(bounded)).join().ok().flatten())
        };
        if answered.is_none() {
            tracing::debug!(audio.call = what, "linked audio: no answer");
        }
        answered
    }
}

impl daw_proto::AudioAccessors for LinkedAudio {
    fn create_track_accessor(
        &self,
        project: daw_proto::ProjectContext,
        track: daw_proto::TrackRef,
    ) -> Option<String> {
        Self::ask("create_track_accessor", |c| async move {
            c.create_track_accessor(project, track).await.ok().flatten()
        })
    }

    fn create_take_accessor(
        &self,
        project: daw_proto::ProjectContext,
        item: daw_proto::ItemRef,
        take: daw_proto::TakeRef,
    ) -> Option<String> {
        Self::ask("create_take_accessor", |c| async move {
            c.create_take_accessor(project, item, take)
                .await
                .ok()
                .flatten()
        })
    }

    fn has_state_changed(&self, accessor_id: &str) -> bool {
        let id = accessor_id.to_owned();
        Self::ask("has_state_changed", |c| async move {
            c.has_state_changed(id).await.ok()
        })
        .unwrap_or(false)
    }

    fn get_samples(&self, request: daw_proto::GetSamplesRequest) -> daw_proto::AudioSampleData {
        Self::ask("get_samples", |c| async move {
            c.get_samples(request).await.ok()
        })
        .unwrap_or_default()
    }

    fn destroy_accessor(&self, accessor_id: &str) {
        let id = accessor_id.to_owned();
        let _ = Self::ask("destroy_accessor", |c| async move {
            c.destroy_accessor(id).await.ok()
        });
    }
}
