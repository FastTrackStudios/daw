//! Module system — standard interface for DAW extension modules.
//!
//! Any library that wants to be loadable by a DAW extension host implements
//! [`DawModule`]. The host collects modules and calls the standard methods
//! to register actions and subscribe to events.
//!
//! # For library authors
//!
//! ```rust,ignore
//! use daw::module::{DawModule, ActionDef, ModuleContext};
//!
//! pub struct MyModule;
//!
//! impl DawModule for MyModule {
//!     fn name(&self) -> &str { "my-module" }
//!     fn display_name(&self) -> &str { "My Module" }
//!
//!     fn actions(&self) -> Vec<ActionDef> {
//!         vec![
//!             ActionDef::new("FTS_MY_ACTION", "Do something", || {
//!                 tracing::info!("Action executed!");
//!             }),
//!         ]
//!     }
//! }
//!
//! // Convention: export a `module()` function
//! pub fn module() -> Box<dyn DawModule> {
//!     Box::new(MyModule)
//! }
//! ```
//!
//! # For the host extension
//!
//! ```rust,ignore
//! use daw::module::{self, DawModule, ModuleContext};
//!
//! let modules: Vec<Box<dyn DawModule>> = vec![
//!     session::module(),
//!     sync::module(),
//!     dynamic_template::module(),
//! ];
//!
//! let ctx = ModuleContext::new(tokio_runtime.clone());
//! module::init_all(&modules, &ctx);
//! let action_defs = module::collect_actions(&modules);
//! ```

use std::sync::Arc;

/// An action that a module registers with the DAW.
pub struct ActionDef {
    /// Unique command ID registered with the DAW (e.g. "FTS_SESSION_NEXT_SONG").
    pub command_id: String,
    /// Human-readable name shown in the action list.
    pub display_name: String,
    /// Handler called on the main thread when the action is triggered.
    pub handler: Arc<dyn Fn() + Send + Sync>,
}

impl ActionDef {
    /// Create a new action definition.
    pub fn new(
        command_id: impl Into<String>,
        display_name: impl Into<String>,
        handler: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            command_id: command_id.into(),
            display_name: display_name.into(),
            handler: Arc::new(handler),
        }
    }

    /// Convert to the tuple format used by extension action registration.
    pub fn into_tuple(self) -> (String, String, Arc<dyn Fn() + Send + Sync>) {
        (self.command_id, self.display_name, self.handler)
    }
}

/// Context provided to modules during initialization.
///
/// Gives modules access to the async runtime for spawning tasks
/// (event subscriptions, background work, etc.)
pub struct ModuleContext {
    /// The shared tokio runtime.
    pub runtime: Arc<tokio::runtime::Runtime>,
}

impl ModuleContext {
    pub fn new(runtime: Arc<tokio::runtime::Runtime>) -> Self {
        Self { runtime }
    }

    /// Spawn an async task on the runtime.
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(future);
    }
}

/// Standard interface for a DAW extension module.
///
/// Libraries implement this trait and export a `pub fn module() -> Box<dyn DawModule>`
/// function. The host extension collects modules and calls these methods during startup.
///
/// # Lifecycle
///
/// 1. Host creates all modules: `let modules = vec![foo::module(), bar::module()]`
/// 2. Host calls `init_all(&modules, &ctx)` — each module initializes
/// 3. Host calls `collect_actions(&modules)` — gathers all action defs for registration
/// 4. Host registers actions with the DAW
/// 5. When an action is triggered, host dispatches to the handler
pub trait DawModule: Send + Sync {
    /// Unique module identifier (e.g. "session", "sync", "input").
    fn name(&self) -> &str;

    /// Human-readable display name (e.g. "Session Control", "Transport Sync").
    fn display_name(&self) -> &str;

    /// Return all actions this module provides.
    /// Called once at startup.
    fn actions(&self) -> Vec<ActionDef>;

    /// Initialize the module. Called once after DAW is available.
    /// Use for one-time setup: loading config, building caches, etc.
    fn init(&self, _ctx: &ModuleContext) {}

    /// Subscribe to DAW events (track changes, transport, etc.)
    /// Called once after init. Use `ctx.spawn()` for async event listeners.
    fn subscribe(&self, _ctx: &ModuleContext) {}
}

/// Collect all action tuples from a list of modules.
pub fn collect_actions(
    modules: &[Box<dyn DawModule>],
) -> Vec<(String, String, Arc<dyn Fn() + Send + Sync>)> {
    let mut all = Vec::new();
    for m in modules {
        let actions = m.actions();
        tracing::info!(
            module = m.name(),
            actions = actions.len(),
            "Collected actions from {}",
            m.display_name()
        );
        all.extend(actions.into_iter().map(|a| a.into_tuple()));
    }
    all
}

/// Initialize all modules and subscribe to events.
pub fn init_all(modules: &[Box<dyn DawModule>], ctx: &ModuleContext) {
    for m in modules {
        tracing::info!(module = m.name(), "Initializing {}", m.display_name());
        m.init(ctx);
    }
    for m in modules {
        m.subscribe(ctx);
    }
    tracing::info!(modules = modules.len(), "All modules initialized");
}
