//! Port description + selection types, and the timestamped event that streams
//! across the I/O boundary. All derive [`facet::Facet`] so they ride the vox
//! wire that `#[architect::rpc]` generates.

use facet::Facet;

use crate::event::MidiEvent;

/// A stable-ish identifier for a physical or virtual port, as the backend
/// reports it. Opaque to `midicore`; adapters define its meaning.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Facet)]
pub struct PortId(pub String);

/// Whether a port carries data in or out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum Direction {
    Input,
    Output,
}

/// What a backend knows about a port before you open it.
#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct PortInfo {
    pub id: PortId,
    pub name: String,
    pub direction: Direction,
    /// True if the backend created this port (a virtual endpoint), rather than
    /// it belonging to external hardware/software.
    pub virtual_port: bool,
}

/// How the caller chooses a port to open — the ergonomic front door. Mirrors
/// the device/all/virtual selection real rigs need.
#[derive(Clone, Debug, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum PortSelector {
    /// The backend's default/first port for the direction.
    Default,
    /// Exact [`PortId`] match.
    Id(PortId),
    /// First port whose name contains this substring (case-insensitive).
    NameContains(String),
    /// Merge all ports of the direction into one stream (inputs) / fan out to
    /// all (outputs).
    All,
    /// Create a virtual port with this name (backends that support it).
    Virtual(String),
}

/// A MIDI event tagged with the backend's capture timestamp (microseconds,
/// monotonic; epoch is backend-defined). This is the streamed unit.
#[derive(Clone, Debug, PartialEq, Eq, Facet)]
pub struct TimedEvent {
    pub timestamp_us: u64,
    pub event: MidiEvent,
}

/// Errors an I/O backend can surface. Adapters map their native errors onto
/// these; `Other` carries an adapter-specific message.
#[derive(Clone, Debug, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum MidiIoError {
    /// No port matched the [`PortSelector`].
    PortNotFound,
    /// The backend does not support the requested operation (e.g. virtual
    /// ports on a platform that lacks them).
    Unsupported,
    /// The connection was closed or the device disappeared.
    Disconnected,
    /// Adapter-specific failure.
    Other(String),
}
