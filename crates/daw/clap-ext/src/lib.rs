//! FastTrackStudio's CLAP extensions (signal#26): the C header
//! (`include/fts/clap-ext.h`) and its Rust side.
//!
//! | id | direction | who implements it |
//! |---|---|---|
//! | `com.fasttrackstudio.host-actions/1` | the plugin calls the host: actions (JSON in, JSON out), events, files | Studio's engine over its Action List ([`host::HostActions`]) |
//! | `com.fasttrackstudio.host-events/1` | the host delivers subscribed events to the plugin | the plugin |
//! | `com.fasttrackstudio.signal/1` | the host drives the plugin: actions, the chain as data, stable parameter ids, modulation sources | the Signal CLAP and each block's CLAP ([`plugin::SignalPlugin`]) |
//! | `com.fasttrackstudio.host-automation/1` | the plugin reads and writes the host's lanes of its own parameters | Studio's engine ([`automation::HostAutomation`]) |
//! | `com.fasttrackstudio.host-timeline/1` | the plugin reads its track's whole MIDI and tempo map ahead of the playhead, and may claim its playback ([`timeline`], `docs/host-timeline.md`) | Studio's engine per instance ([`host::HostTimeline`]); read by [`plugin::HostTimelineClient`] |
//!
//! - [`sys`]: the header's `#[repr(C)]` types.
//! - [`plugin`]: the plugin's side, over clap-sys: [`plugin::SignalVtable`]
//!   answers `signal/1` (nice-plug's `ClapPlugin::clap_extension` returns
//!   it), [`plugin::HostActionsClient`] calls the host.
//! - [`host`]: the host's side: [`host::HostActionsVtable`] answers
//!   `host-actions/1`, [`host::SignalPluginClient`] calls a plugin.
//!
//! Nothing here loads a library or depends on a host framework: daw's clack
//! host and nice-plug plug these vtables in.

pub mod automation;
pub mod host;
pub mod plugin;
pub mod stream;
pub mod sys;
pub mod timeline;

use std::fmt;

/// A modulation source a plugin publishes over `signal/1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModSourceInfo {
    /// Stable id: `gain_reduction`.
    pub id: String,
    pub name: String,
    /// `dB`, `%`, `Hz`, or empty.
    pub unit: String,
}

/// Why a call across the boundary failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtError {
    /// The other side doesn't implement this.
    Unsupported,
    NotFound(String),
    Failed(String),
}

impl fmt::Display for ExtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("not supported"),
            Self::NotFound(what) => write!(f, "not found: {what}"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ExtError {}

impl ExtError {
    /// As the JSON the other side sends: `{"error": "..."}`.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!("{{\"error\":{}}}", json_string(&self.to_string()))
    }

    /// From a failed call's JSON (`{"error": "..."}`), or `fallback`.
    #[must_use]
    pub fn from_json_or(json: &str, fallback: &str) -> Self {
        let why = json
            .trim()
            .strip_prefix("{\"error\":")
            .and_then(|r| r.strip_suffix('}'))
            .and_then(|s| json_unstring(s.trim()));
        Self::Failed(
            why.filter(|w| !w.is_empty())
                .unwrap_or_else(|| fallback.into()),
        )
    }
}

/// A JSON string literal's contents (`"a\\"b"` → `a"b`); `None` if `s`
/// isn't exactly one string literal.
fn json_unstring(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            return None; // an unescaped quote: not one literal
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// `s` as a JSON string literal.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_round_trip_through_json() {
        for why in [
            "plain",
            r#"unknown "SIGNAL_X""#,
            "back\\slash",
            "line\nbreak",
            "tab\tand\u{1}ctl",
            "",
        ] {
            let e = ExtError::Failed(why.into());
            let back = ExtError::from_json_or(&e.to_json(), "fallback");
            let want = if why.is_empty() { "fallback" } else { why };
            assert_eq!(
                back,
                ExtError::Failed(want.into()),
                "{why:?} -> {}",
                e.to_json()
            );
        }
        assert_eq!(
            ExtError::from_json_or("not json", "fb"),
            ExtError::Failed("fb".into())
        );
    }
}
