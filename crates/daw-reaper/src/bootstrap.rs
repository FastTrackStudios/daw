//! Plugin-side REAPER bootstrap types.
//!
//! Re-exports the reaper-rs types needed for CLAP plugins to initialize
//! REAPER API access via `ReaperPluginEntry`. This follows the Helgobox
//! pattern where each .dylib gets its own copy of Rust statics and needs
//! its own initialized `Reaper`, `TaskSupport`, and `daw-reaper` setup.
//!
//! # Usage
//!
//! ```rust,ignore
//! use daw::reaper::bootstrap::*;
//!
//! #[no_mangle]
//! pub unsafe extern "C" fn ReaperPluginEntry(
//!     h_instance: HINSTANCE,
//!     rec: *mut reaper_plugin_info_t,
//! ) -> std::os::raw::c_int {
//!     let ctx = static_plugin_context();
//!     bootstrap_extension_plugin(h_instance, rec, ctx, plugin_init)
//! }
//!
//! fn plugin_init(context: PluginContext) -> Result<(), Box<dyn Error>> {
//!     HighReaper::load(context).setup()?;
//!     // ...
//! }
//! ```

// reaper-low: raw FFI types for ReaperPluginEntry signature and direct API access
pub use reaper_low::Reaper as LowReaper;
pub use reaper_low::raw::HINSTANCE;
pub use reaper_low::raw::MediaItem_Take;
pub use reaper_low::raw::MediaTrack;
pub use reaper_low::raw::reaper_plugin_info_t;
pub use reaper_low::{PluginContext, bootstrap_extension_plugin, static_plugin_context};

// reaper-high: initialization, main-thread dispatch, and project/track access
pub use reaper_high::{
    MainTaskMiddleware, MainThreadTask, Reaper as HighReaper, TaskSupport, Track as HighTrack,
};

// reaper-medium: session registration for timer callbacks and play state
pub use reaper_medium::{ProjectContext as ReaperProjectContext, ReaperSession};

/// Initialize daw-reaper from a CLAP host context.
///
/// CLAP plugins call this during `initialize()` with the raw `clap_host*`
/// obtained from `InitContext::raw_host_context()`. This:
///
/// 1. Calls `host->get_extension("cockos.reaper_extension")` to get `reaper_plugin_info_t*`
/// 2. Builds a `PluginContext` from it
/// 3. Initializes reaper-high (`HighReaper`)
/// 4. Sets up `TaskSupport` and `MainTaskMiddleware` for main-thread dispatch
/// 5. Registers a timer callback for periodic work
///
/// Returns a [`ReaperSession`] that can be used to register timer callbacks,
/// or `None` if REAPER API is not available (e.g. running in a non-REAPER host).
///
/// # Safety
///
/// `raw_clap_host` must be a valid `*const clap_host` pointer from the CLAP host.
pub unsafe fn init_from_clap_host(
    raw_clap_host: *const std::ffi::c_void,
) -> Option<ReaperBootstrap> {
    if raw_clap_host.is_null() {
        return None;
    }

    // Cast to clap_host and call get_extension
    #[repr(C)]
    struct ClapHost {
        clap_version: [u32; 3],
        host_data: *mut std::ffi::c_void,
        name: *const std::ffi::c_char,
        vendor: *const std::ffi::c_char,
        url: *const std::ffi::c_char,
        version: *const std::ffi::c_char,
        get_extension: Option<
            unsafe extern "C" fn(
                host: *const ClapHost,
                extension_id: *const std::ffi::c_char,
            ) -> *const std::ffi::c_void,
        >,
        request_restart: Option<unsafe extern "C" fn(host: *const ClapHost)>,
        request_process: Option<unsafe extern "C" fn(host: *const ClapHost)>,
        request_callback: Option<unsafe extern "C" fn(host: *const ClapHost)>,
    }

    let host = raw_clap_host as *const ClapHost;
    let get_ext = (*host).get_extension?;

    let ext_id = b"cockos.reaper_extension\0";
    let rec_ptr = get_ext(host, ext_id.as_ptr() as *const std::ffi::c_char);
    if rec_ptr.is_null() {
        return None; // Not running in REAPER
    }

    let rec = *(rec_ptr as *const reaper_plugin_info_t);

    // Build PluginContext from the reaper_plugin_info_t
    let static_ctx = static_plugin_context();
    let h_instance = static_ctx.h_instance;
    let context = match PluginContext::from_extension_plugin(h_instance, rec, static_ctx) {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Initialize reaper-high
    match HighReaper::load(context).setup() {
        Ok(_) => {}
        Err(_) => {} // Already initialized — that's fine
    }

    // Set up TaskSupport for main-thread dispatch
    let (task_sender, task_receiver) = crossbeam_channel::unbounded();
    let task_support = TaskSupport::new(task_sender.clone());
    let middleware = MainTaskMiddleware::new(task_sender, task_receiver);

    // Set TaskSupport for daw-reaper
    let task_support_ref: &'static TaskSupport = Box::leak(Box::new(task_support));
    crate::set_task_support(task_support_ref);

    // Create a session for timer registration
    let session = ReaperSession::load(context);

    Some(ReaperBootstrap {
        middleware,
        session,
    })
}

/// Result of [`init_from_clap_host`]. Holds the resources needed for
/// REAPER main-thread dispatch and timer registration.
pub struct ReaperBootstrap {
    /// Drains main-thread tasks — call `run()` from your timer callback.
    pub middleware: MainTaskMiddleware,
    /// Session for registering timer callbacks with REAPER.
    pub session: ReaperSession,
}

// ── DAW-abstracted plugin host API ──────────────────────────────────

/// High-level DAW host API for CLAP plugins.
///
/// Provides DAW-abstracted access to REAPER from within a CLAP plugin.
/// No direct reaper-rs dependency needed — only `daw` crate types.
///
/// # Example
///
/// ```rust,ignore
/// fn initialize(&mut self, ..., context: &mut impl InitContext<Self>) -> bool {
///     if let Some(host) = daw::reaper::PluginHost::init(context.raw_host_context()) {
///         host.register_timer(my_timer_callback);
///     }
///     true
/// }
///
/// extern "C" fn my_timer_callback() {
///     let host = daw::reaper::PluginHost::get();
///     host.set_ext_state("MY_PLUGIN", "status", "running");
///     let tracks = host.track_count();
/// }
/// ```
pub struct PluginHost;

static PLUGIN_HOST_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl PluginHost {
    /// Initialize DAW access from a CLAP host context.
    ///
    /// Call from `Plugin::initialize()` with `context.raw_host_context()`.
    /// Returns `Some(PluginHost)` if running in REAPER, `None` otherwise.
    pub fn init(raw_host_context: Option<*const std::ffi::c_void>) -> Option<Self> {
        let host_ptr = raw_host_context?;

        let mut bootstrap = unsafe { init_from_clap_host(host_ptr) }?;

        // Register an internal timer that drains the task queue
        static MIDDLEWARE: std::sync::OnceLock<std::sync::Mutex<MainTaskMiddleware>> =
            std::sync::OnceLock::new();
        let _ = MIDDLEWARE.set(std::sync::Mutex::new(bootstrap.middleware));

        // Internal timer that drains tasks + calls user timers
        extern "C" fn internal_timer() {
            // Drain main-thread tasks
            let mw = unsafe {
                // SAFETY: MIDDLEWARE is set before the timer is registered
                &*std::ptr::addr_of!(MIDDLEWARE)
            };
            if let Some(m) = mw.get() {
                if let Ok(mut mw) = m.lock() {
                    mw.run();
                }
            }

            // Call user timer callbacks
            let cbs = unsafe { &*std::ptr::addr_of!(USER_TIMER_CALLBACKS) };
            if let Ok(callbacks) = cbs.lock() {
                for cb in callbacks.iter() {
                    cb();
                }
            }
        }

        let _ = bootstrap.session.plugin_register_add_timer(internal_timer);
        let _ = Box::leak(Box::new(bootstrap.session));

        PLUGIN_HOST_READY.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(PluginHost)
    }

    /// Get the plugin host singleton.
    ///
    /// Returns `Some(PluginHost)` if [`PluginHost::init`] was called.
    pub fn get() -> Option<Self> {
        if PLUGIN_HOST_READY.load(std::sync::atomic::Ordering::Relaxed) {
            Some(PluginHost)
        } else {
            None
        }
    }

    /// Register a callback that fires at ~30Hz on REAPER's main thread.
    pub fn register_timer(&self, callback: fn()) {
        if let Ok(mut cbs) = USER_TIMER_CALLBACKS.lock() {
            cbs.push(callback);
        }
    }

    /// Get the number of tracks in the current project.
    pub fn track_count(&self) -> u32 {
        HighReaper::get().current_project().track_count()
    }

    /// Read a global ExtState value.
    pub fn get_ext_state(&self, section: &str, key: &str) -> Option<String> {
        let low = HighReaper::get().medium_reaper().low();
        let section = std::ffi::CString::new(section).ok()?;
        let key = std::ffi::CString::new(key).ok()?;
        let ptr = unsafe { low.GetExtState(section.as_ptr(), key.as_ptr()) };
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .ok()?
            .to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    /// Write a global ExtState value.
    pub fn set_ext_state(&self, section: &str, key: &str, value: &str) {
        let low = HighReaper::get().medium_reaper().low();
        let section = std::ffi::CString::new(section).unwrap();
        let key = std::ffi::CString::new(key).unwrap();
        let val = std::ffi::CString::new(value).unwrap();
        unsafe {
            low.SetExtState(section.as_ptr(), key.as_ptr(), val.as_ptr(), false);
        }
    }

    /// Show a message in REAPER's console.
    pub fn show_console_msg(&self, msg: &str) {
        let low = HighReaper::get().medium_reaper().low();
        if let Ok(c_msg) = std::ffi::CString::new(msg) {
            unsafe { low.ShowConsoleMsg(c_msg.as_ptr()) };
        }
    }
}

static USER_TIMER_CALLBACKS: std::sync::Mutex<Vec<fn()>> = std::sync::Mutex::new(Vec::new());
