//! What a window's modules need of the app they run in.
//!
//! This crate is the contract between a DAW's views and its engine; the
//! app (Studio) is the one that knows which song is on screen, whether it
//! runs the engine itself, how its set and its live session are shown,
//! which folders start shut, and what its editing is dialled to. The few
//! places a module here needs one of those go through [`Host`], which the
//! app installs as it comes up ([`install`]), the same way the engine
//! installs its body ([`crate::body`]). With none installed, every answer
//! is what a window with no app behind it would say: no song, nothing
//! shut, nothing streaming.

use std::sync::OnceLock;

use daw_proto::Track;

/// The app behind a window. Every method answers, by default, as a window
/// with no app would.
pub trait Host: Send + Sync {
    /// The song on screen (an engine project's guid), when there is one.
    fn current_song(&self) -> Option<String> {
        None
    }

    /// The engine in this process, when the app runs one and owns the song
    /// on screen.
    #[cfg(feature = "local")]
    fn local_engine(&self) -> Option<&'static daw::standalone::Standalone> {
        None
    }

    /// The engine's set, as it changes: the app keeps its replica (its
    /// setlist, the songs read back).
    fn set_arrived(&self, _frame: crate::view::SetFrame) {}

    /// What streams in, as it changes.
    fn streams_arrived(&self, _frame: crate::view::StreamsFrame) {}

    /// The engine's live session, as it changes (who is in it).
    fn live_arrived(&self, _frame: &crate::view::LiveFrame) {}

    /// The streaming state: its version, and each item still arriving
    /// with how much has.
    fn streaming(&self) -> (u64, std::collections::HashMap<String, f32>) {
        (0, std::collections::HashMap::new())
    }

    /// How far behind the engine the device plays, seconds (a page's
    /// audio worklet renders ahead).
    fn output_latency(&self) -> f64 {
        0.0
    }

    /// Whether a folder starts shut until it is opened.
    fn shut_by_default(&self, _folder: &Track) -> bool {
        false
    }

    /// Whether a folder's own folders start shut (each a part of it).
    fn shuts_its_folders(&self, _name: &str) -> bool {
        false
    }

    /// The grid an edit quantizes to where nothing more specific says,
    /// as a division of a whole note.
    fn edit_grid(&self) -> u32 {
        8
    }
}

/// A window with no app behind it.
struct Absent;

impl Host for Absent {}

static HOST: OnceLock<&'static dyn Host> = OnceLock::new();

/// Install the app: once, as it comes up. A second install keeps the
/// first.
pub fn install(host: &'static dyn Host) {
    let _ = HOST.set(host);
}

/// Whether an app installed itself in this process.
#[must_use]
pub fn installed() -> bool {
    HOST.get().is_some()
}

/// The app, or a window's with none.
#[must_use]
pub fn host() -> &'static dyn Host {
    HOST.get().copied().unwrap_or(&Absent)
}
