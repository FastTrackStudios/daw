//! Audio host selection.
//!
//! All knowledge of *which* audio backend to talk to lives here. Callers ask
//! for [`audio_host`] and get a ready `cpal::Host`; they never name a backend.

use cpal::traits::HostTrait;

/// The audio host.
///
/// Prefers cpal's **native PipeWire** backend (`target_os = "linux"`) — it
/// talks to PipeWire directly (no JACK shim), enumerates real device names +
/// channel counts, targets a specific device via `target.object`, and requests
/// per-node latency natively. Falls back to JACK (pipewire-jack) where the
/// PipeWire backend isn't built, then to the raw ALSA/CoreAudio default host.
pub fn audio_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    {
        if let Ok(h) = cpal::host_from_id(cpal::HostId::PipeWire) {
            tracing::info!("daw-audio-io: using native PipeWire host");
            return h;
        }
        tracing::warn!("daw-audio-io: PipeWire host unavailable; trying JACK/default");
    }
    #[cfg(feature = "jack")]
    {
        match cpal::host_from_id(cpal::HostId::Jack) {
            Ok(h) => {
                tracing::info!("daw-audio-io: using JACK host (pipewire-jack)");
                return h;
            }
            Err(e) => {
                tracing::warn!("daw-audio-io: JACK host unavailable ({e}); using default host");
            }
        }
    }
    cpal::default_host()
}

/// Whether `host` is the JACK backend. Under JACK the server dictates both
/// sample rate and buffer size, so callers must not force either.
#[cfg(feature = "jack")]
pub fn host_is_jack(host: &cpal::Host) -> bool {
    host.id() == cpal::HostId::Jack
}

/// Whether `host` is the JACK backend (always false without the `jack` feature).
#[cfg(not(feature = "jack"))]
pub fn host_is_jack(_host: &cpal::Host) -> bool {
    false
}

/// Whether `host` is cpal's native PipeWire backend. PipeWire honors a
/// requested sample rate and a fixed buffer size (as a per-node latency hint)
/// and targets a specific device, so the channel-count handling matches JACK
/// (open just enough channels) but the rate/buffer come from prefs.
#[cfg(target_os = "linux")]
pub fn host_is_pipewire(host: &cpal::Host) -> bool {
    host.id() == cpal::HostId::PipeWire
}

/// Whether `host` is the native PipeWire backend (always false when the
/// `pipewire` feature is off or off-Linux).
#[cfg(not(target_os = "linux"))]
pub fn host_is_pipewire(_host: &cpal::Host) -> bool {
    false
}

/// Whether the host is server-managed (JACK or PipeWire) — i.e. the graph wires
/// hardware ports for us, so streams open only as many channels as needed to
/// reach the one a track taps, and the rest of the device is reached through the
/// graph rather than a wide single stream.
pub fn host_is_graph(host: &cpal::Host) -> bool {
    host_is_jack(host) || host_is_pipewire(host)
}
