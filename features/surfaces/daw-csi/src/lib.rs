//! # daw-csi — Control Surface Integration
//!
//! CSI-style hardware control surface support for the daw, named in
//! homage to [CSICode](https://github.com/FunkybotsEvilTwin/CSICode)
//! (REAPER's Control Surface Integration, which doesn't run on
//! Linux — this does). v1 targets the Behringer X-Touch in Mackie
//! Control mode; the MCU codec covers any MCU-compatible surface.
//!
//! Architecture (vs CSI): CSI polls REAPER ~50×/sec because REAPER
//! has no push API. Here the daw **event bus** pushes every state
//! change, so the driver is event-driven end to end:
//!
//! ```text
//!  X-Touch ──MIDI──► mcu::decode ──► DriverState ──Intent──► daw-control
//!     ▲                                  ▲                  (Tracks/Transport)
//!     │                                  │
//!     └──MIDI◄── Shadow diff ◄── render ◄┴── event bus (Rx<DawEvent>)
//! ```
//!
//! - [`mcu`] — Mackie Control wire codec + X-Touch extensions
//! - [`navigator`] — banking + CSI-style folder drill-down
//! - [`shadow`] — surface output diffing (motors don't twitch)
//! - [`driver`] — the event loop; [`driver::run`] is the entry point
//!
//! ## Usage
//!
//! ```ignore
//! let daw: daw_control::Daw = /* in-process or RPC connection */;
//! moire::task::spawn(daw_csi::run(daw, daw_csi::CsiConfig::default()));
//! ```
//!
//! Known v1 gaps: no meter bridge (Peaks isn't in the served layer
//! set yet), no FX-parameter pages (FX param events aren't streamed),
//! no timecode display.

pub mod mcu;
pub mod navigator;
pub mod shadow;
pub mod taper;

#[cfg(feature = "midi-hardware")]
pub mod driver;
#[cfg(feature = "midi-hardware")]
pub mod midi;

#[cfg(feature = "midi-hardware")]
pub use driver::{CsiConfig, run};
