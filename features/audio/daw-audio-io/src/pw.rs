//! Live PipeWire graph + per-device ALSA latency control.
//!
//! Everything that shells out to `pw-metadata` / `pw-cli` / `systemctl` lives
//! here, so callers (e.g. the live-rig UI) express latency *intent* — a buffer
//! size, a sample rate, a device's headroom/period — and never touch PipeWire
//! directly. Two tiers:
//!
//! - **Live** (global clock, applied instantly): graph quantum
//!   ([`force_quantum`]) and sample rate ([`force_rate`], limited to the
//!   server's `clock.allowed-rates`).
//! - **Device** (per-interface ALSA buffering, NOT live): `period-size`,
//!   `period-num`, `headroom`. WirePlumber owns these and only applies them when
//!   it creates the device node, so [`write_device_latency`] persists a
//!   `monitor.alsa.rules` drop-in and [`restart_session_manager`] re-creates the
//!   nodes to pick it up (a brief audio drop — callers re-open afterwards).
//!
//! All functions are best-effort: on a non-PipeWire system the tools are absent,
//! so the spawns fail and the calls are no-ops / return empty — no `cfg` needed.

use std::process::{Command, Stdio};

/// A `Command` with stdio silenced — these tools (`pw-metadata`, `systemctl`)
/// chatter to stdout/stderr, which would corrupt a TUI that owns the terminal.
fn quiet(program: &str) -> Command {
    let mut c = Command::new(program);
    c.stdout(Stdio::null()).stderr(Stdio::null());
    c
}

// ── Live global clock (pw-metadata) ──────────────────────────────────────────

/// Force the graph quantum (live, graph-wide buffer size in frames). `0` clears
/// the force, returning the graph to automatic quantum.
pub fn force_quantum(frames: u32) {
    set_clock("clock.force-quantum", frames);
}

/// Force the graph sample rate (live). Must be one of the server's
/// `clock.allowed-rates` or PipeWire ignores it; `0` clears the force. Pair with
/// re-opening streams at the new rate.
pub fn force_rate(rate: u32) {
    set_clock("clock.force-rate", rate);
}

fn set_clock(key: &str, value: u32) {
    let _ = quiet("pw-metadata")
        .args(["-n", "settings", "0", key, &value.to_string()])
        .status();
}

/// Global graph clock settings, read from `pw-metadata -n settings`.
#[derive(Clone, Debug, Default)]
pub struct ClockSettings {
    /// Current graph rate (`clock.rate`).
    pub rate: u32,
    /// Current graph quantum (`clock.quantum`).
    pub quantum: u32,
    /// Forced rate, or `0` when automatic (`clock.force-rate`).
    pub force_rate: u32,
    /// Forced quantum, or `0` when automatic (`clock.force-quantum`).
    pub force_quantum: u32,
    /// Rates the server permits forcing (`clock.allowed-rates`).
    pub allowed_rates: Vec<u32>,
    pub min_quantum: u32,
    pub max_quantum: u32,
}

/// Read the live graph clock settings (empty/zero fields when `pw-metadata`
/// isn't available).
pub fn clock_settings() -> ClockSettings {
    let out = Command::new("pw-metadata")
        .args(["-n", "settings"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut s = ClockSettings::default();
    // Lines look like: update: id:0 key:'clock.rate' value:'48000' type:''
    for line in out.lines() {
        let Some((key, value)) = parse_metadata_line(line) else {
            continue;
        };
        match key {
            "clock.rate" => s.rate = value.parse().unwrap_or(0),
            "clock.quantum" => s.quantum = value.parse().unwrap_or(0),
            "clock.force-rate" => s.force_rate = value.parse().unwrap_or(0),
            "clock.force-quantum" => s.force_quantum = value.parse().unwrap_or(0),
            "clock.min-quantum" => s.min_quantum = value.parse().unwrap_or(0),
            "clock.max-quantum" => s.max_quantum = value.parse().unwrap_or(0),
            "clock.allowed-rates" => s.allowed_rates = parse_rate_list(value),
            _ => {}
        }
    }
    s
}

/// Extract `(key, value)` from a `pw-metadata` line, stripping the `'…'` quotes.
fn parse_metadata_line(line: &str) -> Option<(&str, &str)> {
    let key = line.split("key:'").nth(1)?.split('\'').next()?;
    let value = line.split("value:'").nth(1)?.split('\'').next()?;
    Some((key, value))
}

/// Parse a `[ 48000, 96000 ]`-style rate list.
fn parse_rate_list(s: &str) -> Vec<u32> {
    s.trim_matches(|c| c == '[' || c == ']' || c == ' ')
        .split([',', ' '])
        .filter_map(|t| t.trim().parse().ok())
        .collect()
}

// ── Per-device ALSA buffering (pw-cli read + WirePlumber config write) ────────

/// One audio interface's ALSA buffering parameters, read from its PipeWire node.
#[derive(Clone, Debug, Default)]
pub struct DeviceLatency {
    pub node_id: u32,
    pub node_name: String,
    /// ALSA period size in frames (`api.alsa.period-size`).
    pub period_size: u32,
    /// Number of periods the device buffers (`api.alsa.period-num`).
    pub period_num: u32,
    /// Extra capture/playback headroom in frames (`api.alsa.headroom`).
    pub headroom: u32,
}

/// Read the ALSA buffering params of the device whose `node.name` contains
/// `name_match` in the requested direction (`capture` → input, else output).
/// Returns `None` when no such node is live (device absent / not opened).
pub fn device_latency(name_match: &str, capture: bool) -> Option<DeviceLatency> {
    let id = device_node_id(name_match, capture)?;
    let info = Command::new("pw-cli")
        .args(["i", &id.to_string()])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut d = DeviceLatency {
        node_id: id,
        ..Default::default()
    };
    for line in info.lines() {
        let l = line.trim_start_matches(['*', ' ', '\t']);
        if let Some(v) = prop_after(l, "api.alsa.period-size") {
            d.period_size = v;
        } else if let Some(v) = prop_after(l, "api.alsa.period-num") {
            d.period_num = v;
        } else if let Some(v) = prop_after(l, "api.alsa.headroom") {
            d.headroom = v;
        } else if l.starts_with("node.name = ") {
            d.node_name = l
                .trim_start_matches("node.name = ")
                .trim_matches('"')
                .to_string();
        }
    }
    Some(d)
}

/// Parse `key = "123"` (pw-cli info format), returning the integer value.
fn prop_after(line: &str, key: &str) -> Option<u32> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    rest.trim_matches('"').parse().ok()
}

/// Find the live PipeWire node id for the ALSA device matching `name_match` in
/// the requested direction. Capture nodes are `alsa_input.*`, playback
/// `alsa_output.*`.
fn device_node_id(name_match: &str, capture: bool) -> Option<u32> {
    let listing = Command::new("pw-cli")
        .args(["ls", "Node"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let prefix = if capture {
        "alsa_input."
    } else {
        "alsa_output."
    };
    // cpal device names use spaces ("Yamaha TF"); PipeWire node.name uses
    // underscores ("…Yamaha_TF…"). Match on the underscore form.
    let token = node_token(name_match);
    let mut id: Option<u32> = None;
    for line in listing.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("id ") {
            id = rest.split(',').next().and_then(|s| s.trim().parse().ok());
        } else if t.starts_with("node.name = ") {
            let name = t.trim_start_matches("node.name = ").trim_matches('"');
            if name.starts_with(prefix) && name.contains(&token) {
                return id;
            }
        }
    }
    None
}

/// The `node.name` token for a cpal device name (spaces → underscores), used to
/// match the PipeWire ALSA node both when reading and in the WirePlumber rule.
fn node_token(device_name: &str) -> String {
    device_name.replace(' ', "_")
}

/// The full `node.name` of the live ALSA device matching `name_match` in the
/// requested direction (`capture` → `alsa_input.*`, else `alsa_output.*`).
/// `None` when no such node is present. Used to build `pw-link` endpoints.
pub fn device_node_name(name_match: &str, capture: bool) -> Option<String> {
    let listing = Command::new("pw-cli")
        .args(["ls", "Node"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let prefix = if capture {
        "alsa_input."
    } else {
        "alsa_output."
    };
    let token = node_token(name_match);
    for line in listing.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix("node.name = ") {
            let name = name.trim_matches('"');
            if name.starts_with(prefix) && name.contains(&token) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Link one PipeWire port to another (`"node:port"` endpoints), best-effort.
/// Returns whether the link command reported success.
pub fn link(src: &str, dst: &str) -> bool {
    Command::new("pw-link")
        .args([src, dst])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Outcome of an [`ensure_link`] pass over one port pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStatus {
    /// The link did not exist and was created.
    Created,
    /// The link already existed (`pw-link` reports "File exists").
    Exists,
    /// The link could not be made (port/node absent, PipeWire down, …).
    Failed,
}

/// Idempotently link `src` → `dst`, reporting whether the link was newly
/// created, already in place, or impossible. `pw-link` exits non-zero for
/// both "already linked" and real failures, so stderr disambiguates — this
/// is what lets a watchdog distinguish a healthy graph from a vanished
/// device using the same cheap enumeration as [`link`].
pub fn ensure_link(src: &str, dst: &str) -> LinkStatus {
    let out = Command::new("pw-link")
        .args([src, dst])
        .stdout(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => LinkStatus::Created,
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).to_lowercase();
            if err.contains("exist") {
                LinkStatus::Exists
            } else {
                LinkStatus::Failed
            }
        }
        Err(_) => LinkStatus::Failed,
    }
}

/// Path of the daw-managed WirePlumber drop-in. Named to sort *after* any
/// hand-written `99-*` rule so the rig's chosen values win.
fn device_rule_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("wireplumber/wireplumber.conf.d/99-zz-daw-rig-latency.conf")
}

/// Persist a `monitor.alsa.rules` drop-in setting `period_size` / `period_num` /
/// `headroom` for every ALSA node whose name matches `name_match`. The values
/// take effect when WirePlumber next creates the device node — call
/// [`restart_session_manager`] to apply now. A zero `period_*` is omitted (keep
/// the device default). Returns the written file path.
pub fn write_device_latency(
    name_match: &str,
    period_size: u32,
    period_num: u32,
    headroom: u32,
) -> std::io::Result<std::path::PathBuf> {
    let mut props = Vec::new();
    if period_size > 0 {
        props.push(format!("api.alsa.period-size = {period_size}"));
    }
    if period_num > 0 {
        props.push(format!("api.alsa.period-num = {period_num}"));
    }
    props.push(format!("api.alsa.headroom = {headroom}"));
    let body = format!(
        "# Written by daw-audio-io (live-rig latency tuning). Edit via the rig UI.\n\
         monitor.alsa.rules = [\n  {{ matches = [ {{ node.name = \"~alsa_.*{m}.*\" }} ]\n    \
         actions = {{ update-props = {{ {props} }} }} }}\n]\n",
        m = regex_escape(&node_token(name_match)),
        props = props.join(", "),
    );
    let path = device_rule_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Escape regex metacharacters in a device name for the `~`-prefixed WirePlumber
/// match (the interface name can contain `.`, `+`, etc.).
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Restart the WirePlumber user service so device nodes are re-created with the
/// latest `monitor.alsa.rules`. Briefly drops audio — callers re-open after.
/// Returns whether the restart command succeeded.
pub fn restart_session_manager() -> bool {
    quiet("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
