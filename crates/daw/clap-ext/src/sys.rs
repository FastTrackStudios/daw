//! `#[repr(C)]` mirrors of `include/fts/clap-ext.h`, over clap-sys.

#![allow(non_camel_case_types)]

use std::ffi::{CStr, c_char};

use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::clap_ostream;
use clap_sys::string_sizes::CLAP_NAME_SIZE;

pub const FTS_EXT_HOST_ACTIONS: &CStr = c"com.fasttrackstudio.host-actions/1";
pub const FTS_EXT_HOST_EVENTS: &CStr = c"com.fasttrackstudio.host-events/1";
pub const FTS_EXT_SIGNAL: &CStr = c"com.fasttrackstudio.signal/1";
pub const FTS_EXT_HOST_AUTOMATION: &CStr = c"com.fasttrackstudio.host-automation/1";
/// The `host-events/1` topic a user's edit of the plugin's lanes arrives on.
pub const FTS_AUTOMATION_TOPIC: &str = "automation";

pub const FTS_AUTOMATION_CREATE_LANE: u32 = 1 << 0;
pub const FTS_AUTOMATION_SHOW_LANE: u32 = 1 << 1;
pub const FTS_AUTOMATION_KEEP_USER_EDITS: u32 = 1 << 2;

/// `fts_host_automation_t`: the host's side, which the plugin calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_host_automation {
    pub lanes:
        Option<unsafe extern "C" fn(host: *const clap_host, out_json: *const clap_ostream) -> bool>,
    pub read: Option<
        unsafe extern "C" fn(
            host: *const clap_host,
            param_id: clap_id,
            from: f64,
            to: f64,
            out_json: *const clap_ostream,
        ) -> bool,
    >,
    pub write: Option<
        unsafe extern "C" fn(
            host: *const clap_host,
            param_id: clap_id,
            from: f64,
            to: f64,
            points_json: *const c_char,
            flags: u32,
            out_json: *const clap_ostream,
        ) -> bool,
    >,
    pub clear: Option<
        unsafe extern "C" fn(host: *const clap_host, param_id: clap_id, from: f64, to: f64) -> bool,
    >,
    pub begin_edit:
        Option<unsafe extern "C" fn(host: *const clap_host, label: *const c_char) -> bool>,
    pub end_edit: Option<unsafe extern "C" fn(host: *const clap_host) -> bool>,
}

/// `fts_host_actions_t`: the host's side, which the plugin calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_host_actions {
    pub list:
        Option<unsafe extern "C" fn(host: *const clap_host, out_json: *const clap_ostream) -> bool>,
    pub describe: Option<
        unsafe extern "C" fn(
            host: *const clap_host,
            action_id: *const c_char,
            out_json: *const clap_ostream,
        ) -> bool,
    >,
    pub run: Option<
        unsafe extern "C" fn(
            host: *const clap_host,
            action_id: *const c_char,
            args_json: *const c_char,
            out_json: *const clap_ostream,
        ) -> bool,
    >,
    pub subscribe:
        Option<unsafe extern "C" fn(host: *const clap_host, topic: *const c_char) -> u64>,
    pub unsubscribe: Option<unsafe extern "C" fn(host: *const clap_host, subscription: u64)>,
    pub load_file: Option<
        unsafe extern "C" fn(
            host: *const clap_host,
            reference: *const c_char,
            blake3_or_null: *const c_char,
            out_bytes: *const clap_ostream,
        ) -> bool,
    >,
}

/// `fts_plugin_host_events_t`: the plugin's half of host-actions.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_plugin_host_events {
    pub on_event: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            subscription: u64,
            topic: *const c_char,
            event_json: *const c_char,
        ),
    >,
}

/// `fts_signal_mod_source_info_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_signal_mod_source_info {
    pub id: [c_char; CLAP_NAME_SIZE],
    pub name: [c_char; CLAP_NAME_SIZE],
    pub unit: [c_char; 16],
}

/// `fts_plugin_signal_t`: the plugin's side, which the host calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_plugin_signal {
    pub list_actions: Option<
        unsafe extern "C" fn(plugin: *const clap_plugin, out_json: *const clap_ostream) -> bool,
    >,
    pub run_action: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            action_id: *const c_char,
            args_json: *const c_char,
            out_json: *const clap_ostream,
        ) -> bool,
    >,
    pub chain: Option<
        unsafe extern "C" fn(plugin: *const clap_plugin, out_json: *const clap_ostream) -> bool,
    >,
    pub revision: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> u64>,
    pub param_stable_id: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            param_id: clap_id,
            out: *mut c_char,
            out_capacity: u32,
        ) -> bool,
    >,
    pub mod_source_count: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> u32>,
    pub mod_source_info: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            index: u32,
            info: *mut fts_signal_mod_source_info,
        ) -> bool,
    >,
    pub mod_source_value:
        Option<unsafe extern "C" fn(plugin: *const clap_plugin, index: u32) -> f64>,
    pub set_session: Option<
        unsafe extern "C" fn(
            plugin: *const clap_plugin,
            session_id: *const c_char,
            port: *const c_char,
        ) -> bool,
    >,
}
