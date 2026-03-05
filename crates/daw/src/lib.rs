//! Unified facade for DAW interaction.
//!
//! Re-exports the high-level control API (`daw-control`) at the crate root and
//! makes the raw protocol types available under `daw::proto`.

pub use daw_control::*;

/// Raw protocol types and service clients from `daw-proto`.
pub mod proto {
    pub use daw_proto::*;
}
