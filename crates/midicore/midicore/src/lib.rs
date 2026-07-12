//! `midicore` — the single app-facing MIDI facade.
//!
//! Depend on **this crate only**. It re-exports the wire types + byte codec +
//! `#[architect::rpc]` service traits from `midicore-proto`, and — behind the
//! `midir` feature — the concrete OS backend as [`midir`]. Nothing outside the
//! midicore workspace should name `midicore-proto` or `midicore-midir`
//! directly.
//!
//! ```ignore
//! midicore = { version = "0.1", features = ["midir"] }   // in Cargo.toml
//! use midicore::{MidiEvent, PortSelector, midir::MidiInput};
//! ```

pub use midicore_proto::*;

mod monitor;
pub use monitor::{MidiMonitor, MIDI_MONITOR_CAP};

/// The midir-backed OS MIDI backend (`MidiInput`, `MidiStream`, `input_ports`,
/// …). Enabled by the `midir` feature; native platforms only.
#[cfg(feature = "midir")]
pub mod midir {
    pub use midicore_midir::*;
}
