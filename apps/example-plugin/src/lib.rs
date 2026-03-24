//! Example CLAP plugin with REAPER API access.
//!
//! Demonstrates how a CLAP plugin can call daw-reaper services:
//!
//! 1. During `initialize()`, gets the CLAP host pointer via `raw_host_context()`
//! 2. Calls `init_from_clap_host()` to get REAPER API access
//! 3. Registers a timer callback that reads track info via reaper-high
//! 4. Logs track count to `/tmp/example-plugin.log` on each tick
//!
//! Build and install:
//! ```sh
//! cargo xtask bundle example-plugin
//! cp target/bundled/example-plugin.clap ~/.config/REAPER/UserPlugins/FX/
//! ```

use nih_plug::prelude::*;
use std::sync::Arc;

const PLUGIN_NAME: &str = "DAW Example Plugin";

// ── Parameters ──────────────────────────────────────────────────────

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

// ── Plugin ──────────────────────────────────────────────────────────

struct ExamplePlugin {
    params: Arc<ExampleParams>,
    reaper_initialized: bool,
}

impl Default for ExamplePlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(ExampleParams::default()),
            reaper_initialized: false,
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
        // Get the CLAP host pointer — this is how we access the REAPER API
        if let Some(host_ptr) = context.raw_host_context() {
            // Initialize daw-reaper via the CLAP host extension
            let result = unsafe { daw::reaper::bootstrap::init_from_clap_host(host_ptr) };
            if let Some(mut bootstrap) = result {
                // Register a timer callback for periodic work
                match bootstrap.session.plugin_register_add_timer(timer_callback) {
                    Ok(_) => tracing::info!("{PLUGIN_NAME}: timer registered"),
                    Err(e) => tracing::warn!("{PLUGIN_NAME}: timer failed: {e:?}"),
                }
                let _ = Box::leak(Box::new(bootstrap.session));
                // Store middleware for the timer — leak for simplicity in example
                let _ = Box::leak(Box::new(std::sync::Mutex::new(bootstrap.middleware)));
                self.reaper_initialized = true;
                tracing::info!("{PLUGIN_NAME}: REAPER API initialized via CLAP host extension");
            } else {
                tracing::info!("{PLUGIN_NAME}: not running in REAPER");
            }
        } else {
            tracing::info!("{PLUGIN_NAME}: no raw host context (not CLAP?)");
        }

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Simple gain passthrough
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
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Example CLAP plugin with REAPER API access via daw-reaper");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Utility];
}

nih_export_clap!(ExamplePlugin);

// ── Timer callback ──────────────────────────────────────────────────

/// Called at ~30Hz on REAPER's main thread.
/// Demonstrates calling daw-reaper APIs from a CLAP plugin.
extern "C" fn timer_callback() {
    use daw::reaper::bootstrap::HighReaper;

    let reaper = HighReaper::get();
    let project = reaper.current_project();
    let track_count = project.track_count();

    // Log to file (console logging would be too noisy at 30Hz)
    static TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let tick = TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Log every ~5 seconds (150 ticks at 30Hz)
    if tick % 150 == 0 {
        tracing::info!("[example-plugin] tick={tick}, tracks={track_count}",);
    }
}
