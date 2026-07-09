//! Audio device enumeration + resolution.

use cpal::traits::{DeviceTrait, HostTrait};
use facet::Facet;

/// An enumerated audio device — name, channel count, native sample rate, and
/// whether it is an input or output device. One `Vec<DeviceInfo>` can carry both
/// kinds for a combined picker.
#[derive(Clone, Debug, Facet)]
pub struct DeviceInfo {
    pub name: String,
    pub channels: u16,
    pub default_sample_rate: u32,
    pub is_input: bool,
}

/// Human-readable device name (empty string if the driver won't report one).
pub fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default()
}

/// List the host's input devices (name + channel count + native rate). Does not
/// open any stream.
pub fn input_devices(host: &cpal::Host) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    if let Ok(devs) = host.input_devices() {
        for d in devs {
            let (channels, sr) = d
                .default_input_config()
                .map(|c| (c.channels(), c.sample_rate()))
                .unwrap_or((0, 0));
            out.push(DeviceInfo {
                name: device_name(&d),
                channels,
                default_sample_rate: sr,
                is_input: true,
            });
        }
    }
    out
}

/// List the host's output devices.
pub fn output_devices(host: &cpal::Host) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            let (channels, sr) = d
                .default_output_config()
                .map(|c| (c.channels(), c.sample_rate()))
                .unwrap_or((0, 0));
            out.push(DeviceInfo {
                name: device_name(&d),
                channels,
                default_sample_rate: sr,
                is_input: false,
            });
        }
    }
    out
}

/// Synthesized input-channel names for the input device matching `name_substr`
/// (or the default input device when `None`). cpal exposes no per-channel names,
/// so this returns `(index, "Input N")` up to the device's channel count — the
/// list a track's input picker chooses from.
pub fn input_channels(host: &cpal::Host, name_substr: Option<&str>) -> Vec<(u32, String)> {
    let dev = match pick_device(host, name_substr, true) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let channels = dev
        .default_input_config()
        .map(|c| c.channels())
        .or_else(|_| {
            dev.supported_input_configs()
                .map(|cfgs| cfgs.map(|c| c.channels()).max().unwrap_or(0))
        })
        .unwrap_or(0);
    (0..channels as u32)
        .map(|i| (i, format!("Input {}", i + 1)))
        .collect()
}

/// Rank a device for the requested direction so [`pick_device`] can choose
/// between same-named nodes. A device that *only* faces the requested direction
/// (a pure capture source for input, a pure sink for output) outranks a Duplex
/// node by a wide margin; within a rank, more channels wins. The channel count
/// comes from the matching default config (0 when the driver won't report one).
fn device_score(device: &cpal::Device, input: bool) -> u32 {
    use cpal::traits::DeviceTrait;
    let dir_bonus = device
        .description()
        .ok()
        .map(|d| match d.direction() {
            cpal::DeviceDirection::Input if input => 1000,
            cpal::DeviceDirection::Output if !input => 1000,
            cpal::DeviceDirection::Duplex => 100,
            _ => 0,
        })
        .unwrap_or(0);
    let channels = if input {
        device
            .default_input_config()
            .map(|c| c.channels())
            .unwrap_or(0)
    } else {
        device
            .default_output_config()
            .map(|c| c.channels())
            .unwrap_or(0)
    };
    dir_bonus + channels as u32
}

/// Pick an input/output device by name substring, falling back to the host
/// default when the name is empty or not found (e.g. under JACK, where the
/// device is the JACK server, not "Yamaha TF").
pub fn pick_device(
    host: &cpal::Host,
    name: Option<&str>,
    input: bool,
) -> Result<cpal::Device, String> {
    if let Some(n) = name.filter(|s| !s.is_empty()) {
        // A pro interface can surface under one name as several nodes — e.g. a
        // PipeWire `Yamaha TF` appears as both a Duplex *sink* (whose monitor is
        // playback, not the mic inputs) and an Input *source* (the real capture
        // channels). Score every name match and take the best for the requested
        // direction, so an input pick lands on the capture source — not the sink
        // monitor — and grabs the variant exposing the most channels.
        let devs = if input {
            host.input_devices()
        } else {
            host.output_devices()
        };
        let best = devs.ok().and_then(|it| {
            it.filter(|d| device_name(d).contains(n))
                .max_by_key(|d| device_score(d, input))
        });
        if let Some(d) = best {
            return Ok(d);
        }
        tracing::warn!(
            device = n,
            "daw-audio-io: named audio device not found; using default"
        );
    }
    let def = if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    };
    def.ok_or_else(|| {
        format!(
            "no default audio {} device",
            if input { "input" } else { "output" }
        )
    })
}
