//! The edit cursor and the time selection: engine state, one per song.
//!
//! What an edit acts on when it is not told where — an insert at the edit
//! cursor, a quantize over the time selection — so it is the engine's, not
//! a window's: an action run headless (the CLI, a model) reads the same
//! cursor a window shows, and two windows on one engine agree on it. Item
//! and track selection are the project's already (`daw::service::Items` /
//! `Tracks` select); this is the rest of "where the user is".
//!
//! Two writers, told apart:
//!
//! - **a window** [`show`]s where its arrangement has the cursor as it
//!   draws (a click, a drag) — the state follows, nobody is asked to move;
//! - **an action** [`ask`]s for it (`TIME_SELECTION_SET`, an insert that
//!   moves the cursor on): the state moves, and every window takes it up on
//!   its next frame ([`asked_since`] / [`take_asked`]).
//!
//! The pure part is [`EditStates`]; the process's one is behind the free
//! functions, keyed by the song on screen (`crate::open::current_song`).

use std::collections::HashMap;
use std::sync::Mutex;

pub use daw_timeline::cursor::{EditCursor as Edit, TimeSpan as Span};

/// Where the cursor and selection are, song by song, and which of those
/// positions were asked for (rather than shown).
#[derive(Debug, Default)]
pub struct EditStates {
    songs: HashMap<String, Kept>,
    /// Bumped by every change, shown or asked.
    revision: u64,
}

/// One song's edit state.
#[derive(Clone, Copy, Debug, Default)]
struct Kept {
    edit: Edit,
    /// The revision of the last [`EditStates::ask`] on this song; 0 never.
    asked: u64,
}

/// Whether two edit states put the cursor and selection in the same place.
#[must_use]
pub fn same(a: &Edit, b: &Edit) -> bool {
    a.at.to_bits() == b.at.to_bits() && a.selection == b.selection
}

impl EditStates {
    /// `song`'s edit state, if anything has put one there.
    #[must_use]
    pub fn of(&self, song: &str) -> Option<Edit> {
        self.songs.get(song).map(|kept| kept.edit)
    }

    /// The revision of the latest change.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// A window shows `edit` on `song`. `true` when that moved it.
    pub fn show(&mut self, song: &str, edit: Edit) -> bool {
        let kept = self.songs.entry(song.to_owned()).or_default();
        if same(&kept.edit, &edit) && self.revision > 0 {
            return false;
        }
        kept.edit = edit;
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// Something other than the window asks for `edit` on `song`: every
    /// window is to take it up. Its revision.
    pub fn ask(&mut self, song: &str, edit: Edit) -> u64 {
        self.revision = self.revision.saturating_add(1);
        let revision = self.revision;
        let kept = self.songs.entry(song.to_owned()).or_default();
        kept.edit = edit;
        kept.asked = revision;
        revision
    }

    /// What was asked for on `song` after revision `seen`, with its
    /// revision — what a window that last took up `seen` takes up now.
    #[must_use]
    pub fn asked_since(&self, song: &str, seen: u64) -> Option<(u64, Edit)> {
        self.songs
            .get(song)
            .filter(|kept| kept.asked > seen)
            .map(|kept| (kept.asked, kept.edit))
    }

    /// `song` is gone: its state with it.
    pub fn forget(&mut self, song: &str) {
        if self.songs.remove(song).is_some() {
            self.revision = self.revision.saturating_add(1);
        }
    }
}

/// The process's edit states (one engine per process).
static STATES: Mutex<Option<EditStates>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut EditStates) -> R) -> R {
    let mut slot = STATES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(slot.get_or_insert_with(EditStates::default))
}

fn song_now() -> Option<String> {
    crate::open::current_song()
}

/// The current song's edit cursor and time selection, if a window or an
/// action has put them anywhere yet.
#[must_use]
pub fn current() -> Option<Edit> {
    let song = song_now()?;
    with(|states| states.of(&song))
}

/// `song`'s edit cursor and time selection.
#[must_use]
pub fn of(song: &str) -> Option<Edit> {
    with(|states| states.of(song))
}

/// The revision of the latest change to any song's edit state.
#[must_use]
pub fn revision() -> u64 {
    with(|states| states.revision())
}

/// A window shows `edit` on the current song (as it draws).
pub fn show(edit: Edit) {
    if let Some(song) = song_now() {
        with(|states| states.show(&song, edit));
    }
}

/// A window shows `edit` on `song`: what the engine hears from a window in
/// another process.
pub fn show_on(song: &str, edit: Edit) {
    with(|states| states.show(song, edit));
}

/// Put the current song's edit cursor and time selection at `edit`: what
/// an action that sets them asks. It is [`current`] at once; every window
/// takes it up on its next frame — one in another process told by
/// [`on_ask`].
pub fn ask(edit: Edit) {
    if let Some(song) = song_now() {
        let revision = with(|states| states.ask(&song, edit));
        if let Some(told) = TOLD.get() {
            told(&song, edit, revision);
        }
    }
}

/// What the engine was asked, taken up by a window in this process from
/// its engine's word — its own frames take it up as they would an ask
/// made here, and the engine is not told again.
pub fn take_up(song: &str, edit: Edit) {
    with(|states| states.ask(song, edit));
}

/// Tell `told` of every ask (its song, where, its revision): how the engine
/// passes them to its windows.
pub fn on_ask(told: impl Fn(&str, Edit, u64) + Send + Sync + 'static) {
    let _ = TOLD.set(Box::new(told));
}

type Told = Box<dyn Fn(&str, Edit, u64) + Send + Sync>;
static TOLD: std::sync::OnceLock<Told> = std::sync::OnceLock::new();

/// What was asked for on the current song after revision `seen`.
#[must_use]
pub fn asked_since(seen: u64) -> Option<(u64, Edit)> {
    let song = song_now()?;
    with(|states| states.asked_since(&song, seen))
}

/// What was asked for on the current song since this process's window
/// last took it up, once — for a process with one arrangement.
#[must_use]
pub fn take_asked() -> Option<Edit> {
    static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seen = SEEN.load(std::sync::atomic::Ordering::Acquire);
    let (revision, edit) = asked_since(seen)?;
    SEEN.store(revision, std::sync::atomic::Ordering::Release);
    Some(edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: f64) -> Edit {
        Edit {
            at: t,
            selection: None,
        }
    }

    #[test]
    fn a_shown_cursor_is_kept_but_never_asked_for() {
        let mut states = EditStates::default();
        assert!(states.show("a", at(1.0)));
        assert!(!states.show("a", at(1.0)), "the same place is no change");
        assert_eq!(states.of("a").map(|e| e.at), Some(1.0));
        assert!(states.asked_since("a", 0).is_none());
    }

    #[test]
    fn an_asked_cursor_reaches_each_window_once() {
        let mut states = EditStates::default();
        states.show("a", at(1.0));
        let asked = states.ask("a", at(4.0));
        let (seen, edit) = states.asked_since("a", 0).unwrap_or((0, at(-1.0)));
        assert_eq!((seen, edit.at), (asked, 4.0));
        assert!(states.asked_since("a", seen).is_none(), "taken up already");
        // A window that has not looked yet still gets it.
        assert!(states.asked_since("a", 0).is_some());
    }

    #[test]
    fn songs_keep_their_own_cursor() {
        let mut states = EditStates::default();
        states.ask("a", at(2.0));
        states.show("b", at(9.0));
        assert_eq!(states.of("a").map(|e| e.at), Some(2.0));
        assert_eq!(states.of("b").map(|e| e.at), Some(9.0));
        assert!(states.asked_since("b", 0).is_none());
        states.forget("a");
        assert!(states.of("a").is_none());
    }

    #[test]
    fn a_selection_is_part_of_the_state() {
        let mut states = EditStates::default();
        states.show("a", at(1.0));
        let selected = Edit {
            at: 1.0,
            selection: Some(Span::between(1.0, 3.0)),
        };
        assert!(states.show("a", selected), "selecting is a change");
        assert_eq!(
            states.of("a").and_then(|e| e.selection),
            Some(Span::between(1.0, 3.0))
        );
    }
}
