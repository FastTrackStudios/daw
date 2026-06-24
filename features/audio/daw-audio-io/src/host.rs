//! Audio host selection.

use cpal::traits::HostTrait;

/// The audio host. With the `jack` feature this prefers JACK (pipewire-jack on a
/// PipeWire system), so the engine connects through PipeWire instead of grabbing
/// a raw ALSA device that PipeWire already holds. Falls back to the default host.
pub fn audio_host() -> cpal::Host {
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

/// Whether `host` is the JACK backend (always false without the `jack` feature).
/// Under JACK the server dictates sample rate + buffer, so callers must not try
/// to force a fixed rate/buffer.
#[cfg(feature = "jack")]
pub fn host_is_jack(host: &cpal::Host) -> bool {
    host.id() == cpal::HostId::Jack
}

/// Whether `host` is the JACK backend (always false without the `jack` feature).
#[cfg(not(feature = "jack"))]
pub fn host_is_jack(_host: &cpal::Host) -> bool {
    false
}
