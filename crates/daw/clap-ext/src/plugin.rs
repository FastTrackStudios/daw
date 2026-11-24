//! The plugin's side: answering `signal/1`, and calling the host's
//! `host-actions/1`. Over clap-sys, so it plugs into nice-plug's
//! `ClapPlugin::clap_extension`.

use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::sync::Arc;

use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::clap_ostream;

use crate::stream::{VecOstream, c_string, copy_to_buf, str_arg, write_all};
use crate::sys::{self, fts_host_actions, fts_plugin_signal, fts_signal_mod_source_info};
use crate::{ExtError, ModSourceInfo};

/// What a plugin implements to answer `com.fasttrackstudio.signal/1`: the
/// Signal CLAP over its graph, each block's CLAP over its processor.
pub trait SignalPlugin: Send + Sync {
    /// Its actions, as a JSON array.
    fn list_actions(&self) -> String;
    /// Run one: JSON arguments in, its JSON result out.
    ///
    /// # Errors
    ///
    /// The action's failure, sent to the host as `{"error": "..."}`.
    fn run_action(&self, id: &str, args_json: &str) -> Result<String, ExtError>;
    /// The chain as data, JSON.
    fn chain(&self) -> String;
    /// Increases on every change to what `chain` returns. Thread-safe.
    fn revision(&self) -> u64;
    /// A CLAP parameter's stable (Signal) id.
    fn param_stable_id(&self, param_id: u32) -> Option<String>;
    fn mod_sources(&self) -> Vec<ModSourceInfo>;
    /// Source `index`'s value after the last `process()`. Audio thread:
    /// lock-free.
    fn mod_source_value(&self, index: usize) -> f64;
    /// Join a session (`Some((session, port))`) or leave it (`None`).
    fn set_session(&self, session: Option<(&str, &str)>) -> bool;
}

/// How a vtable finds the plugin's [`SignalPlugin`] from the raw plugin
/// pointer. With nice-plug: `clap_extension_state::<P>(plugin)` downcast.
pub trait SignalPluginLookup: 'static {
    /// # Safety
    ///
    /// `plugin` is the `clap_plugin` the host called the vtable with.
    unsafe fn lookup(plugin: *const clap_plugin) -> Option<Arc<dyn SignalPlugin>>;
}

/// The `signal/1` vtable for a plugin type, as a `'static`.
pub struct SignalVtable<L>(PhantomData<L>);

impl<L: SignalPluginLookup> SignalVtable<L> {
    pub const VTABLE: fts_plugin_signal = fts_plugin_signal {
        list_actions: Some(Self::list_actions),
        run_action: Some(Self::run_action),
        chain: Some(Self::chain),
        revision: Some(Self::revision),
        param_stable_id: Some(Self::param_stable_id),
        mod_source_count: Some(Self::mod_source_count),
        mod_source_info: Some(Self::mod_source_info),
        mod_source_value: Some(Self::mod_source_value),
        set_session: Some(Self::set_session),
    };

    /// What `get_extension` returns for [`sys::FTS_EXT_SIGNAL`].
    #[must_use]
    pub fn as_ptr() -> *const c_void {
        let v: &'static fts_plugin_signal = &Self::VTABLE;
        std::ptr::from_ref(v).cast()
    }

    unsafe extern "C" fn list_actions(
        plugin: *const clap_plugin,
        out: *const clap_ostream,
    ) -> bool {
        // SAFETY: the host's call, with the plugin it queried.
        let Some(p) = (unsafe { L::lookup(plugin) }) else {
            return false;
        };
        // SAFETY: the host's stream.
        unsafe { write_all(out, p.list_actions().as_bytes()) }
    }

    unsafe extern "C" fn run_action(
        plugin: *const clap_plugin,
        id: *const c_char,
        args: *const c_char,
        out: *const clap_ostream,
    ) -> bool {
        // SAFETY: as above, and the host's strings.
        let (Some(p), Some(id)) = (unsafe { L::lookup(plugin) }, unsafe { str_arg(id) }) else {
            return false;
        };
        let args = unsafe { str_arg(args) }.unwrap_or("{}");
        let (ok, json) = match p.run_action(id, args) {
            Ok(json) => (true, json),
            Err(e) => (false, e.to_json()),
        };
        // SAFETY: the host's stream.
        (unsafe { write_all(out, json.as_bytes()) }) && ok
    }

    unsafe extern "C" fn chain(plugin: *const clap_plugin, out: *const clap_ostream) -> bool {
        let Some(p) = (unsafe { L::lookup(plugin) }) else {
            return false;
        };
        unsafe { write_all(out, p.chain().as_bytes()) }
    }

    unsafe extern "C" fn revision(plugin: *const clap_plugin) -> u64 {
        unsafe { L::lookup(plugin) }.map_or(0, |p| p.revision())
    }

    unsafe extern "C" fn param_stable_id(
        plugin: *const clap_plugin,
        param: clap_id,
        out: *mut c_char,
        cap: u32,
    ) -> bool {
        let Some(p) = (unsafe { L::lookup(plugin) }) else {
            return false;
        };
        let Some(id) = p.param_stable_id(param) else {
            return false;
        };
        if out.is_null() || cap == 0 {
            return false;
        }
        let Ok(cap) = usize::try_from(cap) else {
            return false;
        };
        // SAFETY: the host's buffer of `cap` bytes.
        let buf = unsafe { std::slice::from_raw_parts_mut(out, cap) };
        copy_to_buf(&id, buf);
        true
    }

    unsafe extern "C" fn mod_source_count(plugin: *const clap_plugin) -> u32 {
        unsafe { L::lookup(plugin) }
            .map_or(0, |p| u32::try_from(p.mod_sources().len()).unwrap_or(0))
    }

    unsafe extern "C" fn mod_source_info(
        plugin: *const clap_plugin,
        index: u32,
        info: *mut fts_signal_mod_source_info,
    ) -> bool {
        let Some(p) = (unsafe { L::lookup(plugin) }) else {
            return false;
        };
        let Some(src) = usize::try_from(index)
            .ok()
            .and_then(|i| p.mod_sources().into_iter().nth(i))
        else {
            return false;
        };
        if info.is_null() {
            return false;
        }
        // SAFETY: the host's struct.
        let info = unsafe { &mut *info };
        copy_to_buf(&src.id, &mut info.id);
        copy_to_buf(&src.name, &mut info.name);
        copy_to_buf(&src.unit, &mut info.unit);
        true
    }

    unsafe extern "C" fn mod_source_value(plugin: *const clap_plugin, index: u32) -> f64 {
        let Ok(i) = usize::try_from(index) else {
            return 0.0;
        };
        unsafe { L::lookup(plugin) }.map_or(0.0, |p| p.mod_source_value(i))
    }

    unsafe extern "C" fn set_session(
        plugin: *const clap_plugin,
        session: *const c_char,
        port: *const c_char,
    ) -> bool {
        let Some(p) = (unsafe { L::lookup(plugin) }) else {
            return false;
        };
        match (unsafe { str_arg(session) }, unsafe { str_arg(port) }) {
            (Some(s), Some(port)) => p.set_session(Some((s, port))),
            (None, _) => p.set_session(None),
            (Some(_), None) => false,
        }
    }
}

// ── Calling the host ────────────────────────────────────────────────────

/// The host's `host-actions/1`, from the plugin's side. Main-thread calls
/// only, which is where a plugin builds its graph and runs actions.
#[derive(Clone, Copy)]
pub struct HostActionsClient {
    host: *const clap_host,
    ext: *const fts_host_actions,
}

// SAFETY: the host pointer and its extension live as long as the plugin
// instance, and the extension's functions are [main-thread]: the plugin
// holds the client where only its main thread calls it.
unsafe impl Send for HostActionsClient {}
// SAFETY: as above.
unsafe impl Sync for HostActionsClient {}

impl HostActionsClient {
    /// Ask `host` for `host-actions/1`. `None` from a host without it.
    ///
    /// # Safety
    ///
    /// `host` is the plugin's valid `clap_host`, called on the main thread.
    #[must_use]
    pub unsafe fn query(host: *const clap_host) -> Option<Self> {
        if host.is_null() {
            return None;
        }
        // SAFETY: the caller's contract.
        let get = unsafe { (*host).get_extension }?;
        let ext =
            unsafe { get(host, sys::FTS_EXT_HOST_ACTIONS.as_ptr()) }.cast::<fts_host_actions>();
        (!ext.is_null()).then_some(Self { host, ext })
    }

    fn ext(&self) -> &fts_host_actions {
        // SAFETY: non-null, valid for the plugin's life (`query`).
        unsafe { &*self.ext }
    }

    fn call_json(&self, f: impl FnOnce(*const clap_ostream) -> bool) -> Result<String, ExtError> {
        let out = VecOstream::new();
        let ok = f(out.as_raw());
        let json = out.into_string();
        if ok {
            Ok(json)
        } else {
            Err(ExtError::from_json_or(&json, "the host refused"))
        }
    }

    /// The host's actions, a JSON array.
    ///
    /// # Errors
    ///
    /// When the host has none to give.
    pub fn list(&self) -> Result<String, ExtError> {
        let f = self.ext().list.ok_or(ExtError::Unsupported)?;
        // SAFETY: the host's function, its host, our stream.
        self.call_json(|out| unsafe { f(self.host, out) })
    }

    /// # Errors
    ///
    /// When the host doesn't know the action.
    pub fn describe(&self, id: &str) -> Result<String, ExtError> {
        let f = self.ext().describe.ok_or(ExtError::Unsupported)?;
        let id = c_string(id);
        self.call_json(|out| unsafe { f(self.host, id.as_ptr(), out) })
    }

    /// Run an action: JSON arguments in, its JSON result out.
    ///
    /// # Errors
    ///
    /// The action's failure (`{"error": ...}` from the host).
    pub fn run(&self, id: &str, args_json: &str) -> Result<String, ExtError> {
        let f = self.ext().run.ok_or(ExtError::Unsupported)?;
        let (id, args) = (c_string(id), c_string(args_json));
        self.call_json(|out| unsafe { f(self.host, id.as_ptr(), args.as_ptr(), out) })
    }

    /// Subscribe to a topic; events arrive at the plugin's `host-events/1`.
    #[must_use]
    pub fn subscribe(&self, topic: &str) -> Option<u64> {
        let f = self.ext().subscribe?;
        let topic = c_string(topic);
        let id = unsafe { f(self.host, topic.as_ptr()) };
        (id != 0).then_some(id)
    }

    pub fn unsubscribe(&self, subscription: u64) {
        if let Some(f) = self.ext().unsubscribe {
            unsafe { f(self.host, subscription) };
        }
    }

    /// A file by reference, resolved by the host.
    ///
    /// # Errors
    ///
    /// When the host has no such file, or refuses it.
    pub fn load_file(&self, reference: &str, blake3: Option<&str>) -> Result<Vec<u8>, ExtError> {
        let f = self.ext().load_file.ok_or(ExtError::Unsupported)?;
        let reference_c = c_string(reference);
        let blake3 = blake3.map(c_string);
        let out = VecOstream::new();
        let ok = unsafe {
            f(
                self.host,
                reference_c.as_ptr(),
                blake3.as_ref().map_or(std::ptr::null(), |b| b.as_ptr()),
                out.as_raw(),
            )
        };
        if ok {
            Ok(out.into_bytes())
        } else {
            Err(ExtError::NotFound(reference.into()))
        }
    }
}
