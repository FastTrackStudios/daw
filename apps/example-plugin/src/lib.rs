//! Example CLAP plugin with DAW API access.
//!
//! Uses `daw::reaper::PluginHost` — no direct reaper-rs dependency.
//! The plugin only knows about the `daw` crate.

use nih_plug::prelude::*;
use std::sync::Arc;

const PLUGIN_NAME: &str = "DAW Example Plugin";

#[derive(Params)]
struct ExampleParams {
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for ExampleParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new("Gain", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

struct ExamplePlugin {
    params: Arc<ExampleParams>,
}

impl Default for ExamplePlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(ExampleParams::default()),
        }
    }
}

impl Plugin for ExamplePlugin {
    const NAME: &'static str = PLUGIN_NAME;
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "https://fasttrackstudio.com";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: std::num::NonZeroU32::new(2),
        main_output_channels: std::num::NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        // One line: get DAW host access via the CLAP host extension
        if let Some(host) = daw::reaper::PluginHost::init(context.raw_host_context()) {
            host.register_timer(timer_callback);
        }

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let gain = self.params.gain.value();
        for channel in buffer.iter_samples() {
            for sample in channel {
                *sample *= gain;
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ExamplePlugin {
    const CLAP_ID: &'static str = "com.fasttrackstudio.example-plugin";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Example CLAP plugin with DAW API access");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Utility];
}

nih_export_clap!(ExamplePlugin);

/// Timer callback — runs at ~30Hz on the DAW's main thread.
/// Uses only `daw::reaper::PluginHost` — no reaper-rs.
fn timer_callback() {
    let Some(host) = daw::reaper::PluginHost::get() else {
        return;
    };

    let track_count = host.track_count();

    // Write to ExtState so integration tests can verify
    host.set_ext_state(
        "FTS_EXAMPLE_PLUGIN",
        "track_count",
        &track_count.to_string(),
    );

    static TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let tick = TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    host.set_ext_state("FTS_EXAMPLE_PLUGIN", "tick", &tick.to_string());
}
