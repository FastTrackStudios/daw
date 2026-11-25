//! The host's side: answering `host-actions/1`, and calling a plugin's
//! `signal/1`.

use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::sync::Arc;

use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::clap_ostream;
use clap_sys::string_sizes::CLAP_NAME_SIZE;

use crate::stream::{VecOstream, c_string, from_buf, str_arg, write_all};
use crate::sys::{
    self, fts_host_actions, fts_plugin_host_events, fts_plugin_signal, fts_signal_mod_source_info,
};
use crate::{ExtError, ModSourceInfo};

/// Where a subscription's events go: the plugin's `host-events/1`.
#[derive(Clone, Copy)]
pub struct EventSink {
    plugin: *const clap_plugin,
    ext: *const fts_plugin_host_events,
}

// SAFETY: the plugin and its extension outlive the subscription (the host
// drops it when the plugin goes), and `on_event` is [main-thread]: a host
// only delivers from its main thread.
unsafe impl Send for EventSink {}
// SAFETY: as above.
unsafe impl Sync for EventSink {}

impl EventSink {
    /// The plugin's `host-events/1`, if it has one.
    ///
    /// # Safety
    ///
    /// `plugin` is a valid `clap_plugin`, on the main thread.
    #[must_use]
    pub unsafe fn query(plugin: *const clap_plugin) -> Option<Self> {
        if plugin.is_null() {
            return None;
        }
        let get = unsafe { (*plugin).get_extension }?;
        let ext = unsafe { get(plugin, sys::FTS_EXT_HOST_EVENTS.as_ptr()) }
            .cast::<fts_plugin_host_events>();
        (!ext.is_null()).then_some(Self { plugin, ext })
    }

    /// The plugin it delivers to, as host-automation/1's
    /// [`Caller`](crate::automation::Caller) names it: what a host keys
    /// its per-plugin bookkeeping by, read without calling the plugin.
    #[must_use]
    pub fn plugin_key(&self) -> crate::automation::Caller {
        crate::automation::Caller::of(self.plugin)
    }

    /// Whether this sink delivers to the same plugin as `other`: how a host
    /// finds which of its instances a subscription belongs to.
    #[must_use]
    pub fn same_plugin(&self, other: &Self) -> bool {
        std::ptr::eq(self.plugin, other.plugin)
    }

    /// Deliver an event. Main thread.
    pub fn send(&self, subscription: u64, topic: &str, event_json: &str) {
        // SAFETY: non-null (`query`).
        let Some(f) = (unsafe { (*self.ext).on_event }) else {
            return;
        };
        let (topic, json) = (c_string(topic), c_string(event_json));
        unsafe { f(self.plugin, subscription, topic.as_ptr(), json.as_ptr()) };
    }
}

/// What a host implements to answer `com.fasttrackstudio.host-actions/1`:
/// Studio's engine over its Action List; studio-adapters over REAPER's API
/// later. Main-thread calls.
pub trait HostActions: Send + Sync {
    /// Its actions, a JSON array of `{"id","description","args_schema"?}`.
    ///
    /// # Errors
    ///
    /// When there are none to give.
    fn list(&self) -> Result<String, ExtError>;
    /// # Errors
    ///
    /// When the action is unknown.
    fn describe(&self, id: &str) -> Result<String, ExtError>;
    /// Run one. A long one may return `{"job": <id>}` and report on `job`.
    ///
    /// # Errors
    ///
    /// The action's failure.
    fn run(&self, id: &str, args_json: &str) -> Result<String, ExtError>;
    /// Start sending `topic`'s events to `sink`; the subscription id, or
    /// `None` for an unknown topic.
    fn subscribe(&self, topic: &str, sink: EventSink) -> Option<u64> {
        let _ = (topic, sink);
        None
    }
    fn unsubscribe(&self, subscription: u64) {
        let _ = subscription;
    }
    /// A file by reference.
    ///
    /// # Errors
    ///
    /// When the host has none, or refuses it (a private capture).
    fn load_file(&self, reference: &str, blake3: Option<&str>) -> Result<Vec<u8>, ExtError> {
        let _ = blake3;
        Err(ExtError::NotFound(reference.into()))
    }
}

/// How the vtable finds the host's [`HostActions`] from the raw host
/// pointer (clack: its host wrapper; a test: `host_data`).
pub trait HostActionsLookup: 'static {
    /// # Safety
    ///
    /// `host` is the `clap_host` the plugin called the vtable with.
    unsafe fn lookup(host: *const clap_host) -> Option<Arc<dyn HostActions>>;

    /// The event sink of the plugin `host` serves: only the host's own
    /// bookkeeping knows which plugin a `clap_host` belongs to. `None`
    /// (the default) refuses subscriptions.
    ///
    /// # Safety
    ///
    /// As [`lookup`](Self::lookup).
    unsafe fn event_sink(host: *const clap_host) -> Option<EventSink> {
        let _ = host;
        None
    }
}

/// The `host-actions/1` vtable for a host type, as a `'static`.
pub struct HostActionsVtable<L>(PhantomData<L>);

impl<L: HostActionsLookup> HostActionsVtable<L> {
    pub const VTABLE: fts_host_actions = fts_host_actions {
        list: Some(Self::list),
        describe: Some(Self::describe),
        run: Some(Self::run),
        subscribe: Some(Self::subscribe),
        unsubscribe: Some(Self::unsubscribe),
        load_file: Some(Self::load_file),
    };

    /// What `get_extension` returns for [`sys::FTS_EXT_HOST_ACTIONS`].
    #[must_use]
    pub fn as_ptr() -> *const c_void {
        let v: &'static fts_host_actions = &Self::VTABLE;
        std::ptr::from_ref(v).cast()
    }

    fn reply(out: *const clap_ostream, r: Result<String, ExtError>) -> bool {
        let (ok, json) = match r {
            Ok(j) => (true, j),
            Err(e) => (false, e.to_json()),
        };
        // SAFETY: the plugin's stream.
        (unsafe { write_all(out, json.as_bytes()) }) && ok
    }

    unsafe extern "C" fn list(host: *const clap_host, out: *const clap_ostream) -> bool {
        let Some(h) = (unsafe { L::lookup(host) }) else {
            return false;
        };
        Self::reply(out, h.list())
    }

    unsafe extern "C" fn describe(
        host: *const clap_host,
        id: *const c_char,
        out: *const clap_ostream,
    ) -> bool {
        let (Some(h), Some(id)) = (unsafe { L::lookup(host) }, unsafe { str_arg(id) }) else {
            return false;
        };
        Self::reply(out, h.describe(id))
    }

    unsafe extern "C" fn run(
        host: *const clap_host,
        id: *const c_char,
        args: *const c_char,
        out: *const clap_ostream,
    ) -> bool {
        let (Some(h), Some(id)) = (unsafe { L::lookup(host) }, unsafe { str_arg(id) }) else {
            return false;
        };
        Self::reply(out, h.run(id, unsafe { str_arg(args) }.unwrap_or("{}")))
    }

    unsafe extern "C" fn subscribe(host: *const clap_host, topic: *const c_char) -> u64 {
        // The plugin to send to is the one this `clap_host` serves, which
        // only the host knows (clack keeps one host struct per instance).
        let (Some(h), Some(topic)) = (unsafe { L::lookup(host) }, unsafe { str_arg(topic) }) else {
            return 0;
        };
        let Some(sink) = (unsafe { L::event_sink(host) }) else {
            return 0;
        };
        h.subscribe(topic, sink).unwrap_or(0)
    }

    unsafe extern "C" fn unsubscribe(host: *const clap_host, subscription: u64) {
        if let Some(h) = unsafe { L::lookup(host) } {
            h.unsubscribe(subscription);
        }
    }

    unsafe extern "C" fn load_file(
        host: *const clap_host,
        reference: *const c_char,
        blake3: *const c_char,
        out: *const clap_ostream,
    ) -> bool {
        let (Some(h), Some(reference)) =
            (unsafe { L::lookup(host) }, unsafe { str_arg(reference) })
        else {
            return false;
        };
        match h.load_file(reference, unsafe { str_arg(blake3) }) {
            Ok(bytes) => unsafe { write_all(out, &bytes) },
            Err(_) => false,
        }
    }
}

// ── Calling a plugin ────────────────────────────────────────────────────

/// A plugin's `signal/1`, from the host's side.
#[derive(Clone, Copy)]
pub struct SignalPluginClient {
    plugin: *const clap_plugin,
    ext: *const fts_plugin_signal,
}

// SAFETY: valid while the plugin instance lives (the host holds the client
// beside the instance); each function's thread rule is the caller's, as
// for every CLAP call.
unsafe impl Send for SignalPluginClient {}
// SAFETY: as above.
unsafe impl Sync for SignalPluginClient {}

impl SignalPluginClient {
    /// Ask `plugin` for `signal/1`. `None` from a plugin without it.
    ///
    /// # Safety
    ///
    /// `plugin` is a valid, initialized `clap_plugin`, on the main thread.
    #[must_use]
    pub unsafe fn query(plugin: *const clap_plugin) -> Option<Self> {
        if plugin.is_null() {
            return None;
        }
        let get = unsafe { (*plugin).get_extension }?;
        let ext = unsafe { get(plugin, sys::FTS_EXT_SIGNAL.as_ptr()) }.cast::<fts_plugin_signal>();
        (!ext.is_null()).then_some(Self { plugin, ext })
    }

    fn ext(&self) -> &fts_plugin_signal {
        // SAFETY: non-null (`query`), valid while the plugin is.
        unsafe { &*self.ext }
    }

    fn json(&self, f: impl FnOnce(*const clap_ostream) -> bool) -> Result<String, ExtError> {
        let out = VecOstream::new();
        let ok = f(out.as_raw());
        let json = out.into_string();
        if ok {
            Ok(json)
        } else {
            Err(ExtError::from_json_or(&json, "the plugin refused"))
        }
    }

    /// [main-thread]
    ///
    /// # Errors
    ///
    /// When the plugin gives none.
    pub fn list_actions(&self) -> Result<String, ExtError> {
        let f = self.ext().list_actions.ok_or(ExtError::Unsupported)?;
        self.json(|out| unsafe { f(self.plugin, out) })
    }

    /// [main-thread]
    ///
    /// # Errors
    ///
    /// The action's failure.
    pub fn run_action(&self, id: &str, args_json: &str) -> Result<String, ExtError> {
        let f = self.ext().run_action.ok_or(ExtError::Unsupported)?;
        let (id, args) = (c_string(id), c_string(args_json));
        self.json(|out| unsafe { f(self.plugin, id.as_ptr(), args.as_ptr(), out) })
    }

    /// [main-thread]
    ///
    /// # Errors
    ///
    /// When the plugin gives none.
    pub fn chain(&self) -> Result<String, ExtError> {
        let f = self.ext().chain.ok_or(ExtError::Unsupported)?;
        self.json(|out| unsafe { f(self.plugin, out) })
    }

    /// [thread-safe]
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.ext().revision.map_or(0, |f| unsafe { f(self.plugin) })
    }

    /// [main-thread]
    #[must_use]
    pub fn param_stable_id(&self, param_id: u32) -> Option<String> {
        let f = self.ext().param_stable_id?;
        let mut buf = [0 as c_char; CLAP_NAME_SIZE];
        let cap = u32::try_from(buf.len()).ok()?;
        unsafe { f(self.plugin, param_id, buf.as_mut_ptr(), cap) }.then(|| from_buf(&buf))
    }

    /// [main-thread]
    #[must_use]
    pub fn mod_sources(&self) -> Vec<ModSourceInfo> {
        let (Some(count), Some(info)) = (self.ext().mod_source_count, self.ext().mod_source_info)
        else {
            return Vec::new();
        };
        let n = unsafe { count(self.plugin) };
        (0..n)
            .filter_map(|i| {
                let mut raw = fts_signal_mod_source_info {
                    id: [0; CLAP_NAME_SIZE],
                    name: [0; CLAP_NAME_SIZE],
                    unit: [0; 16],
                };
                unsafe { info(self.plugin, i, &raw mut raw) }.then(|| ModSourceInfo {
                    id: from_buf(&raw.id),
                    name: from_buf(&raw.name),
                    unit: from_buf(&raw.unit),
                })
            })
            .collect()
    }

    /// [audio-thread]: lock-free, between `process()` calls.
    #[must_use]
    pub fn mod_source_value(&self, index: u32) -> Option<f64> {
        let f = self.ext().mod_source_value?;
        Some(unsafe { f(self.plugin, index) })
    }

    /// [main-thread]
    #[must_use]
    pub fn set_session(&self, session: Option<(&str, &str)>) -> bool {
        let Some(f) = self.ext().set_session else {
            return false;
        };
        match session {
            Some((s, p)) => {
                let (s, p) = (c_string(s), c_string(p));
                unsafe { f(self.plugin, s.as_ptr(), p.as_ptr()) }
            }
            None => unsafe { f(self.plugin, std::ptr::null(), std::ptr::null()) },
        }
    }
}
