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

// ── signal-revisions/1 ──────────────────────────────────────────────────

pub const FTS_EXT_SIGNAL_REVISIONS: &CStr = c"com.fasttrackstudio.signal-revisions/1";

/// `fts_plugin_signal_revisions_t`: Signal's revisions, from any thread.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_plugin_signal_revisions {
    pub revision: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> u64>,
    pub values_revision: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> u64>,
}

// ── host-timeline/1 ─────────────────────────────────────────────────────

pub const FTS_EXT_HOST_TIMELINE: &CStr = c"com.fasttrackstudio.host-timeline/1";

/// `FTS_TIMELINE_MAGIC`: "FTL1", little-endian.
pub const FTS_TIMELINE_MAGIC: u32 = 0x314C_5446;

pub const FTS_TIMELINE_CC: u8 = 0;
pub const FTS_TIMELINE_PITCH_BEND: u8 = 1;
pub const FTS_TIMELINE_CHANNEL_PRESSURE: u8 = 2;
pub const FTS_TIMELINE_POLY_PRESSURE: u8 = 3;
pub const FTS_TIMELINE_PROGRAM: u8 = 4;

/// `FTS_TIMELINE_TEMPO_RAMP`.
pub const FTS_TIMELINE_TEMPO_RAMP: u32 = 1;

/// `fts_timeline_note_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct fts_timeline_note {
    pub start_qn: f64,
    pub end_qn: f64,
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    pub release_velocity: u8,
    pub port: u16,
    pub reserved: u16,
}

/// `fts_timeline_ctrl_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct fts_timeline_ctrl {
    pub qn: f64,
    pub kind: u8,
    pub channel: u8,
    pub number: u8,
    pub reserved: u8,
    pub value: u16,
    pub port: u16,
}

/// `fts_timeline_tempo_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct fts_timeline_tempo {
    pub qn: f64,
    pub seconds: f64,
    pub bpm: f64,
    pub numerator: u16,
    pub denominator: u16,
    pub flags: u32,
}

/// `fts_timeline_header_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct fts_timeline_header {
    pub magic: u32,
    pub header_size: u32,
    pub revision: u64,
    pub note_count: u32,
    pub ctrl_count: u32,
    pub tempo_count: u32,
    pub note_size: u32,
    pub ctrl_size: u32,
    pub tempo_size: u32,
    pub end_qn: f64,
}

/// `fts_host_timeline_t`: the host's side, which the plugin calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct fts_host_timeline {
    pub revision: Option<unsafe extern "C" fn(host: *const clap_host) -> u64>,
    pub read:
        Option<unsafe extern "C" fn(host: *const clap_host, out: *const clap_ostream) -> bool>,
    pub set_playback_claimed:
        Option<unsafe extern "C" fn(host: *const clap_host, claimed: bool) -> bool>,
}

// The header's sizes, held at compile time.
const _: () = assert!(std::mem::size_of::<fts_timeline_note>() == 24);
const _: () = assert!(std::mem::size_of::<fts_timeline_ctrl>() == 16);
const _: () = assert!(std::mem::size_of::<fts_timeline_tempo>() == 32);
const _: () = assert!(std::mem::size_of::<fts_timeline_header>() == 48);
