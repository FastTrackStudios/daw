//! The window's half of its engine's Action List: the list a window's
//! keys, menus and palette run ([`list`]), how a press runs where the
//! engine is ([`run`]: over the link when the window is linked to one),
//! and the transport's, undo's and save's action ids.
//!
//! The engine's actions are registered into the same list by the engine,
//! when it runs in this process (through [`fill_with`]); a window linked to
//! an engine in another process has only its own, and runs the engine's
//! over the link. What an app asks of its own windows (a mode, a dock) is
//! the app's.

#[cfg(feature = "native")]
use std::sync::{Once, OnceLock};

#[cfg(feature = "native")]
use architect::action::ActionList;
use architect::action::ActionMeta;

#[cfg(feature = "native")]
static LIST: OnceLock<ActionList> = OnceLock::new();

/// What fills the list with the engine's actions, when the engine runs in
/// this process ([`fill_with`]).
#[cfg(feature = "native")]
static FILL: OnceLock<fn(&ActionList)> = OnceLock::new();

#[cfg(feature = "native")]
static FILLED: Once = Once::new();

/// The Action List: the window's actions, and the engine's when the engine
/// runs in this process.
#[cfg(feature = "native")]
pub fn list() -> &'static ActionList {
    let list = LIST.get_or_init(ActionList::new);
    if let Some(fill) = FILL.get() {
        FILLED.call_once(|| fill(list));
    }
    list
}

/// Fill the list with the engine's actions (once): what the engine does
/// when it runs in this process.
#[cfg(feature = "native")]
pub fn fill_with(fill: fn(&ActionList)) {
    let _ = FILL.set(fill);
}

/// An action's description, as the list and its menus show it.
pub const fn meta(
    id: &'static str,
    method_name: &'static str,
    display_name: &'static str,
    description: &'static str,
    group: &'static str,
    shortcut: &'static str,
) -> ActionMeta {
    ActionMeta {
        id,
        trait_name: "Session",
        method_name,
        display_name,
        description,
        category: "Session",
        group,
        shortcut,
        toggleable: false,
        undo: false,
    }
}

pub static TRANSPORT_RECORD: architect::action::ActionMeta = crate::actions::meta(
    "TRANSPORT_RECORD",
    "transport_record",
    "Record",
    "Start recording the armed tracks from where the transport is, or stop: `on` true or false, toggled when left out. Stopping puts each armed track's recording on it as a new take, on the lowest take lane free over what was recorded (else a new lane), the newest take playing unless a comp holds something there. Says whether it records now and, on stop, the takes it placed.",
    "Transport",
    "",
);

pub static PLAY_STOP: ActionMeta = meta(
    "SESSION_PLAY_STOP",
    "play_stop",
    "Play/stop",
    "Start playback, or stop it.",
    "Transport",
    "Space",
);
pub static PLAY_PAUSE: ActionMeta = meta(
    "SESSION_PLAY_PAUSE",
    "play_pause",
    "Play/pause",
    "Start playback, or pause it where it is.",
    "Transport",
    "",
);
pub static GO_TO_START: ActionMeta = meta(
    "SESSION_GO_TO_START",
    "go_to_start",
    "Go to start",
    "The edit cursor to the start of the song.",
    "Transport",
    "Home",
);
pub static STOP: ActionMeta = meta(
    "SESSION_STOP",
    "stop",
    "Stop",
    "Stop the transport where it is (SESSION_PLAY_STOP toggles).",
    "Transport",
    "",
);
pub static GO_TO_END: ActionMeta = meta(
    "SESSION_GO_TO_END",
    "go_to_end",
    "Go to end",
    "The edit cursor to the end of the song.",
    "Transport",
    "End",
);
pub static LOOP: ActionMeta = meta(
    "SESSION_LOOP",
    "loop",
    "Loop",
    "Turn looping over the loop range on or off.",
    "Transport",
    "",
);
pub static SEEK: ActionMeta = meta(
    "SESSION_SEEK",
    "seek",
    "Go to a time",
    "Move the play position to `at` seconds from the start of the song — what a click on a section in the navigator, the progress bar or the ruler does. Playing, it plays on from there.",
    "Transport",
    "",
);
pub static PLAY_FROM: ActionMeta = meta(
    "SESSION_PLAY_FROM",
    "play_from",
    "Play from a time",
    "Move to `at` seconds and play from there, as one step (the play cannot land before the move).",
    "Transport",
    "",
);

/// The action a press of the transport runs (`crate::engine::transport`):
/// its id and its arguments, `at` seconds where it takes a time.
#[must_use]
pub fn transport_action(command: crate::engine::Move, at: f64) -> (&'static str, String) {
    use crate::engine::Move as M;
    let at = || format!(r#"{{"at":{at}}}"#);
    match command {
        M::PlayStop => (PLAY_STOP.id, String::new()),
        M::Home => (GO_TO_START.id, String::new()),
        M::Stop => (STOP.id, String::new()),
        M::End => (GO_TO_END.id, String::new()),
        M::ToggleLoop => (LOOP.id, String::new()),
        M::ToggleRecord => (TRANSPORT_RECORD.id, "{}".to_owned()),
        M::Seek => (SEEK.id, at()),
        M::PlayFrom => (PLAY_FROM.id, at()),
    }
}

#[cfg(feature = "native")]
pub static SAVE: ActionMeta = meta(
    "SESSION_SAVE",
    "session_save",
    "Save the song",
    "Save the song as it is now — its edits, its tracks, every FX with what it holds (a sampler's kit, a rig's capture, a reverb's time) — into its project file: `Song.session` beside the `.RPP` it was opened from (what opens next time), or the alternative it is. What changed since the last save goes in as one step of the file's history. `to` saves to another file instead and leaves the song's own alone. Says where.",
    "Session",
    "",
);

#[cfg(feature = "native")]
pub static UNDO: ActionMeta = meta(
    "SESSION_UNDO",
    "session_undo",
    "Undo",
    "Take back the last step — a move, a cut, a slip edit, whatever one gesture or action did, as one. Says what it undid, or that there is nothing to.",
    "Session",
    "",
);

#[cfg(feature = "native")]
pub static REDO: ActionMeta = meta(
    "SESSION_REDO",
    "session_redo",
    "Redo",
    "Do again the last step undone. Says what it redid, or that there is nothing to.",
    "Session",
    "",
);

/// Session undos and redos run, ever (one that found nothing counts): what
/// a view holding a copy of the song (the Editor's take) watches to read
/// it again.
static UNDONE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many Session undos and redos have been done.
#[must_use]
pub fn undone() -> u64 {
    UNDONE.load(std::sync::atomic::Ordering::Acquire)
}

/// The engine said it undid or redid a step: counted here, as one done
/// here is ([`undone`]).
pub fn undone_there() {
    UNDONE.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Run `SESSION_UNDO` (`back`) or `SESSION_REDO` behind the window, as
/// the keys do: for a view whose own Undo is Session's.
#[cfg(feature = "native")]
pub fn undo_behind(back: bool) {
    off_the_window(if back { UNDO.id } else { REDO.id }, String::new());
}

/// Whether this process's windows are linked into the app that hosts them
/// ([`in_host`]).
#[cfg(feature = "native")]
static IN_HOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Say the windows here run inside their host app's own process, on its
/// thread: an async window action is then started, never waited for.
#[cfg(feature = "native")]
pub fn in_host(on: bool) {
    IN_HOST.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether [`in_host`] was said.
#[cfg(feature = "native")]
#[must_use]
pub fn is_in_host() -> bool {
    IN_HOST.load(std::sync::atomic::Ordering::Relaxed)
}

/// Run `id` with `args` (JSON) where the engine is: this process's list,
/// or — a window driving an engine in another process — that engine's
/// (over its link, `crate::view_client`). A window's own actions always run here.
///
/// # Errors
///
/// The action refused, or the engine did not answer.
#[cfg(feature = "native")]
pub fn run(id: &str, args: &[u8]) -> Result<Vec<u8>, String> {
    // A window's: every action but its own runs on its engine, over its
    // link — in another process or this one alike.
    if crate::view_client::is_linked() && !crate::window_queue::is_window_action(id) {
        return crate::view_client::run(id, args).map(String::into_bytes);
    }
    let ran = match list().run_json(id, args) {
        // An async action (the arrangement's gestures await the engine):
        // awaited here, off any runtime — a window with no link (driving
        // a REAPER) runs the engine's actions itself.
        Err(architect::action::ActionError::Failed { message, .. })
            if message.contains("run_json_async") =>
        {
            // Linked into the app it is a window of (Studio's distribution):
            // this thread is the app's, and the action asks that app
            // (`WORKSPACE_…`) — waiting here would wait on itself. Started,
            // not waited for.
            if IN_HOST.load(std::sync::atomic::Ordering::Relaxed) {
                let Some(rt) = crate::open::runtime() else {
                    return Err("no runtime to run it on".to_owned());
                };
                let (id, args) = (id.to_owned(), args.to_vec());
                rt.spawn(async move {
                    if let Err(error) = list().run_json_async(&id, &args).await {
                        tracing::warn!(action = %id, %error, "a window action failed");
                    }
                });
                return Ok(b"null".to_vec());
            }
            let block = || {
                crate::open::runtime()
                    .ok_or_else(|| "no runtime to run it on".to_owned())
                    .and_then(|rt| {
                        rt.block_on(list().run_json_async(id, args))
                            .map_err(|e| e.to_string())
                    })
            };
            // On a thread driving a runtime already (an Architect process
            // view's paint, where a control presses a window action the
            // view claims): on a thread of its own, waited for.
            if tokio::runtime::Handle::try_current().is_ok() {
                std::thread::scope(|s| {
                    s.spawn(block)
                        .join()
                        .unwrap_or_else(|_| Err("the action's thread panicked".to_owned()))
                })
            } else {
                block()
            }
        }
        other => other.map_err(|e| e.to_string()),
    };
    // The window's own, run here: said on the engine's list too, where
    // whoever watches the engine looks.
    if ran.is_ok() && crate::view_client::is_linked() {
        crate::view_client::ran_in_window(id, args);
    }
    ran
}

/// Run a key's action on the one thread that runs them behind the window,
/// in the order they were pressed; the window redraws when each is done,
/// as it does for a model's.
#[cfg(feature = "native")]
pub fn off_the_window(id: &'static str, args: String) {
    type Job = (&'static str, String, tracing::Span);
    static WORKER: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Job>>> =
        std::sync::OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let _ = std::thread::Builder::new()
            .name("key-actions".into())
            .spawn(move || {
                // Each under the span it was asked in (the key's): its
                // trace runs from the key to the engine.
                for (id, args, asked_in) in rx {
                    let _in = asked_in.entered();
                    if let Err(e) = run(id, args.as_bytes()) {
                        tracing::warn!(action.id = id, error = %e, "key action refused");
                    }
                }
            });
        std::sync::Mutex::new(tx)
    });
    if let Ok(tx) = worker.lock() {
        let _ = tx.send((id, args, tracing::Span::current()));
    }
}
