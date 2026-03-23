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
