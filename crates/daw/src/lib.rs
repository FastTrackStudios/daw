//! Unified facade for DAW interaction.
//!
//! This is the single public API surface for the `daw` domain. External consumers
//! should depend only on this crate — never on internal crates directly.
//!
//! # Core (always available, WASM-compatible)
//!
//! - **Root**: High-level control API — `Daw`, `Project`, `Track`, `FxChain`,
//!   `Transport`, etc.
//! - **`service`**: Raw protocol types and service clients.
//!
//! # Feature-gated modules
//!
//! - **`sync`** → `daw::sync` — Blocking wrapper for audio plugins (`DawSync`, `LocalCaller`).
//! - **`reaper`** → `daw::reaper` — REAPER-specific implementations.
//! - **`standalone`** → `daw::standalone` — Reference implementation for testing.
//! - **`file`** → `daw::file` — RPP file format parser.

// ── Core: high-level control API (WASM-compatible) ──────────────────────────
pub use daw_control::*;

// ── Service: raw protocol types & service clients ───────────────────────────
/// Raw protocol types and service clients.
pub mod service {
    pub use daw_proto::*;
}

// ── Sync: blocking wrapper for audio plugins ────────────────────────────────
#[cfg(feature = "sync")]
/// Synchronous (blocking) DAW control API for real-time audio contexts.
pub mod sync {
    pub use daw_control_sync::*;
}

// ── Reaper: REAPER-specific implementations ─────────────────────────────────
#[cfg(feature = "reaper")]
/// REAPER DAW implementation — in-process service dispatchers.
pub mod reaper {
    pub use daw_reaper::*;
}

// ── Standalone: reference/mock implementation ───────────────────────────────
#[cfg(feature = "standalone")]
/// Standalone reference implementation for testing (mock data included).
pub mod standalone {
    pub use daw_standalone::*;
}

// ── File: RPP file format parser ────────────────────────────────────────────
#[cfg(feature = "file")]
/// High-performance RPP (REAPER Project) file format parser.
pub mod file {
    pub use dawfile_reaper::*;
}
