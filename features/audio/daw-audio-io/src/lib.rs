//! Reusable audio I/O for the DAW engine.
//!
//! This crate owns the platform audio-device plumbing that every part of the
//! DAW (and downstream consumers like Signal's guitar rig) shares:
//!
//! - [`host`] — pick the audio host (JACK/PipeWire when the `jack` feature is on,
//!   else the platform default).
//! - [`device`] — enumerate input/output devices ([`DeviceInfo`]) and resolve a
//!   device by name substring ([`pick_device`]).
//! - [`prefs`] — the persisted **engine-level** device selection
//!   ([`AudioIoPrefs`]: which input device, which output device, sample rate,
//!   buffer) plus **track-level** routing ([`TrackIo`]: which input channel a
//!   track records from and which output channels it sums into).
//! - [`open`] — turn an [`AudioIoPrefs`] into opened cpal devices + a resolved
//!   [`cpal::StreamConfig`], honoring JACK's sample-rate/buffer forcing. This is
//!   the single place the engine's "open the chosen device" logic lives.
//! - [`monitor`] (feature `monitor`) — a live duplex [`DuplexMonitor`]: cpal
//!   input → a caller-supplied [`MonitorProcess`] closure → cpal output. The
//!   processing (e.g. a guitar amp/FX chain) stays with the caller; this crate
//!   only owns the device streams, the lock-free input ring, and the meters.
//!
//! ## Routing model
//!
//! Two levels, matching a DAW: the **engine** chooses one input device and one
//! output device ([`AudioIoPrefs`]); each **track** then taps any input channel
//! of that input device and routes to any output channel of that output device
//! ([`TrackIo`]). The duplex monitor implements the single-track case.

pub mod device;
pub mod duplex;
pub mod host;
pub mod open;
pub mod prefs;
pub mod pw;

/// Native PipeWire `pw_filter` duplex backend (Linux, `pipewire` feature).
#[cfg(all(feature = "pipewire", target_os = "linux"))]
pub mod duplex_pw;

#[cfg(feature = "monitor")]
pub mod monitor;

pub use device::{
    DeviceInfo, device_name, input_channels, input_devices, output_devices, pick_device,
};
pub use host::{audio_host, host_is_graph, host_is_jack, host_is_pipewire};
pub use open::{OpenedInput, OpenedOutput, open_input, open_output};
pub use prefs::{AudioIoPrefs, TrackIo};

#[cfg(feature = "monitor")]
pub use monitor::{DuplexMonitor, MonitorProcess};
