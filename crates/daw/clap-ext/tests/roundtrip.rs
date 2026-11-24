//! Both extensions across a real C ABI boundary: a `clap_host` and a
//! `clap_plugin` built in memory, each side's vtable called through the
//! other side's client, as a host and a plugin would.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use daw_clap_ext::host::{
    EventSink, HostActions, HostActionsLookup, HostActionsVtable, SignalPluginClient,
};
use daw_clap_ext::plugin::{HostActionsClient, SignalPlugin, SignalPluginLookup, SignalVtable};
use daw_clap_ext::{ExtError, ModSourceInfo, sys};

// ── A host with actions ─────────────────────────────────────────────────

struct Studio {
    ran: Mutex<Vec<(String, String)>>,
}

impl HostActions for Studio {
    fn list(&self) -> Result<String, ExtError> {
        Ok(r#"[{"id":"HITS_GET","description":"The hits of a piece"}]"#.into())
    }
    fn describe(&self, id: &str) -> Result<String, ExtError> {
        if id == "HITS_GET" {
            Ok(r#"{"id":"HITS_GET"}"#.into())
        } else {
            Err(ExtError::NotFound(id.into()))
        }
    }
    fn run(&self, id: &str, args: &str) -> Result<String, ExtError> {
        self.ran.lock().unwrap().push((id.into(), args.into()));
        match id {
            "HITS_GET" => Ok(r#"{"hits":[{"time":0.5,"velocity":0.8}]}"#.into()),
            _ => Err(ExtError::Failed(format!("no action {id}"))),
        }
    }
    fn load_file(&self, reference: &str, _blake3: Option<&str>) -> Result<Vec<u8>, ExtError> {
        if reference == "rig-library:guitar/amp.nam" {
            Ok(vec![1, 2, 3, 0, 255])
        } else {
            Err(ExtError::NotFound(reference.into()))
        }
    }
}

struct ByHostData;
impl HostActionsLookup for ByHostData {
    unsafe fn lookup(host: *const clap_host) -> Option<Arc<dyn HostActions>> {
        let data = unsafe { (*host).host_data }.cast::<Arc<dyn HostActions>>();
        (!data.is_null()).then(|| unsafe { (*data).clone() })
    }
}

unsafe extern "C" fn host_get_extension(
    _host: *const clap_host,
    id: *const c_char,
) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == sys::FTS_EXT_HOST_ACTIONS {
        HostActionsVtable::<ByHostData>::as_ptr()
    } else {
        std::ptr::null()
    }
}

// ── A plugin with signal/1 ──────────────────────────────────────────────

struct Comp {
    revision: AtomicU64,
    session: Mutex<Option<(String, String)>>,
}

impl SignalPlugin for Comp {
    fn list_actions(&self) -> String {
        r#"[{"id":"SIGNAL_CHAIN"}]"#.into()
    }
    fn run_action(&self, id: &str, args: &str) -> Result<String, ExtError> {
        if id == "SIGNAL_CHAIN" {
            self.revision.fetch_add(1, Ordering::Relaxed);
            Ok(format!(r#"{{"ok":true,"args":{args}}}"#))
        } else {
            Err(ExtError::Failed(format!("unknown \"{id}\"")))
        }
    }
    fn chain(&self) -> String {
        r#"{"blocks":["comp"]}"#.into()
    }
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
    fn param_stable_id(&self, param_id: u32) -> Option<String> {
        ["threshold", "ratio"]
            .get(param_id as usize)
            .map(|s| (*s).to_owned())
    }
    fn mod_sources(&self) -> Vec<ModSourceInfo> {
        vec![ModSourceInfo {
            id: "gain_reduction".into(),
            name: "Gain Reduction".into(),
            unit: "dB".into(),
        }]
    }
    fn mod_source_value(&self, index: usize) -> f64 {
        if index == 0 { -4.5 } else { 0.0 }
    }
    fn set_session(&self, session: Option<(&str, &str)>) -> bool {
        *self.session.lock().unwrap() = session.map(|(s, p)| (s.into(), p.into()));
        true
    }
}

struct ByPluginData;
impl SignalPluginLookup for ByPluginData {
    unsafe fn lookup(plugin: *const clap_plugin) -> Option<Arc<dyn SignalPlugin>> {
        let data = unsafe { (*plugin).plugin_data }.cast::<Arc<dyn SignalPlugin>>();
        (!data.is_null()).then(|| unsafe { (*data).clone() })
    }
}

unsafe extern "C" fn plugin_get_extension(
    _plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == sys::FTS_EXT_SIGNAL {
        SignalVtable::<ByPluginData>::as_ptr()
    } else {
        std::ptr::null()
    }
}

#[test]
fn a_plugin_runs_the_hosts_actions_and_loads_its_files() {
    let studio = Arc::new(Studio {
        ran: Mutex::new(Vec::new()),
    });
    let data: Box<Arc<dyn HostActions>> = Box::new(studio.clone());
    // SAFETY: an all-null clap_host is valid; we fill what the test uses.
    let mut host: clap_host = unsafe { std::mem::zeroed() };
    host.host_data = Box::into_raw(data).cast();
    host.get_extension = Some(host_get_extension);

    let client =
        unsafe { HostActionsClient::query(&raw const host) }.expect("the host has host-actions/1");
    assert!(client.list().unwrap().contains("HITS_GET"));
    assert_eq!(client.describe("HITS_GET").unwrap(), r#"{"id":"HITS_GET"}"#);
    assert!(client.describe("NOPE").is_err());
    let hits = client.run("HITS_GET", r#"{"piece":"Drums/Kick"}"#).unwrap();
    assert!(hits.contains("0.8"), "{hits}");
    assert_eq!(
        studio.ran.lock().unwrap()[0],
        ("HITS_GET".into(), r#"{"piece":"Drums/Kick"}"#.into())
    );
    match client.run("QUANTIZE", "{}") {
        Err(ExtError::Failed(why)) => assert_eq!(why, "no action QUANTIZE"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        client
            .load_file("rig-library:guitar/amp.nam", None)
            .unwrap(),
        vec![1, 2, 3, 0, 255]
    );
    assert!(client.load_file("elsewhere", None).is_err());
    assert_eq!(
        client.subscribe("hits:Drums/Kick"),
        None,
        "no event sink: subscriptions refused"
    );

    // A host without the extension.
    host.get_extension = None;
    assert!(unsafe { HostActionsClient::query(&raw const host) }.is_none());
    drop(unsafe { Box::from_raw(host.host_data.cast::<Arc<dyn HostActions>>()) });
}

#[test]
fn a_host_drives_signal_and_reads_its_modulation_sources() {
    let comp = Arc::new(Comp {
        revision: AtomicU64::new(0),
        session: Mutex::new(None),
    });
    let data: Box<Arc<dyn SignalPlugin>> = Box::new(comp.clone());
    // SAFETY: as above.
    let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
    plugin.plugin_data = Box::into_raw(data).cast();
    plugin.get_extension = Some(plugin_get_extension);

    let client =
        unsafe { SignalPluginClient::query(&raw const plugin) }.expect("the plugin has signal/1");
    assert!(client.list_actions().unwrap().contains("SIGNAL_CHAIN"));
    assert_eq!(client.revision(), 0);
    assert_eq!(
        client
            .run_action("SIGNAL_CHAIN", r#"{"track":"Kick/Sum"}"#)
            .unwrap(),
        r#"{"ok":true,"args":{"track":"Kick/Sum"}}"#
    );
    assert_eq!(client.revision(), 1);
    match client.run_action("SIGNAL_X", "{}") {
        Err(ExtError::Failed(why)) => assert_eq!(why, r#"unknown "SIGNAL_X""#),
        other => panic!("{other:?}"),
    }
    assert_eq!(client.chain().unwrap(), r#"{"blocks":["comp"]}"#);
    assert_eq!(client.param_stable_id(1).as_deref(), Some("ratio"));
    assert_eq!(client.param_stable_id(9), None);
    assert_eq!(
        client.mod_sources(),
        vec![ModSourceInfo {
            id: "gain_reduction".into(),
            name: "Gain Reduction".into(),
            unit: "dB".into()
        }]
    );
    assert_eq!(client.mod_source_value(0), Some(-4.5));
    assert!(client.set_session(Some(("kit-1", "Kick/In"))));
    assert_eq!(
        *comp.session.lock().unwrap(),
        Some(("kit-1".into(), "Kick/In".into()))
    );
    assert!(client.set_session(None));
    assert_eq!(*comp.session.lock().unwrap(), None);
    drop(unsafe { Box::from_raw(plugin.plugin_data.cast::<Arc<dyn SignalPlugin>>()) });
}

#[test]
fn event_sinks_deliver_to_the_plugins_host_events() {
    static GOT: Mutex<Vec<(u64, String, String)>> = Mutex::new(Vec::new());
    unsafe extern "C" fn on_event(
        _p: *const clap_plugin,
        sub: u64,
        topic: *const c_char,
        json: *const c_char,
    ) {
        let (t, j) = unsafe { (CStr::from_ptr(topic), CStr::from_ptr(json)) };
        GOT.lock()
            .unwrap()
            .push((sub, t.to_string_lossy().into(), j.to_string_lossy().into()));
    }
    static EVENTS: sys::fts_plugin_host_events = sys::fts_plugin_host_events {
        on_event: Some(on_event),
    };
    unsafe extern "C" fn get_ext(_p: *const clap_plugin, id: *const c_char) -> *const c_void {
        if unsafe { CStr::from_ptr(id) } == sys::FTS_EXT_HOST_EVENTS {
            (&raw const EVENTS).cast()
        } else {
            std::ptr::null()
        }
    }
    let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
    plugin.get_extension = Some(get_ext);
    let sink =
        unsafe { EventSink::query(&raw const plugin) }.expect("the plugin has host-events/1");
    sink.send(7, "hits:Drums/Kick", r#"{"time":1.25}"#);
    assert_eq!(
        GOT.lock().unwrap()[0],
        (7, "hits:Drums/Kick".into(), r#"{"time":1.25}"#.into())
    );
}
