//! FTS Modulation — reusable MSEG pattern engine with tempo sync and triggers.
//!
//! Ported from tiagolr's plugin suite (gate12, filtr, time12, reevr).
//! Pattern/curve system based on KottV/SimpleSide (SSCurve).
//!
//! This crate provides the shared modulation infrastructure used by
//! gate, filter, delay, reverb, and any future modulation-driven plugins.
//!
//! # Architecture
//!
//! ```text
//! Transport/MIDI/Audio ──→ TriggerEngine ──→ phase (0..1)
//!                                              │
//!                                              ▼
//!                                        Pattern::get_y(phase) ──→ raw 0..1
//!                                              │
//!                                              ▼
//!                                      RcSmoother::tick() ──→ smoothed 0..1
//!                                              │
//!                                              ▼
//!                                     Plugin-specific mapping
//!                                  (gain, cutoff, delay time, etc.)
//! ```

pub mod curves;
pub mod modulator;
pub mod pattern;
pub mod smoother;
pub mod tempo;
pub mod transient;
pub mod trigger;
