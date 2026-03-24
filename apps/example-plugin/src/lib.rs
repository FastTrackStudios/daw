//! Example CLAP plugin with DAW API access.
//!
//! Uses `daw::init()` / `daw::get()` — fully DAW-agnostic.
//! The same code works in REAPER, standalone, or any future host.

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
        // One line — DAW-agnostic initialization
        daw::init(context.raw_host_context());
        daw::register_timer(timer_callback);
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

/// Timer callback — uses `daw::get()` for fully DAW-agnostic access.
fn timer_callback() {
    let Some(daw) = daw::get() else { return };

    let result = daw::block_on(async {
        let ext = daw.ext_state();
        let project = daw.current_project().await.ok()?;
        let all_tracks = project.tracks().all().await.ok()?;
        let count = all_tracks.len();

        let _ = ext
            .set(
                "FTS_EXAMPLE_PLUGIN",
                "track_count",
                &count.to_string(),
                false,
            )
            .await;

        static TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let tick = TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = ext
            .set("FTS_EXAMPLE_PLUGIN", "tick", &tick.to_string(), false)
            .await;

        Some(())
    });
    let _ = result;
}
