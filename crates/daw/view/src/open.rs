//! Where a window's work on its engine runs, and which song it is on.
//!
//! - **The runtime** a window's link and its engine calls run on
//!   ([`engine_runtime`], [`runtime`]), and the ways sync code waits on it
//!   ([`block_on_engine`], [`facade_blocking`], [`traced_blocking`]). A
//!   page has none: its engine runs in the page's own loop.
//! - **The song on screen** ([`current_song`]): the one a call is scoped
//!   to ([`in_song`]), else the app's ([`crate::host::Host::current_song`]).
//! - **The engine in this process** (`local`): the app's, when it runs one
//!   and owns the song on screen ([`local_engine`]).
//!
//! How a window opens songs, attaches to an engine and chooses its audio
//! is the app's own.

#[cfg(feature = "native")]
mod native {
    use std::sync::OnceLock;

    /// The engine runtime: made once, leaked.
    static RUNTIME: OnceLock<&'static tokio::runtime::Runtime> = OnceLock::new();

    /// Build the engine runtime once, or hand back the one there is.
    ///
    /// Leaked, multi-threaded, with 16 MiB worker stacks — vox's
    /// debug-build channel encode recurses deeply enough to overflow
    /// tokio's default 2 MiB, and it presents as a stack overflow
    /// somewhere entirely unrelated the first time a panel subscribes to
    /// anything.
    ///
    /// # Errors
    ///
    /// The runtime could not be built.
    pub fn engine_runtime() -> eyre::Result<&'static tokio::runtime::Runtime> {
        if let Some(rt) = RUNTIME.get() {
            return Ok(rt);
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("session-engine")
            .thread_stack_size(16 * 1024 * 1024)
            .enable_all()
            .build()?;
        let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));
        let _ = RUNTIME.set(rt);
        Ok(RUNTIME.get().copied().unwrap_or(rt))
    }

    /// The engine's runtime, for work that must not run on the event
    /// loop. `None` until one has been built: a window that is still
    /// loading has nothing to edit.
    #[must_use]
    pub fn runtime() -> Option<&'static tokio::runtime::Runtime> {
        RUNTIME.get().copied()
    }

    /// `work` off the runtime's workers under the span it was asked in: a
    /// key's or an action's edit traced from the window to the engine,
    /// though it runs on a thread of its own.
    pub fn traced_blocking<F, R>(
        runtime: &tokio::runtime::Runtime,
        work: F,
    ) -> tokio::task::JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let span = tracing::Span::current();
        runtime.spawn_blocking(move || span.in_scope(work))
    }

    /// Run `future` to completion on the engine runtime from sync code —
    /// on a scoped thread of its own when the caller is already inside a
    /// runtime, where blocking on another would panic. `None` before there
    /// is one.
    pub fn block_on_engine<T: Send>(
        future: impl std::future::Future<Output = T> + Send,
    ) -> Option<T> {
        let rt = runtime()?;
        if tokio::runtime::Handle::try_current().is_err() {
            return Some(rt.block_on(future));
        }
        std::thread::scope(|scope| scope.spawn(|| rt.block_on(future)).join().ok())
    }

    /// Run one facade call to completion from a sync context (the UI
    /// thread, a worker), on the engine runtime, bounded so a remote that
    /// has gone quiet cannot hang the caller.
    ///
    /// # Errors
    ///
    /// The facade is not up, the runtime is not, or the call failed or
    /// took longer than three seconds.
    pub fn facade_blocking<F, Fut, T>(what: &str, f: F) -> eyre::Result<T>
    where
        F: FnOnce(&'static daw::rpc::Daw) -> Fut,
        Fut: std::future::Future<Output = Result<T, daw::rpc::Error>> + Send,
        T: Send,
    {
        let daw = daw::rpc::Daw::try_get()
            .ok_or_else(|| eyre::eyre!("{what}: the daw facade is not up"))?;
        let call = f(daw);
        block_on_engine(async {
            tokio::time::timeout(std::time::Duration::from_secs(3), call)
                .await
                .map_err(|_| eyre::eyre!("{what}: the remote did not answer"))?
                .map_err(|e| eyre::eyre!("{what}: {e}"))
        })
        .ok_or_else(|| eyre::eyre!("{what}: the engine runtime is not up"))?
    }
}

#[cfg(feature = "native")]
pub use native::{block_on_engine, engine_runtime, facade_blocking, runtime, traced_blocking};

/// A page's stand-in for a runtime: never constructed, [`runtime`] has
/// none to give.
#[cfg(not(feature = "native"))]
pub struct Runtime(());

#[cfg(not(feature = "native"))]
impl Runtime {
    pub fn block_on<F: core::future::Future>(&self, _future: F) -> F::Output {
        unreachable!("no runtime is ever handed out on the web")
    }
}

/// A page has no engine runtime: its engine runs in the page's own loop.
#[cfg(not(feature = "native"))]
#[must_use]
pub fn runtime() -> Option<&'static Runtime> {
    None
}

thread_local! {
    /// The song a call on this thread is scoped to ([`in_song`]).
    static IN_SONG: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The song on screen: the one this thread's call is scoped to
/// ([`in_song`]), else the app's.
#[must_use]
pub fn current_song() -> Option<String> {
    IN_SONG
        .with(|s| s.borrow().clone())
        .or_else(|| crate::host::host().current_song())
}

/// Runs `f` with song `project` (an engine project's guid) as the current
/// song for everything it calls on this thread: an action that works on
/// "the open song" then works on that one, the window staying where it
/// is. How a script edits every song of a set in one go.
pub fn in_song<R>(project: &str, f: impl FnOnce() -> R) -> R {
    let before = IN_SONG.with(|s| s.replace(Some(project.to_owned())));
    let out = f();
    IN_SONG.with(|s| *s.borrow_mut() = before);
    out
}

/// The engine in this process, when the app runs one and owns the song on
/// screen.
#[cfg(feature = "local")]
#[must_use]
pub fn local_engine() -> Option<&'static daw::standalone::Standalone> {
    crate::host::host().local_engine()
}
