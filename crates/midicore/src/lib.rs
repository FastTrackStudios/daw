//! `midicore` facade — re-exports the wire types + service traits from
//! `midicore-proto`. Concrete I/O backends (midir, ...) plug in as features.
pub use midicore_proto::*;
